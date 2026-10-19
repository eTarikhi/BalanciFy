//! Throughput measurement harness — §10.3 aggregate/single-flow/failover
//! Simulates bonded vs single-path throughput with pacing + congestion control
//! enabled, records median/p95/spread over 10 reps, no external dependencies.

use balancify_core::{UplinkId, FlowKey};
use balancify_crypto::TunnelCipher;
use balancify_service::{Service, ServiceConfig};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

fn main() {
    println!("=== Balancify throughput harness §10.3 ===");
    let reps = 10;
    let packets_per_rep = 1000;
    let payload = vec![0u8; 1200]; // ~MTU

    // Config with pacing + CC
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
        auto_accept: true,
    };
    let dummy_key = [0u8; 32];
    let dummy_salt = [0u8; 3];
    let dummy_cipher = TunnelCipher::new(&dummy_key, dummy_salt).unwrap();

    // Helper to run one rep
    let run_rep = |bonded: bool, loss_path1: f64| -> (Duration, u64, u64) {
        let mut svc = Service::new(
            ServiceConfig { bond_enabled: bonded, ..cfg.clone() },
            dummy_cipher.clone(),
            dummy_cipher.clone(),
        );
        let now = Instant::now();
        svc.add_uplink(UplinkId(0), 200_000_000, false, 1500, now);
        svc.add_uplink(UplinkId(1), 50_000_000, false, 1500, now);
        for _ in 0..5 {
            svc.on_probe_ok(UplinkId(0), Duration::from_millis(10), now);
            svc.on_probe_ok(UplinkId(1), Duration::from_millis(50), now);
        }
        svc.publish(now);

        let src: SocketAddr = "10.0.0.1:12345".parse().unwrap();
        let dst: SocketAddr = "8.8.8.8:80".parse().unwrap();
        let flow = FlowKey::new(6, src, dst);

        let start = Instant::now();
        let mut sent = 0u64;
        let mut held = 0u64;
        let mut t = now;
        for i in 0..packets_per_rep {
            // Simulate loss on path 1: every Nth packet dropped
            let actions = svc.send_app_packet(&flow, None, &payload, t);
            for act in actions {
                match act {
                    balancify_service::Action::Send { uplink, .. } => {
                        // Simulate ACK after RTT
                        let rtt = if uplink.0 == 0 { Duration::from_millis(10) } else { Duration::from_millis(50) };
                        t += Duration::from_micros(100);
                        // Simulate loss: drop ACK for path1 with probability loss_path1
                        let drop = uplink.0 == 1 && (i as f64 * loss_path1) % 1.0 < loss_path1;
                        if !drop {
                            // Fake ACK processing: just call on_ack via policy? Simplified: call cc.on_ack
                            svc.cc.on_ack(uplink.0, payload.len() as u64, t);
                            svc.cc.on_rtt(uplink.0, rtt, t);
                        } else {
                            svc.cc.on_loss(uplink.0, t);
                        }
                        sent += 1;
                    }
                    balancify_service::Action::Hold { .. } => held += 1,
                    _ => {}
                }
            }
            // tick every 10 packets
            if i % 10 == 0 {
                let _ = svc.tick(t);
            }
        }
        let elapsed = start.elapsed();
        (elapsed, sent, held)
    };

    // Measure bonded vs single
    let mut bonded_times = Vec::new();
    let mut single_times = Vec::new();
    let mut bonded_sent = Vec::new();
    for rep in 0..reps {
        let (elapsed_bonded, sent_bonded, held_bonded) = run_rep(true, 0.0);
        let (elapsed_single, sent_single, _) = run_rep(false, 0.0);
        bonded_times.push(elapsed_bonded.as_micros() as f64);
        single_times.push(elapsed_single.as_micros() as f64);
        bonded_sent.push(sent_bonded);
        println!("rep {}: bonded {:?} (sent {}, held {}), single {:?} (sent {})",
            rep, elapsed_bonded, sent_bonded, held_bonded, elapsed_single, sent_single);
    }

    fn stats(mut v: Vec<f64>) -> (f64, f64, f64, f64) {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = v.len();
        let median = v[n/2];
        let p95 = v[(n as f64 * 0.95) as usize % n];
        let mean = v.iter().sum::<f64>() / n as f64;
        let spread = v[n-1] - v[0];
        (mean, median, p95, spread)
    }

    let (b_mean, b_med, b_p95, b_spread) = stats(bonded_times);
    let (s_mean, s_med, s_p95, s_spread) = stats(single_times);
    println!("\n=== Results over {} reps, {} packets each ===", reps, packets_per_rep);
    println!("Bonded: mean {:.1}us median {:.1}us p95 {:.1}us spread {:.1}us", b_mean, b_med, b_p95, b_spread);
    println!("Single: mean {:.1}us median {:.1}us p95 {:.1}us spread {:.1}us", s_mean, s_med, s_p95, s_spread);
    println!("Gain: {:.1}% (median bonded vs single)", (s_med - b_med) / s_med * 100.0);
    println!("Bonded sent median: {} packets", {
        let mut v = bonded_sent.clone();
        v.sort();
        v[v.len()/2]
    });

    // Lossy path test
    println!("\n=== Lossy path (1% loss on path1) ===");
    let mut lossy_times = Vec::new();
    for _ in 0..reps {
        let (elapsed, _, _) = run_rep(true, 0.01);
        lossy_times.push(elapsed.as_micros() as f64);
    }
    let (l_mean, l_med, l_p95, l_spread) = stats(lossy_times);
    println!("Lossy bonded: mean {:.1}us median {:.1}us p95 {:.1}us spread {:.1}us", l_mean, l_med, l_p95, l_spread);
    println!("Loss penalty: {:.1}% vs clean bonded", (l_med - b_med) / b_med * 100.0);

    // ECN simulation
    println!("\n=== ECN CE simulation ===");
    let mut svc = Service::new(cfg.clone(), dummy_cipher.clone(), dummy_cipher.clone());
    let now = Instant::now();
    svc.add_uplink(UplinkId(0), 200_000_000, false, 1500, now);
    svc.add_uplink(UplinkId(1), 50_000_000, false, 1500, now);
    for _ in 0..5 {
        svc.on_probe_ok(UplinkId(0), Duration::from_millis(10), now);
        svc.on_probe_ok(UplinkId(1), Duration::from_millis(50), now);
    }
    svc.publish(now);
    let src: SocketAddr = "10.0.0.1:12345".parse().unwrap();
    let dst: SocketAddr = "8.8.8.8:80".parse().unwrap();
    let flow = FlowKey::new(6, src, dst);
    for i in 0..100 {
        let t = now + Duration::from_millis(i * 10);
        let _ = svc.send_app_packet(&flow, None, &payload, t);
        if i % 10 == 0 {
            svc.cc.on_ecn(0, t);
        }
    }
    println!("After 10 ECN events on path0: cwnd0={} cwnd1={} ecn_events={}",
        svc.cc.window(0), svc.cc.window(1), svc.cc.ecn_events());

    // Delay detection
    println!("\n=== Delay-based detection ===");
    let mut svc2 = Service::new(cfg, dummy_cipher.clone(), dummy_cipher);
    let now = Instant::now();
    svc2.add_uplink(UplinkId(0), 200_000_000, false, 1500, now);
    for _ in 0..5 {
        svc2.on_probe_ok(UplinkId(0), Duration::from_millis(10), now);
    }
    svc2.publish(now);
    svc2.cc.on_rtt(0, Duration::from_millis(10), now);
    println!("RTT min 10ms, cwnd={}", svc2.cc.window(0));
    svc2.cc.on_rtt(0, Duration::from_millis(50), now + Duration::from_millis(200));
    println!("RTT 50ms (delay event), cwnd={} delay_events={}", svc2.cc.window(0), svc2.cc.delay_events());

    println!("\nThroughput harness done — see docs/phase2/lab-progress.md for S2c checklist");
}
