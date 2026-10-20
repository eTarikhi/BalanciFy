//! Balancify client binary -- thin CLI that performs secure handshake and
//! demonstrates encrypted tunnel flow. Blueprint Sec.4.2, Sec.8.3, Sec.11.1 Phase 4.
//!
//! This binary is intentionally simple: it loads or generates a static keypair
//! via FileKeyStore (DPAPI-protected on Windows), initiates Noise_IK handshake
//! to a relay, and then sends a configurable number of app packets through the
//! bonded tunnel, printing counters and status.
//!
//! Real Windows service integration would bind the virtual miniport and forward
//! IP packets via this service core; this CLI validates the handshake + tunnel
//! path end-to-end without requiring the driver. It fails closed unless a relay
//! identity is configured; `--insecure-lab` is debug-build-only and loopback-only.

use balancify_core::{UplinkId, FlowKey};
use balancify_crypto::{FileKeyStore, KeyStore, PinnedKeys, TunnelCipher};
use balancify_service::{Service, ServiceConfig};
use std::net::{UdpSocket, SocketAddr};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionMode {
    Secure,
    InsecureLab,
}

fn select_connection_mode(
    has_relay_key: bool,
    insecure_lab_requested: bool,
    debug_build: bool,
) -> Result<ConnectionMode, std::io::Error> {
    if has_relay_key {
        return Ok(ConnectionMode::Secure);
    }
    if !insecure_lab_requested {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "no relay identity configured; refusing insecure transport (use --insecure-lab for loopback-only debug runs)",
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

fn parse_relay_public_key(encoded: &str) -> Result<[u8; 32], std::io::Error> {
    let bytes = hex::decode(encoded).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "relay public key must be valid hex")
    })?;
    let public: [u8; 32] = bytes.try_into().map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "relay public key must be exactly 32 bytes")
    })?;
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
) -> Result<Option<[u8; 32]>, std::io::Error> {
    match configured {
        Some(public) => {
            if public == [0; 32] {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "all-zero relay public key is not a valid identity",
                ));
            }
            if !pinned.is_empty() && !pinned.is_pinned(&public) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "configured relay key does not match any pinned identity",
                ));
            }
            Ok(Some(public))
        }
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

fn choose_state_dir(
    override_dir: Option<std::path::PathBuf>,
    windows_local_app_data: Option<std::path::PathBuf>,
    xdg_state_home: Option<std::path::PathBuf>,
    home: Option<std::path::PathBuf>,
    is_windows: bool,
) -> Result<std::path::PathBuf, std::io::Error> {
    if let Some(path) = override_dir {
        if !path.is_absolute() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "BALANCIFY_STATE_DIR must be an absolute path",
            ));
        }
        return Ok(path);
    }
    if is_windows {
        if let Some(path) = windows_local_app_data.filter(|path| path.is_absolute()) {
            return Ok(path.join("Balancify"));
        }
        return home
            .filter(|path| path.is_absolute())
            .map(|path| path.join("AppData").join("Local").join("Balancify"))
            .ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "absolute LOCALAPPDATA or USERPROFILE is required",
            ));
    }
    if let Some(path) = xdg_state_home.filter(|path| path.is_absolute()) {
        return Ok(path.join("balancify"));
    }
    home.filter(|path| path.is_absolute())
        .map(|path| path.join(".local").join("state").join("balancify"))
        .ok_or_else(|| std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "absolute XDG_STATE_HOME or HOME is required",
        ))
}

fn default_state_dir() -> Result<std::path::PathBuf, std::io::Error> {
    let override_dir = std::env::var_os("BALANCIFY_STATE_DIR").map(std::path::PathBuf::from);
    #[cfg(windows)]
    let windows_local_app_data = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
    #[cfg(not(windows))]
    let windows_local_app_data = None;
    #[cfg(not(windows))]
    let xdg_state_home = std::env::var_os("XDG_STATE_HOME").map(std::path::PathBuf::from);
    #[cfg(windows)]
    let xdg_state_home = None;
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from);
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    choose_state_dir(override_dir, windows_local_app_data, xdg_state_home, home, cfg!(windows))
}

fn validate_lab_target(mode: ConnectionMode, relay_addr: SocketAddr) -> Result<(), std::io::Error> {
    if mode == ConnectionMode::InsecureLab && !relay_addr.ip().is_loopback() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "insecure lab mode is restricted to loopback relay addresses",
        ));
    }
    Ok(())
}

