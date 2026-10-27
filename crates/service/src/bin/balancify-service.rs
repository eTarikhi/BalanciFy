//! Balancify service binary -- Windows service core with UDP I/O adapters.
//! Blueprint Sec.4.2, Sec.11.1 Phase 4 MVP.
//!
//! This binary demonstrates the sans-I/O service core wired to real UDP sockets
//! (one per uplink), with secure handshake, pacing, congestion control, key-update diagnostics,
//! and dual-relay failover. It also serves IPC v1 request/response and
//! subscribed event frames over loopback TCP on non-Windows lab hosts or an
//! ACL-protected Windows named pipe. In production, packets come from the NDIS miniport.

use balancify_core::UplinkId;
use balancify_crypto::{FileKeyStore, KeyStore, PinnedKeys, TunnelCipher};
use balancify_service::driver_device::{is_driver_not_ready, should_retry_driver_tx, DriverDevice};
use balancify_service::ipc::server::{IpcServer, IpcStream};
use balancify_service::ipc::{Event as IpcEvent, Request, Response, Status};
use balancify_service::machine_policy::MachinePolicyStore;
use balancify_service::{Service, ServiceConfig, Action};
use std::collections::VecDeque;
use std::net::{UdpSocket, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

const RELAY_FAILOVER_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PENDING_DRIVER_RX: usize = 1024;

fn queue_pending_driver_rx(queue: &mut VecDeque<Vec<u8>>, packet: Vec<u8>) -> std::io::Result<()> {
    if queue.len() >= MAX_PENDING_DRIVER_RX {
        return Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "bounded inbound driver retry queue is full; refusing to discard tunnel payload",
        ));
    }
    queue.push_back(packet);
    Ok(())
}

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

fn parse_remote_relay_address(value: &str, label: &str) -> std::io::Result<SocketAddr> {
    let address = value.parse::<SocketAddr>().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid {label} relay address"),
        )
    })?;
    if address.port() == 0 || address.ip().is_unspecified() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{label} relay address must use a nonzero port and specified IP"),
        ));
    }
    Ok(address)
}

fn parse_relay_addresses(
    primary: Option<&str>,
    secondary: Option<&str>,
) -> std::io::Result<(SocketAddr, Option<SocketAddr>)> {
    let primary = primary
        .map(|value| parse_remote_relay_address(value, "primary"))
        .transpose()?
        .unwrap_or_else(|| "127.0.0.1:4430".parse().unwrap());
    let secondary = secondary
        .map(|value| parse_remote_relay_address(value, "secondary"))
        .transpose()?;
    if secondary == Some(primary) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "primary and secondary relay addresses must differ",
        ));
    }
    Ok((primary, secondary))
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

fn configured_relay_public_key(name: &str) -> std::io::Result<Option<[u8; 32]>> {
    match std::env::var(name) {
        Ok(encoded) => parse_relay_public_key(&encoded).map(Some),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{name} must contain a hexadecimal relay identity"),
        )),
    }
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

fn relay_identity_for_address(
    address: SocketAddr,
    primary: SocketAddr,
    primary_identity: Option<[u8; 32]>,
    secondary: Option<SocketAddr>,
    secondary_identity: Option<[u8; 32]>,
) -> Option<[u8; 32]> {
    if address == primary {
        primary_identity
    } else if secondary == Some(address) {
        secondary_identity
    } else {
        None
    }
}

fn initiate_and_send_handshake(
    service: &mut Service<TunnelCipher>,
    relay_identity: [u8; 32],
    socket: &UdpSocket,
    relay_address: SocketAddr,
    now: Instant,
) -> std::io::Result<()> {
    let msg1 = service.initiate_handshake(relay_identity, b"balancify handshake v1")
        .map_err(|error| std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("handshake initialization failed: {error:?}"),
        ))?;
    service.note_relay_attempt(now);
    send_datagram(socket, &msg1, relay_address)
}

fn is_expected_relay_source(source: SocketAddr, expected_relay: SocketAddr) -> bool {
    source == expected_relay
}

