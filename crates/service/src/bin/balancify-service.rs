//! Balancify service binary -- Windows service core with UDP I/O adapters.
//! Blueprint Sec.4.2, Sec.11.1 Phase 4 MVP.
//!
//! This binary demonstrates the sans-I/O service core wired to real UDP sockets
//! (one per uplink), with secure handshake, pacing, congestion control, rekey,
//! and dual-relay failover. It also serves IPC v1 request/response and
//! subscribed event frames on a loopback-only TCP adapter for the lab; production uses an ACL-protected
//! Windows named pipe. In production, packets come from the NDIS miniport.

use balancify_core::{UplinkId, FlowKey};
use balancify_crypto::{FileKeyStore, KeyStore, PinnedKeys, TunnelCipher};
use balancify_service::ipc::server::IpcServer;
use balancify_service::ipc::{Event as IpcEvent, Request, Response, Status};
use balancify_service::{Service, ServiceConfig, Action};
use std::collections::BTreeMap;
use std::net::{UdpSocket, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionMode {
    Secure,
    InsecureLab,
}

fn select_connection_mode(
    has_relay_identity: bool,
    insecure_lab_requested: bool,
    debug_build: bool,
) -> std::io::Result<ConnectionMode> {
    if has_relay_identity {
        return Ok(ConnectionMode::Secure);
    }
    if !insecure_lab_requested {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "no relay identity configured; refusing zero-key transport (use --insecure-lab for loopback-only debug runs)",
        ));
    }
    if !debug_build {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "insecure lab mode is disabled in release builds",
        ));
    }
    Ok(ConnectionMode::InsecureLab)
}

fn parse_relay_public_key(encoded: &str) -> std::io::Result<[u8; 32]> {
    let bytes = hex_decode(encoded).filter(|bytes| bytes.len() == 32).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "relay public key must be exactly 32 bytes of hex")
    })?;
    let public: [u8; 32] = bytes.try_into().expect("length checked above");
    if public == [0; 32] {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "all-zero relay public key is not a valid identity",
        ));
    }
    Ok(public)
}

fn resolve_relay_public(
    configured: Option<[u8; 32]>,
    pinned: &PinnedKeys,
) -> std::io::Result<Option<[u8; 32]>> {
    match configured {
        Some(public) if public == [0; 32] => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "all-zero relay public key is not a valid identity",
        )),
        Some(public) if !pinned.is_empty() && !pinned.is_pinned(&public) => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "configured relay key does not match any pinned identity",
        )),
        Some(public) => Ok(Some(public)),
        None => match pinned.iter().next().copied() {
            Some(public) if public == [0; 32] => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "stored pinned relay identity is all zeroes",
            )),
            Some(public) => Ok(Some(public)),
            None => Ok(None),
        },
    }
}

fn is_expected_relay_source(source: SocketAddr, expected_relay: SocketAddr) -> bool {
    source == expected_relay
}

fn send_datagram(sock: &UdpSocket, bytes: &[u8], destination: SocketAddr) -> std::io::Result<()> {
    let sent = sock.send_to(bytes, destination)?;
    if sent != bytes.len() {
        return Err(std::io::Error::new(std::io::ErrorKind::WriteZero, "short UDP datagram send"));
    }
    Ok(())
}

fn validate_lab_target(mode: ConnectionMode, primary: SocketAddr, secondary: Option<SocketAddr>) -> std::io::Result<()> {
    if mode == ConnectionMode::InsecureLab
        && (!primary.ip().is_loopback() || secondary.is_some_and(|addr| !addr.ip().is_loopback()))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "insecure lab mode is restricted to loopback relay addresses",
        ));
    }
    Ok(())
}

fn choose_state_dir(
    override_dir: Option<std::path::PathBuf>,
    program_data: Option<std::path::PathBuf>,
    xdg_state_home: Option<std::path::PathBuf>,
    home: Option<std::path::PathBuf>,
    is_windows: bool,
) -> std::io::Result<std::path::PathBuf> {
    if let Some(path) = override_dir {
        if !path.is_absolute() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "BALANCIFY_STATE_DIR must be an absolute path"));
        }
        return Ok(path);
    }
    if is_windows {
        return program_data
            .filter(|path| path.is_absolute())
            .map(|path| path.join("Balancify"))
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "absolute ProgramData or BALANCIFY_STATE_DIR is required"));
    }
    if let Some(path) = xdg_state_home.filter(|path| path.is_absolute()) {
        return Ok(path.join("balancify-service"));
    }
    home.filter(|path| path.is_absolute())
        .map(|path| path.join(".local").join("state").join("balancify-service"))
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "absolute XDG_STATE_HOME, HOME, or BALANCIFY_STATE_DIR is required"))
}

