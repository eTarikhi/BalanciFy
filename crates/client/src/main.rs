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
//! path end-to-end without requiring the driver.

use balancify_core::{UplinkId, FlowKey};
use balancify_crypto::{FileKeyStore, KeyStore, PinnedKeys, TunnelCipher};
use balancify_service::{Service, ServiceConfig};
use std::net::{UdpSocket, SocketAddr};
use std::time::{Duration, Instant};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let relay_addr: SocketAddr = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| "127.0.0.1:4430".parse().unwrap());
    let count: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);

    // Load or generate client static keypair
    let state_dir = std::env::var("BALANCIFY_STATE_DIR").unwrap_or_else(|_| {
        #[cfg(windows)]
        {
            std::env::var("ProgramData")
                .map(|pd| format!("{}\\Balancify", pd))
                .unwrap_or_else(|_| "/tmp/balancify_client".to_string())
        }
        #[cfg(not(windows))]
        {
            "/tmp/balancify_client".to_string()
        }
    });
    let secret_path = std::path::Path::new(&state_dir).join("client_secret.key");
    let pinned_path = std::path::Path::new(&state_dir).join("pinned_relays.keys");
    let mut store = FileKeyStore::new(&secret_path, &pinned_path);
    let keypair = store.load_or_generate().unwrap_or_else(|_| {
        eprintln!("Failed to load keypair, generating ephemeral");
        balancify_crypto::StaticKeypair::generate()
    });
    let pinned_relays = store.load_pinned().unwrap_or_else(|_| PinnedKeys::new());

    // For demo, if no pinned relay, we need relay's public key. In real use,
    // the relay public key would be provisioned out-of-band. Here we try to
    // load from BALANCIFY_RELAY_PUBLIC_KEY env (hex) or fail.
    let relay_public = if pinned_relays.is_empty() {
        if let Ok(hex) = std::env::var("BALANCIFY_RELAY_PUBLIC_KEY") {
            let bytes = hex::decode(&hex).unwrap_or_else(|_| vec![0u8; 32]);
            if bytes.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                arr
            } else {
                eprintln!("BALANCIFY_RELAY_PUBLIC_KEY must be 32-byte hex");
                eprintln!("No pinned relay, will try unauthenticated (lab) handshake");
                [0u8; 32]
            }
        } else {
            eprintln!("No pinned relay and no BALANCIFY_RELAY_PUBLIC_KEY -- using lab mode (no pinning)");
            [0u8; 32]
        }
    } else {
        // Use first pinned
        let mut arr = [0u8; 32];
        // PinnedKeys doesn't expose iteration easily, so we use env var still
        if let Ok(hex) = std::env::var("BALANCIFY_RELAY_PUBLIC_KEY") {
            if let Ok(bytes) = hex::decode(&hex) {
                if bytes.len() == 32 {
                    arr.copy_from_slice(&bytes);
                }
            }
        }
        arr
    };

    println!("Balancify client -- state_dir={}, relay={}, count={}", state_dir, relay_addr, count);
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

    // Initiate handshake if relay public known, otherwise skip to lab mode
    let use_secure = relay_public != [0u8; 32];
    if use_secure {
        println!("  initiating secure handshake (Noise_IK)...");
        let prologue = b"balancify handshake v1";
        let msg1_framed = svc
            .initiate_handshake(relay_public, prologue)
            .expect("handshake init");
        sock.send_to(&msg1_framed, relay_addr)?;
        println!("  sent handshake msg1 ({} bytes)", msg1_framed.len());

        let mut buf = [0u8; 65535];
        match sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                println!("  received {} bytes from {}", n, from);
                if n >= 2 && buf[0] == 0xFF && buf[1] == 0x02 {
                    let now2 = Instant::now();
                    svc.handle_handshake_msg2(&buf[..n], now2)
                        .expect("handle msg2");
                    println!("  handshake complete, transport keys installed");
                } else {
                    eprintln!("  unexpected handshake response: first bytes {:02x?}", &buf[..n.min(16)]);
                }
            }
            Err(e) => {
                eprintln!("  handshake response timeout: {}", e);
                eprintln!("  continuing in lab mode (null cipher)");
            }
        }
    } else {
        println!("  lab mode: no secure handshake, using null cipher");
        svc.open_session(1, now);
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

// Minimal hex decode without external crate for env var parsing
mod hex {
    pub fn decode(s: &str) -> Result<Vec<u8>, ()> {
        let s = s.trim();
        if s.len() % 2 != 0 {
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