fn next_simulated_packet_tick(started_at: Instant, now: Instant, last_tick: u64) -> Option<u64> {
    let elapsed_seconds = now.duration_since(started_at).as_secs();
    (elapsed_seconds > last_tick).then_some(elapsed_seconds)
}

fn validate_simulated_traffic(mode: ConnectionMode, requested: bool) -> std::io::Result<()> {
    if requested && mode != ConnectionMode::InsecureLab {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "synthetic app traffic is restricted to explicit --insecure-lab mode",
        ));
    }
    Ok(())
}

fn build_simulated_ipv4_udp_packet(
    identification: u16,
    source_port: u16,
    payload: &[u8],
) -> std::io::Result<Vec<u8>> {
    let udp_len = 8usize.checked_add(payload.len()).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "synthetic UDP payload is too large")
    })?;
    let total_len = 20usize.checked_add(udp_len).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "synthetic IP packet is too large")
    })?;
    if total_len > balancify_service::packet::MAX_INNER_PACKET_LEN || total_len > u16::MAX as usize {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "synthetic IP packet exceeds the inner-packet limit",
        ));
    }

    let mut packet = vec![0u8; total_len];
    packet[0] = 0x45;
    packet[2..4].copy_from_slice(&(total_len as u16).to_be_bytes());
    packet[4..6].copy_from_slice(&identification.to_be_bytes());
    packet[8] = 64;
    packet[9] = 17;
    packet[12..16].copy_from_slice(&[10, 0, 0, 1]);
    packet[16..20].copy_from_slice(&[8, 8, 8, 8]);
    packet[20..22].copy_from_slice(&source_port.to_be_bytes());
    packet[22..24].copy_from_slice(&443u16.to_be_bytes());
    packet[24..26].copy_from_slice(&(udp_len as u16).to_be_bytes());
    packet[28..].copy_from_slice(payload);
    let header_checksum = ipv4_header_checksum(&packet[..20]);
    packet[10..12].copy_from_slice(&header_checksum.to_be_bytes());
    // A zero UDP checksum is permitted for IPv4 and keeps this synthetic packet
    // builder independent of a transport pseudo-header checksum implementation.
    Ok(packet)
}