fn main() -> std::io::Result<()> {
    let mut positional = Vec::new();
    let mut insecure_lab_requested = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--insecure-lab" => insecure_lab_requested = true,
            "--help" | "-h" => {
                println!("Usage: balancify-client [--insecure-lab] [RELAY_ADDR] [PACKET_COUNT]");
                println!("Without a configured relay identity, the client fails closed. Insecure lab mode is debug-build-only and loopback-only.");
                return Ok(());
            }
            _ if arg.starts_with('-') => {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("unknown option: {arg}")));
            }
            _ => positional.push(arg),
        }
    }
    if positional.len() > 2 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "expected at most RELAY_ADDR and PACKET_COUNT"));
    }
    let relay_addr: SocketAddr = positional
        .first()
        .map(|s| s.parse())
        .transpose()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid relay address"))?
        .unwrap_or_else(|| "127.0.0.1:4430".parse().unwrap());
    let count: usize = positional
        .get(1)
        .map(|s| s.parse())
        .transpose()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "packet count must be a non-negative integer"))?
        .unwrap_or(10);

    // Load or generate client static keypair
    let state_dir = default_state_dir()?;
    let secret_path = state_dir.join("client_secret.key");
    let pinned_path = state_dir.join("pinned_relays.keys");
    let mut store = FileKeyStore::new(&secret_path, &pinned_path);
    let keypair = store.load_or_generate().map_err(|e| {
        std::io::Error::other(format!("failed to load or persist client identity: {e}"))
    })?;
    let pinned_relays = store.load_pinned().map_err(|e| {
        std::io::Error::other(format!("failed to load pinned relay identities: {e}"))
    })?;

    let configured_relay = match std::env::var("BALANCIFY_RELAY_PUBLIC_KEY") {
        Ok(encoded) => Some(parse_relay_public_key(&encoded)?),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(std::io::Error::other(format!("invalid relay identity setting: {error}"))),
    };
    let relay_public = resolve_relay_public(configured_relay, &pinned_relays)?;
    let mode = select_connection_mode(relay_public.is_some(), insecure_lab_requested, cfg!(debug_assertions))?;
    validate_lab_target(mode, relay_addr)?;
    let relay_public = relay_public.unwrap_or([0; 32]);

    println!("Balancify client -- state_dir={}, relay={}, count={}", state_dir.display(), relay_addr, count);
    println!("  client public: {:02x?}...", &keypair.public_bytes()[..8]);

    // Create service core with dummy TunnelCipher, will be replaced after handshake
    let cfg = ServiceConfig {
        session: 1,
        seed: 0xC11E,
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

    // Start with dummy TunnelCipher (zero key), will be replaced after handshake
    let dummy_key = [0u8; 32];
    let dummy_salt = [0u8; 3];
    let dummy_cipher = TunnelCipher::new(&dummy_key, dummy_salt).unwrap();
    let mut svc = Service::new(cfg, dummy_cipher.clone(), dummy_cipher.clone());
    svc.set_static_keypair(keypair.clone());
    if !pinned_relays.is_empty() {
        svc.set_pinned_relays(pinned_relays);
    }
    let now = Instant::now();
    svc.add_uplink(UplinkId(0), 100_000_000, false, 1500, now);
    svc.add_uplink(UplinkId(1), 50_000_000, false, 1500, now);
    svc.publish(now);

    let sock = UdpSocket::bind("0.0.0.0:0")?;
    sock.set_read_timeout(Some(Duration::from_secs(2)))?;
    let local_addr = sock.local_addr()?;
    println!("  local socket: {}", local_addr);

    match mode {
        ConnectionMode::Secure => {
            println!("  initiating authenticated Noise_IK handshake...");
            let prologue = b"balancify handshake v1";
            let msg1_framed = svc
                .initiate_handshake(relay_public, prologue)
                .map_err(|e| std::io::Error::other(format!("handshake initialization failed: {e:?}")))?;
            sock.send_to(&msg1_framed, relay_addr)?;
            println!("  sent handshake msg1 ({} bytes)", msg1_framed.len());

            let mut buf = [0u8; 65535];
            let (n, from) = sock.recv_from(&mut buf)?;
            if from != relay_addr {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("handshake response came from unexpected peer {from}"),
                ));
            }
            if n < 2 || buf[0] != 0xFF || buf[1] != 0x02 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("unexpected handshake response framing: {:02x?}", &buf[..n.min(16)]),
                ));
            }
            svc.handle_handshake_msg2(&buf[..n], Instant::now())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, format!("relay authentication failed: {e:?}")))?;
            println!("  authenticated handshake complete; transport keys installed");
        }
        ConnectionMode::InsecureLab => {
            println!("  WARNING: explicitly enabled INSECURE lab mode; loopback only, zero-key test cipher");
            svc.open_session(1, now);
        }
    }

    // Optional per-flow sessions (Sec.4.7.4)
    if std::env::var("BALANCIFY_PER_FLOW").map(|v| v == "1").unwrap_or(false) {
        svc.set_per_flow_sessions(true);
        println!("  per-flow sessions enabled");
    }

    // Send app packets through bonded tunnel
    let src: SocketAddr = "10.0.0.1:12345".parse().unwrap();
    let dst: SocketAddr = "8.8.8.8:80".parse().unwrap();
    let flow = FlowKey::new(6, src, dst);
    let mut delivered = 0;
    for i in 0..count {
        let now = Instant::now();
        let payload = format!("hello balancify packet {}", i);
        let actions = svc.send_app_packet(&flow, None, payload.as_bytes(), now);
        for act in actions {
            match act {
                balancify_service::Action::Send { uplink, bytes } => {
                    let _ = sock.send_to(&bytes, relay_addr);
                    println!("  [{}] sent {} bytes via uplink {}", i, bytes.len(), uplink.0);
                }
                balancify_service::Action::Hold { .. } => {
                    println!("  [{}] held (congestion)", i);
                }
                balancify_service::Action::Refused { .. } => {
                    println!("  [{}] refused (no eligible uplink)", i);
                }
                _ => {}
            }
        }
        // Try to receive replies (ACKs, data)
        let mut buf = [0u8; 65535];
        loop {
            match sock.recv_from(&mut buf) {
                Ok((n, _)) => {
                    let now = Instant::now();
                    // Try handshake or tunnel
                    if n >= 2 && buf[0] == 0xFF && buf[1] == 0x02 {
                        let _ = svc.handle_handshake_msg2(&buf[..n], now);
                    } else {
                        let actions = svc.ingest_path(UplinkId(0), &buf[..n], now);
                        for a in actions {
                            if let balancify_service::Action::Deliver { data } = a {
                                println!("  [{}] delivered {} bytes: {:?}", i, data.len(), String::from_utf8_lossy(&data));
                                delivered += 1;
                            }
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => break,
                Err(e) => {
                    eprintln!("  recv error: {}", e);
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
        let _ = svc.tick(Instant::now());
    }

    println!("Done: sent {}, delivered {}", count, delivered);
    let c = svc.counters();
    println!("Counters: stripped={}, forwarded={}, dups={}, reinj={}, delivered={}, ooo={}, refused={}, auth_fail={}, ecn_ce={}, delay={}",
        c.packets_stripped, c.packets_forwarded, c.duplicates_sent, c.reinjections, c.delivered, c.out_of_order, c.refused, c.auth_failed, c.ecn_ce, c.delay_events);
    println!("Congestion: loss_events={}, ecn_events={}, delay_events={}, paths={:?}", svc.cc.loss_events(), svc.cc.ecn_events(), svc.cc.delay_events(), svc.cc.paths());
    let bundle = balancify_service::support::SupportBundle::collect(&svc, Instant::now());
    println!("Support bundle: {}", bundle.to_json());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_identity_selects_secure_mode_even_when_lab_flag_was_set() {
        assert_eq!(
            select_connection_mode(true, true, true).unwrap(),
            ConnectionMode::Secure
        );
    }

    #[test]
    fn missing_identity_fails_closed_by_default() {
        let error = select_connection_mode(false, false, true).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn insecure_lab_requires_explicit_debug_opt_in() {
        assert_eq!(
            select_connection_mode(false, true, true).unwrap(),
            ConnectionMode::InsecureLab
        );
        assert_eq!(
            select_connection_mode(false, true, false).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn insecure_lab_is_loopback_only() {
        assert!(validate_lab_target(
            ConnectionMode::InsecureLab,
            "127.0.0.1:4430".parse().unwrap()
        ).is_ok());
        assert_eq!(
            validate_lab_target(ConnectionMode::InsecureLab, "192.0.2.1:4430".parse().unwrap())
                .unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert!(validate_lab_target(
            ConnectionMode::Secure,
            "192.0.2.1:4430".parse().unwrap()
        ).is_ok());
    }

    #[test]
    fn configured_relay_key_must_match_existing_pins() {
        let pinned_key = [0x11; 32];
        let mut pins = PinnedKeys::new();
        pins.pin(pinned_key);
        assert_eq!(resolve_relay_public(None, &pins).unwrap(), Some(pinned_key));
        assert_eq!(resolve_relay_public(Some(pinned_key), &pins).unwrap(), Some(pinned_key));
        assert_eq!(
            resolve_relay_public(Some([0x22; 32]), &pins).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            resolve_relay_public(Some([0; 32]), &PinnedKeys::new()).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert_eq!(
            resolve_relay_public(None, &PinnedKeys::from_iter([[0; 32]])).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn state_dir_override_has_highest_precedence() {
        let override_path = std::env::temp_dir().join("custom").join("state");
        let chosen = choose_state_dir(
            Some(override_path.clone()),
            Some(std::env::temp_dir().join("local")),
            Some(std::env::temp_dir().join("xdg")),
            Some(std::env::temp_dir().join("home")),
            false,
        ).unwrap();
        assert_eq!(chosen, override_path);
    }

    #[test]
    fn windows_state_prefers_per_user_local_app_data() {
        let local_app_data = std::env::temp_dir().join("local-app-data");
        let chosen = choose_state_dir(
            None,
            Some(local_app_data.clone()),
            None,
            Some(std::env::temp_dir().join("user-profile")),
            true,
        ).unwrap();
        assert_eq!(chosen, local_app_data.join("Balancify"));
    }

    #[test]
    fn windows_state_falls_back_to_user_profile_not_program_data() {
        let user_profile = std::env::temp_dir().join("user-profile");
        let chosen = choose_state_dir(
            None,
            None,
            None,
            Some(user_profile.clone()),
            true,
        ).unwrap();
        assert_eq!(chosen, user_profile.join("AppData").join("Local").join("Balancify"));
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_state_prefers_absolute_xdg_then_home_state_dir() {
        assert_eq!(
            choose_state_dir(None, None, Some("/run/user/1000/state".into()), Some("/home/test".into()), false).unwrap(),
            std::path::PathBuf::from("/run/user/1000/state/balancify")
        );
        assert_eq!(
            choose_state_dir(None, None, Some("relative".into()), Some("/home/test".into()), false).unwrap(),
            std::path::PathBuf::from("/home/test/.local/state/balancify")
        );
    }

    #[test]
    fn state_dir_rejects_empty_override_and_missing_home() {
        assert_eq!(
            choose_state_dir(Some(std::path::PathBuf::new()), None, None, None, false).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert_eq!(
            choose_state_dir(Some("relative".into()), None, None, None, false).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert_eq!(
            choose_state_dir(None, Some("relative".into()), None, Some("relative".into()), true).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        assert_eq!(
            choose_state_dir(None, None, None, Some("relative".into()), false).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        assert_eq!(
            choose_state_dir(None, None, None, None, false).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
    }

    #[test]
    fn relay_key_parser_rejects_malformed_or_zero_identity() {
        assert!(parse_relay_public_key(&"01".repeat(32)).is_ok());
        assert!(parse_relay_public_key("abcdxyz").is_err());
        assert!(parse_relay_public_key(&"00".repeat(32)).is_err());
        assert!(parse_relay_public_key("é" ).is_err());
    }
}

// Minimal hex decode without external crate for env var parsing
mod hex {
    pub fn decode(s: &str) -> Result<Vec<u8>, ()> {
        let s = s.trim();
        if !s.is_ascii() || s.len() % 2 != 0 {
            return Err(());
        }
        let mut out = Vec::with_capacity(s.len() / 2);
        for i in (0..s.len()).step_by(2) {
            let byte = u8::from_str_radix(&s[i..i+2], 16).map_err(|_| ())?;
            out.push(byte);
        }
        Ok(out)
    }
}