fn default_state_dir() -> std::io::Result<std::path::PathBuf> {
    let override_dir = std::env::var_os("BALANCIFY_STATE_DIR").map(std::path::PathBuf::from);
    #[cfg(windows)]
    let program_data = std::env::var_os("ProgramData").map(std::path::PathBuf::from);
    #[cfg(not(windows))]
    let program_data = None;
    #[cfg(not(windows))]
    let xdg_state_home = std::env::var_os("XDG_STATE_HOME").map(std::path::PathBuf::from);
    #[cfg(windows)]
    let xdg_state_home = None;
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from);
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    choose_state_dir(override_dir, program_data, xdg_state_home, home, cfg!(windows))
}

fn main() -> std::io::Result<()> {
    let mut positional = Vec::new();
    let mut insecure_lab_requested = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--insecure-lab" => insecure_lab_requested = true,
            "--help" | "-h" => {
                println!("Usage: balancify-service [--insecure-lab] [PRIMARY_RELAY_ADDR] [SECONDARY_RELAY_ADDR]");
                println!("Without a relay identity, the service fails closed. Insecure lab mode is debug-only and loopback-only.");
                return Ok(());
            }
            _ if arg.starts_with('-') => return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("unknown option: {arg}"))),
            _ => positional.push(arg),
        }
    }
    if positional.len() > 2 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "expected at most primary and secondary relay addresses"));
    }
    let relay_primary: SocketAddr = positional.first()
        .map(|value| value.parse())
        .transpose()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid primary relay address"))?
        .unwrap_or_else(|| "127.0.0.1:4430".parse().unwrap());
    let relay_secondary: Option<SocketAddr> = positional.get(1)
        .map(|value| value.parse())
        .transpose()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid secondary relay address"))?;

    let state_dir = default_state_dir()?;
    let secret_path = state_dir.join("client_secret.key");
    let pinned_path = state_dir.join("pinned_relays.keys");
    let mut store = FileKeyStore::new(&secret_path, &pinned_path);
    let keypair = store.load_or_generate().map_err(|error| std::io::Error::other(format!("failed to load or persist service identity: {error}")))?;
    let pinned_relays = store.load_pinned().map_err(|error| std::io::Error::other(format!("failed to load pinned relay identities: {error}")))?;
    let configured_relay = match std::env::var("BALANCIFY_RELAY_PUBLIC_KEY") {
        Ok(encoded) => Some(parse_relay_public_key(&encoded)?),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(std::io::Error::other(format!("invalid relay identity setting: {error}"))),
    };
    let relay_public = resolve_relay_public(configured_relay, &pinned_relays)?;
    let mode = select_connection_mode(relay_public.is_some(), insecure_lab_requested, cfg!(debug_assertions))?;
    validate_lab_target(mode, relay_primary, relay_secondary)?;

    println!("Balancify service -- state_dir={}, primary={}, secondary={:?}", state_dir.display(), relay_primary, relay_secondary);
    println!("  public key: {:02x?}...", &keypair.public_bytes()[..8]);

    let cfg = ServiceConfig {
        session: 1,
        seed: 0xBEEF,
        bond_enabled: true,
        duplicate_mode: false,
        allow_metered: false,
        hold_cap: Duration::from_millis(80),
        rto: Duration::from_millis(120),
        idle_timeout: Duration::from_secs(120),
        keepalive_interval: Duration::from_secs(5),
        tunnel_overhead: 60,
        idle_probe_bytes: 1024,
        probe_timeout: Duration::from_secs(5),
        auto_accept: false,
    };
    let dummy_key = [0u8; 32];
    let dummy_salt = [0u8; 3];
    let dummy_cipher = TunnelCipher::new(&dummy_key, dummy_salt).unwrap();
    let mut svc = Service::new(cfg, dummy_cipher.clone(), dummy_cipher);
    svc.set_static_keypair(keypair);
    if !pinned_relays.is_empty() {
        svc.set_pinned_relays(pinned_relays);
    }
    svc.set_relays(relay_primary, relay_secondary);
    svc.set_relay_failover_timeout(Duration::from_secs(10));

    // Simulate two uplinks with bound sockets
    let sock0 = UdpSocket::bind("0.0.0.0:0")?;
    let sock1 = UdpSocket::bind("0.0.0.0:0")?;
    // Keep the single-threaded service/IPC dispatcher responsive: blocking
    // reads on both sockets could otherwise stall it for 200 ms per loop.
    sock0.set_nonblocking(true)?;
    sock1.set_nonblocking(true)?;
    let addrs = [sock0.local_addr()?, sock1.local_addr()?];
    println!("  uplink sockets: {:?} (simulating Ethernet + Wi-Fi)", addrs);

    let now = Instant::now();
    svc.add_uplink(UplinkId(0), 200_000_000, false, 1500, now);
    svc.add_uplink(UplinkId(1), 100_000_000, false, 1500, now);
    for _ in 0..5 {
        svc.on_probe_ok(UplinkId(0), Duration::from_millis(10), now);
        svc.on_probe_ok(UplinkId(1), Duration::from_millis(35), now);
    }
    svc.publish(now);

    let (ipc_calls, ipc_addr) = start_ipc_listener()?;
    println!("  IPC v1 loopback listener: {}", ipc_addr);

    // Handshake
    match mode {
        ConnectionMode::Secure => {
            let relay_identity = relay_public.expect("secure mode requires a relay identity");
            println!("  initiating authenticated handshake to primary...");
            let prologue = b"balancify handshake v1";
            let msg1 = svc.initiate_handshake(relay_identity, prologue)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::PermissionDenied, format!("handshake initialization failed: {error:?}")))?;
            send_datagram(&sock0, &msg1, relay_primary)?;
            println!("  sent msg1 {} bytes", msg1.len());
        }
        ConnectionMode::InsecureLab => {
            eprintln!("  WARNING: explicitly enabled INSECURE lab mode; loopback only, zero-key test cipher");
            svc.open_session(1, now);
        }
    }

    let mut dg = [0u8; 65535];
    let mut flow_counter = 0u64;
    let mut last_tick = Instant::now();
    let mut last_stats = Instant::now();

    loop {
        let now = Instant::now();

        // Keep service state on this thread; the socket workers only decode
        // frames and forward requests through the channel.
        while let Ok(call) = ipc_calls.try_recv() {
            match call {
                IpcCall::Request(request, reply) => {
                    let response = svc.handle(request, now);
                    let _ = reply.send(response);
                }
                IpcCall::PollEvents(reply) => {
                    let _ = reply.send(svc.telemetry_events());
                }
            }
        }

        // Tick every 100ms
        if now.duration_since(last_tick) >= Duration::from_millis(100) {
            let actions = svc.tick(now);
            for act in actions {
                if let Action::Send { uplink, bytes } = act {
                    let sock = if uplink.0 == 0 { &sock0 } else { &sock1 };
                    let relay = svc.relay_current().unwrap_or(relay_primary);
                    send_datagram(sock, &bytes, relay)?;
                }
            }
            last_tick = now;
        }
        // Stats every 5s
        if now.duration_since(last_stats) >= Duration::from_secs(5) {
            let c = svc.counters();
            println!("Stats: stripped={} forwarded={} delivered={} held={} evicted={} failovers={} current_relay={:?}",
                c.packets_stripped, c.packets_forwarded, c.delivered, c.held, c.sessions_evicted,
                svc.relay_failover_count(), svc.relay_current());
            last_stats = now;
        }

        // Poll both sockets
        for (uplink_id, sock) in [(UplinkId(0), &sock0), (UplinkId(1), &sock1)] {
            match sock.recv_from(&mut dg) {
                Ok((n, from)) => {
                    let expected_relay = svc.relay_current().unwrap_or(relay_primary);
                    if !is_expected_relay_source(from, expected_relay) {
                        eprintln!("ignoring datagram from an unexpected relay endpoint");
                        continue;
                    }
                    // Record local endpoint for NAT rebinding detection
                    if let Ok(local) = sock.local_addr() {
                        svc.note_local_endpoint(uplink_id, local, now);
                    }
                    match svc.handle_path_datagram(uplink_id, &dg[..n], now) {
                        Ok(actions) => {
                            for act in actions {
                                match act {
                                    Action::Deliver { data } => {
                                        println!("Deliver {} bytes from {}: {:?}", data.len(), from, String::from_utf8_lossy(&data[..data.len().min(64)]));
                                    }
                                    Action::Send { uplink, bytes } => {
                                        let s = if uplink.0 == 0 { &sock0 } else { &sock1 };
                                        let relay = svc.relay_current().unwrap_or(relay_primary);
                                        send_datagram(s, &bytes, relay)?;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Handshake error from {}: {:?}", from, e);
                            // On handshake failure, try secondary if available
                            if let Some(sec) = relay_secondary {
                                if svc.relay_current() == Some(relay_primary) {
                                    println!("Trying secondary relay {}", sec);
                                    svc.set_relays(sec, Some(relay_primary));
                                }
                            }
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => return Err(e),
            }
        }

        // Simulate app packet every second
        if now.elapsed().as_secs() > flow_counter {
            flow_counter = now.elapsed().as_secs();
            let src: SocketAddr = format!("10.0.0.1:{}", 5000 + (flow_counter % 1000) as u16).parse().unwrap();
            let dst: SocketAddr = "8.8.8.8:443".parse().unwrap();
            let flow = FlowKey::new(6, src, dst);
            let payload = format!("app packet {} at {:?}", flow_counter, now);
            let actions = svc.send_app_packet(&flow, None, payload.as_bytes(), now);
            for act in actions {
                if let Action::Send { uplink, bytes } = act {
                    let sock = if uplink.0 == 0 { &sock0 } else { &sock1 };
                    let relay = svc.relay_current().unwrap_or(relay_primary);
                    send_datagram(sock, &bytes, relay)?;
                }
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

const MAX_IPC_CLIENTS: usize = 16;

enum IpcCall {
    Request(Request, Sender<Response>),
    PollEvents(Sender<Vec<IpcEvent>>),
}

struct ActiveIpcClient(Arc<AtomicUsize>);

impl Drop for ActiveIpcClient {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn reserve_ipc_client_slot(active: &AtomicUsize) -> bool {
    let mut count = active.load(Ordering::Acquire);
    loop {
        if count >= MAX_IPC_CLIENTS {
            return false;
        }
        match active.compare_exchange_weak(
            count,
            count + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return true,
            Err(observed) => count = observed,
        }
    }
}

/// Start the lab TCP adapter on loopback. The actual product transport is an
/// ACL-protected Windows named pipe; keeping this adapter local-only prevents
/// accidental exposure of control operations to the LAN.
fn start_ipc_listener() -> std::io::Result<(Receiver<IpcCall>, SocketAddr)> {
    let port_value = match std::env::var("BALANCIFY_IPC_PORT") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => "5000".to_string(),
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "BALANCIFY_IPC_PORT must be valid UTF-8",
            ));
        }
    };
    let port = port_value.parse::<u16>().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "BALANCIFY_IPC_PORT must be an integer in 0..=65535",
        )
    })?;
    let bind_addr = SocketAddr::from(([127, 0, 0, 1], port));
    let server = IpcServer::bind_loopback_at(bind_addr)?;
    let local_addr = server.addr();
    let (request_tx, request_rx) = mpsc::channel::<IpcCall>();
    let active_clients = Arc::new(AtomicUsize::new(0));

    std::thread::Builder::new()
        .name("balancify-ipc-accept".to_string())
        .spawn(move || loop {
            if let Some(stream) = server.accept() {
                if !reserve_ipc_client_slot(&active_clients) {
                    eprintln!("IPC client limit reached ({MAX_IPC_CLIENTS}); rejecting local connection");
                    drop(stream);
                    continue;
                }
                let slot = ActiveIpcClient(Arc::clone(&active_clients));
                let requests = request_tx.clone();
                let event_requests = request_tx.clone();
                let _ = std::thread::Builder::new()
                    .name("balancify-ipc-client".to_string())
                    .spawn(move || {
                        let _slot = slot;
                        let _ = balancify_service::ipc::server::handle_stream_with_events(
                            stream,
                            move |request| {
                                let (reply_tx, reply_rx) = mpsc::channel();
                                if requests.send(IpcCall::Request(request, reply_tx)).is_err() {
                                    return Response::Error {
                                        status: Status::Unavailable,
                                        message: "service IPC dispatcher stopped".to_string(),
                                    };
                                }
                                reply_rx.recv().unwrap_or_else(|_| Response::Error {
                                    status: Status::Unavailable,
                                    message: "service did not return an IPC response".to_string(),
                                })
                            },
                            move || {
                                let (reply_tx, reply_rx) = mpsc::channel();
                                if event_requests.send(IpcCall::PollEvents(reply_tx)).is_err() {
                                    return Vec::new();
                                }
                                reply_rx.recv().unwrap_or_default()
                            },
                        );
                    });
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        })?;

    Ok((request_rx, local_addr))
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    let bytes = s.as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        out.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
    }
    Some(out)
}

#[cfg(test)]
mod ipc_capacity_tests {
    use super::*;

    #[test]
    fn service_requires_authenticated_transport_by_default() {
        let error = select_connection_mode(false, false, true).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(select_connection_mode(true, false, false).unwrap(), ConnectionMode::Secure);
        assert_eq!(select_connection_mode(true, true, false).unwrap(), ConnectionMode::Secure);
    }

    #[test]
    fn insecure_lab_mode_is_debug_only_and_loopback_only() {
        assert_eq!(
            select_connection_mode(false, true, true).unwrap(),
            ConnectionMode::InsecureLab
        );
        assert_eq!(
            select_connection_mode(false, true, false).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        let loopback: SocketAddr = "127.0.0.1:4430".parse().unwrap();
        let public: SocketAddr = "192.0.2.1:4430".parse().unwrap();
        assert!(is_expected_relay_source(loopback, loopback));
        assert!(!is_expected_relay_source(public, loopback));
        validate_lab_target(ConnectionMode::InsecureLab, loopback, Some(loopback)).unwrap();
        assert!(validate_lab_target(ConnectionMode::InsecureLab, loopback, Some(public)).is_err());
        assert!(validate_lab_target(ConnectionMode::InsecureLab, public, None).is_err());
        validate_lab_target(ConnectionMode::Secure, public, None).unwrap();
    }

    #[test]
    fn relay_identity_parsing_and_pins_fail_closed() {
        let encoded = format!("01{}", "00".repeat(31));
        let public = parse_relay_public_key(&encoded).unwrap();
        assert_eq!(public[0], 1);
        assert!(parse_relay_public_key("00").is_err());
        assert!(parse_relay_public_key(&"00".repeat(32)).is_err());
        assert!(parse_relay_public_key(&format!(" {encoded}")).is_err());

        let mut pinned = PinnedKeys::new();
        pinned.pin(public);
        assert_eq!(resolve_relay_public(None, &pinned).unwrap(), Some(public));
        assert_eq!(resolve_relay_public(Some(public), &pinned).unwrap(), Some(public));
        assert_eq!(
            resolve_relay_public(Some([2; 32]), &pinned).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert_eq!(resolve_relay_public(None, &PinnedKeys::new()).unwrap(), None);
    }

    #[test]
    fn service_state_directory_requires_an_absolute_secure_root() {
        let root = std::env::temp_dir();
        assert!(choose_state_dir(Some("relative".into()), None, None, None, false).is_err());
        assert_eq!(
            choose_state_dir(None, None, Some(root.join("xdg")), None, false).unwrap(),
            root.join("xdg").join("balancify-service")
        );
        assert_eq!(
            choose_state_dir(None, Some(root.clone()), None, None, true).unwrap(),
            root.join("Balancify")
        );
        assert!(choose_state_dir(None, None, None, None, true).is_err());
    }

    #[test]
    fn udp_datagram_send_reaches_the_requested_local_endpoint() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let destination = receiver.local_addr().unwrap();
        send_datagram(&sender, b"test datagram", destination).unwrap();
        let mut buffer = [0; 32];
        let (length, source) = receiver.recv_from(&mut buffer).unwrap();
        assert_eq!(&buffer[..length], b"test datagram");
        assert_eq!(source, sender.local_addr().unwrap());
    }

    #[test]
    fn local_ipc_worker_slots_are_bounded_and_released() {
        let active = Arc::new(AtomicUsize::new(0));
        for _ in 0..MAX_IPC_CLIENTS {
            assert!(reserve_ipc_client_slot(&active));
        }
        assert_eq!(active.load(Ordering::Acquire), MAX_IPC_CLIENTS);
        assert!(!reserve_ipc_client_slot(&active));

        drop(ActiveIpcClient(Arc::clone(&active)));
        assert_eq!(active.load(Ordering::Acquire), MAX_IPC_CLIENTS - 1);
        assert!(reserve_ipc_client_slot(&active));
        assert_eq!(active.load(Ordering::Acquire), MAX_IPC_CLIENTS);
    }
}