fn ipv4_header_checksum(header: &[u8]) -> u16 {
    let mut sum = 0u32;
    for pair in header.chunks_exact(2) {
        sum += u32::from(u16::from_be_bytes([pair[0], pair[1]]));
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn send_datagram(sock: &UdpSocket, bytes: &[u8], destination: SocketAddr) -> std::io::Result<()> {
    let sent = sock.send_to(bytes, destination)?;
    if sent != bytes.len() {
        return Err(std::io::Error::new(std::io::ErrorKind::WriteZero, "short UDP datagram send"));
    }
    Ok(())
}

fn send_scheduled_actions(
    actions: Vec<Action>,
    service: &Service<TunnelCipher>,
    sock0: &UdpSocket,
    sock1: &UdpSocket,
    relay_primary: SocketAddr,
) -> std::io::Result<()> {
    for action in actions {
        if let Action::Send { uplink, bytes } = action {
            let socket = if uplink.0 == 0 { sock0 } else { sock1 };
            let relay = service.relay_current().unwrap_or(relay_primary);
            send_datagram(socket, &bytes, relay)?;
        }
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
    let mut simulate_app_traffic_requested = false;
    let mut driver_device_requested = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--insecure-lab" => insecure_lab_requested = true,
            "--simulate-app-traffic" => simulate_app_traffic_requested = true,
            "--driver-device" => driver_device_requested = true,
            "--help" | "-h" => {
                println!("Usage: balancify-service [--insecure-lab] [--simulate-app-traffic] [--driver-device] [PRIMARY_RELAY_ADDR] [SECONDARY_RELAY_ADDR]");
                println!("Without a relay identity, the service fails closed. Insecure lab mode is debug-only and loopback-only.");
                println!("For a secondary relay with a distinct identity, set BALANCIFY_SECONDARY_RELAY_PUBLIC_KEY; otherwise it uses the primary identity.");
                println!("Synthetic app traffic is opt-in and restricted to --insecure-lab mode.");
                println!("--driver-device opens the Windows control device, injects received tunnel packets, and polls outbound raw-IP packets; it requires the driver and appropriate access.");
                return Ok(());
            }
            _ if arg.starts_with('-') => return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("unknown option: {arg}"))),
            _ => positional.push(arg),
        }
    }
    if positional.len() > 2 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "expected at most primary and secondary relay addresses"));
    }
    let (relay_primary, relay_secondary) = parse_relay_addresses(
        positional.first().map(String::as_str),
        positional.get(1).map(String::as_str),
    )?;
    let configured_relay = configured_relay_public_key("BALANCIFY_RELAY_PUBLIC_KEY")?;
    let configured_secondary_relay = configured_relay_public_key("BALANCIFY_SECONDARY_RELAY_PUBLIC_KEY")?;
    if relay_secondary.is_none() && configured_secondary_relay.is_some() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "BALANCIFY_SECONDARY_RELAY_PUBLIC_KEY requires a secondary relay address",
        ));
    }

    let state_dir = default_state_dir()?;
    let secret_path = state_dir.join("client_secret.key");
    let pinned_path = state_dir.join("pinned_relays.keys");
    let mut store = FileKeyStore::new(&secret_path, &pinned_path);
    let pinned_relays = store.load_pinned().map_err(|error| std::io::Error::other(format!("failed to load pinned relay identities: {error}")))?;
    let relay_public = resolve_relay_public(configured_relay, &pinned_relays)?;
    // By default both endpoints may share a relay identity (for a replicated
    // cluster). Set the secondary key explicitly when it has a distinct pin.
    let secondary_relay_public = if relay_secondary.is_some() {
        match configured_secondary_relay {
            Some(public) => resolve_relay_public(Some(public), &pinned_relays)?,
            None => relay_public,
        }
    } else {
        None
    };
    let mode = select_connection_mode(relay_public.is_some(), insecure_lab_requested, cfg!(debug_assertions))?;
    if mode != ConnectionMode::Secure && configured_secondary_relay.is_some() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a secondary relay identity requires secure mode with a primary relay identity",
        ));
    }
    validate_lab_target(mode, relay_primary, relay_secondary)?;
    validate_simulated_traffic(mode, simulate_app_traffic_requested)?;
    let keypair = store.load_or_generate().map_err(|error| std::io::Error::other(format!("failed to load or persist service identity: {error}")))?;
    let driver_device = if driver_device_requested {
        Some(DriverDevice::open()?)
    } else {
        None
    };

    println!("Balancify service -- state_dir={}, primary={}, secondary={:?}", state_dir.display(), relay_primary, relay_secondary);
    if driver_device.is_some() {
        println!("  NDIS device path: tunnel injection and outbound packet polling enabled");
    }
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
    let policy_store = MachinePolicyStore::open(state_dir.join("machine-policy"))?;
    let dummy_key = [0u8; 32];
    let dummy_salt = [0u8; 3];
    let dummy_cipher = TunnelCipher::new(&dummy_key, dummy_salt).unwrap();
    let (mut svc, recovered_policy) = Service::new_with_machine_policy_store(
        cfg,
        dummy_cipher.clone(),
        dummy_cipher,
        policy_store,
    )?;
    if recovered_policy {
        eprintln!("WARNING: machine policy was recovered from its last valid backup");
    }
    svc.set_static_keypair(keypair);
    if !pinned_relays.is_empty() {
        svc.set_pinned_relays(pinned_relays);
    }
    svc.set_relays(relay_primary, relay_secondary);
    svc.set_relay_failover_timeout(RELAY_FAILOVER_TIMEOUT);

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
    println!("  IPC v1 listener: {}", ipc_addr);

    // Handshake
    let handshake_started_at = Instant::now();
    match mode {
        ConnectionMode::Secure => {
            let relay_identity = relay_public.expect("secure mode requires a relay identity");
            println!("  initiating authenticated handshake to primary...");
            initiate_and_send_handshake(&mut svc, relay_identity, &sock0, relay_primary, handshake_started_at)?;
        }
        ConnectionMode::InsecureLab => {
            eprintln!("  WARNING: explicitly enabled INSECURE lab mode; loopback only, zero-key test cipher");
            svc.open_session(1, now);
        }
    }

    let mut handshake_pending = mode == ConnectionMode::Secure;
    let mut last_handshake_attempt = (mode == ConnectionMode::Secure).then_some(handshake_started_at);
    let mut dg = [0u8; 65535];
    let mut flow_counter = 0u64;
    let started_at = Instant::now();
    let mut last_tick = Instant::now();
    let mut last_stats = Instant::now();
    let mut last_driver_poll = Instant::now();
    // DEQUEUE_TX removes a packet from the kernel queue. Retain it here when
    // the service reports Hold/Refused so temporary congestion or link loss
    // cannot turn a successful miniport send completion into a silent drop.
    let mut pending_driver_tx: Option<Vec<u8>> = None;
    // Retain authenticated inbound payloads in order if the miniport is paused
    // or has not enabled its receive filter yet. The bounded FIFO applies
    // backpressure to relay reads; overflow is a loud error, never a silent drop.
    let mut pending_driver_rx: VecDeque<Vec<u8>> = VecDeque::new();

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
            let relay_before_tick = svc.relay_current();
            let actions = svc.tick(now);
            let relay_after_tick = svc.relay_current();
            if mode == ConnectionMode::Secure && relay_after_tick != relay_before_tick {
                if let Some(relay_address) = relay_after_tick {
                    let relay_identity = relay_identity_for_address(
                        relay_address,
                        relay_primary,
                        relay_public,
                        relay_secondary,
                        secondary_relay_public,
                    ).ok_or_else(|| std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "failed over to a relay without a configured identity",
                    ))?;
                    initiate_and_send_handshake(&mut svc, relay_identity, &sock0, relay_address, now)?;
                    handshake_pending = true;
                    last_handshake_attempt = Some(now);
                }
            }
            for act in actions {
                if mode == ConnectionMode::Secure && handshake_pending {
                    continue;
                }
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

        // Retry retained payloads in FIFO order before reading more relay
        // datagrams. If the adapter is still not ready, preserve the head and
        // apply socket backpressure; service ticks and IPC continue to run.
        while let Some(packet) = pending_driver_rx.pop_front() {
            let Some(device) = driver_device.as_ref() else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "pending driver receive exists without an open control device",
                ));
            };
            match device.inject_nbl(&packet) {
                Ok(()) => println!("Injected {} retained tunnel bytes into the NDIS miniport", packet.len()),
                Err(error) if is_driver_not_ready(&error) => {
                    pending_driver_rx.push_front(packet);
                    break;
                }
                Err(error) => return Err(error),
            }
        }

        // Stop socket reads when the bounded FIFO is full.
        for (uplink_id, sock) in [(UplinkId(0), &sock0), (UplinkId(1), &sock1)] {
            if pending_driver_rx.len() >= MAX_PENDING_DRIVER_RX {
                break;
            }
            match sock.recv_from(&mut dg) {
                Ok((n, from)) => {
                    let expected_relay = svc.relay_current().unwrap_or(relay_primary);
                    if !is_expected_relay_source(from, expected_relay) {
                        eprintln!("ignoring datagram from an unexpected relay endpoint");
                        continue;
                    }
                    if mode == ConnectionMode::Secure
                        && handshake_pending
                        && !(n >= 2 && dg[0] == 0xFF && dg[1] == 0x02)
                    {
                        continue;
                    }
                    // Record local endpoint for NAT rebinding detection
                    if let Ok(local) = sock.local_addr() {
                        svc.note_local_endpoint(uplink_id, local, now);
                    }
                    match svc.handle_path_datagram(uplink_id, &dg[..n], now) {
                        Ok(actions) => {
                            if mode == ConnectionMode::Secure
                                && n >= 2
                                && dg[0] == 0xFF
                                && dg[1] == 0x02
                            {
                                handshake_pending = false;
                            }
                            for act in actions {
                                match act {
                                    Action::Deliver { data } => {
                                        if let Some(device) = &driver_device {
                                            if !pending_driver_rx.is_empty() {
                                                queue_pending_driver_rx(&mut pending_driver_rx, data)?;
                                            } else {
                                                match device.inject_nbl(&data) {
                                                    Ok(()) => println!("Injected {} received tunnel bytes into the NDIS miniport", data.len()),
                                                    Err(error) if is_driver_not_ready(&error) => {
                                                        queue_pending_driver_rx(&mut pending_driver_rx, data)?;
                                                    }
                                                    Err(error) => return Err(error),
                                                }
                                            }
                                        } else {
                                            println!("Received {} tunnel bytes from {} (not injected; pass --driver-device to use the miniport)", data.len(), from);
                                        }
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
                            // If a handshake was pending, keep it pending for the bounded
                            // fresh-flight retry. An unsolicited/duplicate response must
                            // not suspend an already-established secure session.
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => return Err(e),
            }
        }

        // Retry only after the full failure-detection interval. This gives a
        // delayed response time to match its single-use Noise initiator; a
        // secondary switch sends a fresh flight immediately from the tick above.
        if handshake_pending
            && last_handshake_attempt.is_some_and(|last| now.duration_since(last) >= RELAY_FAILOVER_TIMEOUT)
        {
            let relay_address = svc.relay_current().unwrap_or(relay_primary);
            let relay_identity = relay_identity_for_address(
                relay_address,
                relay_primary,
                relay_public,
                relay_secondary,
                secondary_relay_public,
            ).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "current relay has no configured identity",
            ))?;
            initiate_and_send_handshake(&mut svc, relay_identity, &sock0, relay_address, now)?;
            last_handshake_attempt = Some(now);
        }

        // Poll a bounded batch from the miniport's driver-owned TX queue only
        // after a receiver session exists (authenticated handshake or explicit
        // insecure lab session). This keeps packets queued before handshake and
        // prevents use of the placeholder cipher. Each record is validated by
        // DriverDevice and revalidated by Service before tunnel scheduling.
        // Keep the first packet whose service action is Hold/Refused and retry
        // it before dequeuing more, preserving FIFO and bounding extra memory to
        // one packet while the kernel queue supplies backpressure.
        if let Some(device) = &driver_device {
            if svc.sessions() > 0
                && !handshake_pending
                && now.duration_since(last_driver_poll) >= Duration::from_millis(10)
            {
                last_driver_poll = now;
                let mut budget = 64;
                if let Some(packet) = pending_driver_tx.take() {
                    budget -= 1;
                    let actions = svc.send_ip_packet(&packet, now).map_err(|error| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                    })?;
                    if should_retry_driver_tx(&actions) {
                        pending_driver_tx = Some(packet);
                    } else {
                        send_scheduled_actions(actions, &svc, &sock0, &sock1, relay_primary)?;
                    }
                }

                if pending_driver_tx.is_none() {
                    for _ in 0..budget {
                        let Some(packet) = device.dequeue_tx()? else { break };
                        let actions = svc.send_ip_packet(&packet, now).map_err(|error| {
                            std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                        })?;
                        if should_retry_driver_tx(&actions) {
                            pending_driver_tx = Some(packet);
                            break;
                        }
                        send_scheduled_actions(actions, &svc, &sock0, &sock1, relay_primary)?;
                    }
                }
            }
        }

        // Optional synthetic raw-IP traffic is restricted to the explicit
        // loopback-only insecure lab mode; normal secure operation never sends
        // fabricated application packets to a configured relay.
        if simulate_app_traffic_requested {
            if let Some(elapsed_seconds) = next_simulated_packet_tick(started_at, now, flow_counter) {
                flow_counter = elapsed_seconds;
                let payload = format!("BalanciFy lab datagram {flow_counter}");
                let packet = build_simulated_ipv4_udp_packet(
                    flow_counter as u16,
                    5000 + (flow_counter % 1000) as u16,
                    payload.as_bytes(),
                )?;
                let actions = svc.send_ip_packet(&packet, now).map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                })?;
                for act in actions {
                    if let Action::Send { uplink, bytes } = act {
                        let sock = if uplink.0 == 0 { &sock0 } else { &sock1 };
                        let relay = svc.relay_current().unwrap_or(relay_primary);
                        send_datagram(sock, &bytes, relay)?;
                    }
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

fn spawn_ipc_client<S>(
    stream: S,
    requests_tx: Sender<IpcCall>,
    active_clients: Arc<AtomicUsize>,
) where
    S: IpcStream + Send + 'static,
{
    if !reserve_ipc_client_slot(&active_clients) {
        eprintln!("IPC client limit reached ({MAX_IPC_CLIENTS}); rejecting local connection");
        return;
    }
    let slot = ActiveIpcClient(active_clients);
    let requests = requests_tx.clone();
    let event_requests = requests_tx;
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
}

fn start_loopback_tcp_listener(
    request_tx: Sender<IpcCall>,
    request_rx: Receiver<IpcCall>,
    active_clients: Arc<AtomicUsize>,
) -> std::io::Result<(Receiver<IpcCall>, String)> {
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
    let local_addr = server.addr().to_string();
    std::thread::Builder::new()
        .name("balancify-ipc-accept".to_string())
        .spawn(move || loop {
            if let Some(stream) = server.accept() {
                spawn_ipc_client(
                    stream,
                    request_tx.clone(),
                    Arc::clone(&active_clients),
                );
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        })?;
    Ok((request_rx, local_addr))
}

/// Windows defaults to ACL-protected named-pipe IPC. The explicit debug-only
/// override keeps the loopback CLI usable during native Windows development;
/// non-Windows lab hosts retain loopback TCP.
fn start_ipc_listener() -> std::io::Result<(Receiver<IpcCall>, String)> {
    let (request_tx, request_rx) = mpsc::channel::<IpcCall>();
    let active_clients = Arc::new(AtomicUsize::new(0));

    #[cfg(windows)]
    {
        match std::env::var("BALANCIFY_IPC_TRANSPORT") {
            Ok(value) if value == "loopback-tcp" && cfg!(debug_assertions) => {
                return start_loopback_tcp_listener(request_tx, request_rx, active_clients);
            }
            Ok(value) if value == "loopback-tcp" => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "loopback TCP IPC override is available only in debug builds",
                ));
            }
            Ok(value) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("unsupported BALANCIFY_IPC_TRANSPORT value: {value}"),
                ));
            }
            Err(std::env::VarError::NotPresent) => {}
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "BALANCIFY_IPC_TRANSPORT must be valid UTF-8",
                ));
            }
        }

        let mut server = balancify_service::ipc::server::NamedPipeServer::bind(
            balancify_service::ipc::server::WINDOWS_SERVICE_PIPE_NAME,
        )?;
        std::thread::Builder::new()
            .name("balancify-ipc-accept".to_string())
            .spawn(move || loop {
                match server.accept() {
                    Ok(stream) => spawn_ipc_client(
                        stream,
                        request_tx.clone(),
                        Arc::clone(&active_clients),
                    ),
                    Err(error) => {
                        eprintln!("IPC named-pipe accept failed: {error}");
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            })?;
        Ok((
            request_rx,
            balancify_service::ipc::server::WINDOWS_SERVICE_PIPE_NAME.to_string(),
        ))
    }

    #[cfg(not(windows))]
    start_loopback_tcp_listener(request_tx, request_rx, active_clients)
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
    fn pending_driver_receive_queue_preserves_fifo_and_fails_closed_at_capacity() {
        let mut queue = VecDeque::new();
        queue_pending_driver_rx(&mut queue, vec![1]).unwrap();
        queue_pending_driver_rx(&mut queue, vec![2]).unwrap();
        assert_eq!(queue.pop_front(), Some(vec![1]));
        assert_eq!(queue.pop_front(), Some(vec![2]));

        for _ in 0..MAX_PENDING_DRIVER_RX {
            queue_pending_driver_rx(&mut queue, vec![3]).unwrap();
        }
        let error = queue_pending_driver_rx(&mut queue, vec![4]).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(queue.len(), MAX_PENDING_DRIVER_RX);
        assert!(queue.iter().all(|packet| packet == &[3]));
    }

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
    fn relay_addresses_reject_malformed_configuration() {
        let (primary, secondary) = parse_relay_addresses(None, None).unwrap();
        assert_eq!(primary, "127.0.0.1:4430".parse().unwrap());
        assert_eq!(secondary, None);
        assert_eq!(
            parse_relay_addresses(Some("not-an-address"), None).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert_eq!(
            parse_relay_addresses(Some("127.0.0.1:4430"), Some("bad"))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert!(parse_relay_addresses(Some("127.0.0.1:0"), None).is_err());
        assert!(parse_relay_addresses(Some("0.0.0.0:4430"), None).is_err());
        assert!(parse_relay_addresses(None, Some("[::]:4431")).is_err());
        assert!(parse_relay_addresses(Some("127.0.0.1:4430"), Some("127.0.0.1:4430")).is_err());
    }

    #[test]
    fn failover_uses_the_identity_configured_for_the_selected_endpoint() {
        let primary: SocketAddr = "192.0.2.1:4430".parse().unwrap();
        let secondary: SocketAddr = "192.0.2.2:4430".parse().unwrap();
        let unknown: SocketAddr = "192.0.2.3:4430".parse().unwrap();
        assert_eq!(
            relay_identity_for_address(primary, primary, Some([1; 32]), Some(secondary), Some([2; 32])),
            Some([1; 32]),
        );
        assert_eq!(
            relay_identity_for_address(secondary, primary, Some([1; 32]), Some(secondary), Some([2; 32])),
            Some([2; 32]),
        );
        assert_eq!(
            relay_identity_for_address(unknown, primary, Some([1; 32]), Some(secondary), Some([2; 32])),
            None,
        );
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
    fn synthetic_app_traffic_is_confined_to_explicit_insecure_lab_mode() {
        assert!(validate_simulated_traffic(ConnectionMode::Secure, false).is_ok());
        assert_eq!(
            validate_simulated_traffic(ConnectionMode::Secure, true).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert!(validate_simulated_traffic(ConnectionMode::InsecureLab, true).is_ok());
    }

    #[test]
    fn synthetic_udp_packet_is_valid_raw_ipv4_for_service_ingress() {
        let packet = build_simulated_ipv4_udp_packet(7, 5050, b"lab payload").unwrap();
        assert_eq!(packet.len(), 39);
        assert_eq!(ipv4_header_checksum(&packet[..20]), 0);
        assert_eq!(&packet[24..26], &19u16.to_be_bytes());
        assert_eq!(&packet[28..], b"lab payload");
        let flow = balancify_service::packet::flow_key_from_packet(&packet).unwrap();
        assert_eq!(flow.proto, 17);
        assert_eq!(flow.src.to_string(), "10.0.0.1:5050");
        assert_eq!(flow.dst.to_string(), "8.8.8.8:443");
        assert!(build_simulated_ipv4_udp_packet(0, 5000, &vec![0; 9000]).is_err());
    }

    #[test]
    fn simulated_packet_scheduler_uses_elapsed_service_time() {
        let started_at = Instant::now();
        assert_eq!(
            next_simulated_packet_tick(started_at, started_at + Duration::from_millis(999), 0),
            None
        );
        assert_eq!(
            next_simulated_packet_tick(started_at, started_at + Duration::from_secs(1), 0),
            Some(1)
        );
        assert_eq!(
            next_simulated_packet_tick(started_at, started_at + Duration::from_millis(1500), 1),
            None
        );
        assert_eq!(
            next_simulated_packet_tick(started_at, started_at + Duration::from_secs(3), 1),
            Some(3)
        );
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
