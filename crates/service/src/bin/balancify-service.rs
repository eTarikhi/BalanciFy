//! Balancify service binary — Windows service core with UDP I/O adapters.
//! Blueprint §4.2, §11.1 Phase 4 MVP.
//!
//! This binary demonstrates the sans-I/O service core wired to real UDP sockets
//! (one per uplink), with secure handshake, pacing, congestion control, rekey,
//! and dual-relay failover. In production, the packet source would be the
//! NDIS miniport; here we use a simple echo of stdin or a TUN-like loop.

use balancify_core::{UplinkId, FlowKey};
use balancify_crypto::{FileKeyStore, KeyStore, PinnedKeys, TunnelCipher};
use balancify_service::{Service, ServiceConfig, Action};
use std::collections::BTreeMap;
use std::net::{UdpSocket, SocketAddr};
use std::time::{Duration, Instant};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let relay_primary: SocketAddr = args.get(1).and_then(|s| s.parse().ok()).unwrap_or_else(|| "127.0.0.1:4430".parse().unwrap());
    let relay_secondary: Option<SocketAddr> = args.get(2).and_then(|s| s.parse().ok());

    let state_dir = std::env::var("BALANCIFY_STATE_DIR").unwrap_or_else(|_| {
        #[cfg(windows)]
        {
            std::env::var("ProgramData").map(|pd| format!("{}\\Balancify", pd)).unwrap_or_else(|_| "/tmp/balancify_service".to_string())
        }
        #[cfg(not(windows))]
        {
            "/tmp/balancify_service".to_string()
        }
    });
    let secret_path = std::path::Path::new(&state_dir).join("client_secret.key");
    let pinned_path = std::path::Path::new(&state_dir).join("pinned_relays.keys");
    let mut store = FileKeyStore::new(&secret_path, &pinned_path);
    let keypair = store.load_or_generate().unwrap_or_else(|_| balancify_crypto::StaticKeypair::generate());
    let pinned_relays = store.load_pinned().unwrap_or_else(|_| PinnedKeys::new());

    println!("Balancify service — state_dir={}, primary={}, secondary={:?}", state_dir, relay_primary, relay_secondary);
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
    sock0.set_read_timeout(Some(Duration::from_millis(100)))?;
    sock1.set_read_timeout(Some(Duration::from_millis(100)))?;
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

    // Handshake
    let relay_public = std::env::var("BALANCIFY_RELAY_PUBLIC_KEY").ok().and_then(|hex| {
        let bytes = hex_decode(&hex)?;
        if bytes.len() == 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            Some(arr)
        } else {
            None
        }
    });
    if let Some(rp) = relay_public {
        println!("  initiating handshake to primary...");
        let prologue = b"balancify handshake v1";
        match svc.initiate_handshake(rp, prologue) {
            Ok(msg1) => {
                let _ = sock0.send_to(&msg1, relay_primary);
                println!("  sent msg1 {} bytes", msg1.len());
            }
            Err(e) => eprintln!("  handshake init failed: {:?}", e),
        }
    } else {
        println!("  no BALANCIFY_RELAY_PUBLIC_KEY — lab mode, opening session directly");
        svc.open_session(1, now);
    }

    let mut dg = [0u8; 65535];
    let mut flow_counter = 0u64;
    let mut last_tick = Instant::now();
    let mut last_stats = Instant::now();

    loop {
        let now = Instant::now();
        // Tick every 100ms
        if now.duration_since(last_tick) >= Duration::from_millis(100) {
            let actions = svc.tick(now);
            for act in actions {
                if let Action::Send { uplink, bytes } = act {
                    let sock = if uplink.0 == 0 { &sock0 } else { &sock1 };
                    let relay = svc.relay_current().unwrap_or(relay_primary);
                    let _ = sock.send_to(&bytes, relay);
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
                                        let _ = s.send_to(&bytes, relay);
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
                    let _ = sock.send_to(&bytes, relay);
                }
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 { return None; }
    let mut out = Vec::with_capacity(s.len()/2);
    for i in (0..s.len()).step_by(2) {
        let byte = u8::from_str_radix(&s[i..i+2], 16).ok()?;
        out.push(byte);
    }
    Some(out)
}
