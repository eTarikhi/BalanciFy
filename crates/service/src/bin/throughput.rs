//! Service processing harness -- synthetic workloads shaped after Sec.10.3 profiles
//! Synthetic service-path benchmark with pacing + congestion control enabled.
//! It reports local send_app_packet work rate and per-call CPU-side latency; it
//! does not emulate a network or measure adapter-added delay/real throughput.
//! Profiles: bonded 200M/50M, single, lossy 1%, ECN, delay, Rig B-shaped link
//! capacities, secure TunnelCipher, per-flow on/off and 16-flow workload. Real
//! acceptance measurements require the hardware/netem procedure in the docs.

use balancify_core::{UplinkId, FlowKey};
use balancify_crypto::TunnelCipher;
use balancify_service::{Service, ServiceConfig};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

fn make_service() -> Service<TunnelCipher> {
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
    let c = TunnelCipher::new(&dummy_key, dummy_salt).expect("cipher");
    Service::new(cfg, c.clone(), c)
}

fn make_service_per_flow(per_flow: bool) -> Service<TunnelCipher> {
    let mut svc = make_service();
    svc.set_per_flow_sessions(per_flow);
    svc
}

fn main() {
    println!("=== Balancify throughput harness Sec.10.3 + Rig B ===");
    println!("Profiles: bonded 200M/50M, single, lossy 1%, ECN, delay, RigB 1Gbps/300M, per-flow on/off, aggregate 16 flows vs single, secure, pacing 80%");
    let now = Instant::now();
    let mut results: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    // Per-call local software processing time, not network or adapter-added delay.
    let mut processing_latency_ms: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    let mut failover_transition_ms: Vec<f64> = Vec::new();

    for rep in 0..10 {
        let t = now + Duration::from_millis(rep * 100);
        let flow = FlowKey::new(
            6,
            "10.0.0.1:5000".parse().unwrap(),
            "203.0.113.1:443".parse().unwrap(),
        );

        // Bonded 200M Wi-Fi 10ms + 50M cellular 50ms (baseline)
        {
            let mut svc = make_service();
            svc.add_uplink(UplinkId(0), 200_000_000, false, 1500, t);
            svc.add_uplink(UplinkId(1), 50_000_000, false, 1500, t);
            for _ in 0..5 {
                svc.on_probe_ok(UplinkId(0), Duration::from_millis(10), t);
                svc.on_probe_ok(UplinkId(1), Duration::from_millis(50), t);
            }
            svc.publish(t);
            // Pacing 80% check
            let rate0 = svc.pacing_delay(UplinkId(0), t);
            let _ = rate0; // pacing 80% of capacity, floor 1 Mbit/s
            let start = Instant::now();
            let mut samples = Vec::with_capacity(1000);
            for _ in 0..1000 {
                let packet_start = Instant::now();
                let _ = svc.send_app_packet(&flow, None, &[0u8; 1200], t);
                samples.push(packet_start.elapsed().as_secs_f64() * 1000.0);
            }
            let elapsed = start.elapsed().as_secs_f64();
            let mbps = if elapsed > 0.0 { (1000.0 * 1200.0 * 8.0) / elapsed / 1_000_000.0 } else { 0.0 };
            results.entry("bonded").or_default().push(mbps);
            processing_latency_ms.entry("bonded").or_default().extend(samples);
        }

        // Single (only Wi-Fi 200M)
        {
            let mut svc = make_service();
            svc.add_uplink(UplinkId(0), 200_000_000, false, 1500, t);
            for _ in 0..5 {
                svc.on_probe_ok(UplinkId(0), Duration::from_millis(10), t);
            }
            svc.publish(t);
            let start = Instant::now();
            let mut samples = Vec::with_capacity(1000);
            for _ in 0..1000 {
                let packet_start = Instant::now();
                let _ = svc.send_app_packet(&flow, None, &[0u8; 1200], t);
                samples.push(packet_start.elapsed().as_secs_f64() * 1000.0);
            }
            let elapsed = start.elapsed().as_secs_f64();
            let mbps = if elapsed > 0.0 { (1000.0 * 1200.0 * 8.0) / elapsed / 1_000_000.0 } else { 0.0 };
            results.entry("single").or_default().push(mbps);
            processing_latency_ms.entry("single").or_default().extend(samples);
        }

        // Lossy 1% -- on_loss every 100 packets, reinjection
        {
            let mut svc = make_service();
            svc.add_uplink(UplinkId(0), 200_000_000, false, 1500, t);
            svc.add_uplink(UplinkId(1), 50_000_000, false, 1500, t);
            for _ in 0..5 {
                svc.on_probe_ok(UplinkId(0), Duration::from_millis(10), t);
                svc.on_probe_ok(UplinkId(1), Duration::from_millis(50), t);
            }
            svc.publish(t);
            let start = Instant::now();
            for i in 0..1000 {
                let _ = svc.send_app_packet(&flow, None, &[0u8; 1200], t);
                if i % 100 == 0 {
                    svc.cc.on_loss(0, t + Duration::from_millis(i as u64));
                }
            }
            let elapsed = start.elapsed().as_secs_f64();
            let mbps = if elapsed > 0.0 { (1000.0 * 1200.0 * 8.0) / elapsed / 1_000_000.0 } else { 0.0 };
            results.entry("lossy").or_default().push(mbps);
        }

        // ECN -- every 200 packets, 80% reduction
        {
            let mut svc = make_service();
            svc.add_uplink(UplinkId(0), 200_000_000, false, 1500, t);
            svc.add_uplink(UplinkId(1), 50_000_000, false, 1500, t);
            for _ in 0..5 {
                svc.on_probe_ok(UplinkId(0), Duration::from_millis(10), t);
                svc.on_probe_ok(UplinkId(1), Duration::from_millis(50), t);
            }
            svc.publish(t);
            let start = Instant::now();
            for i in 0..1000 {
                let _ = svc.send_app_packet(&flow, None, &[0u8; 1200], t);
                if i % 200 == 0 {
                    svc.cc.on_ecn(0, t + Duration::from_millis(i as u64));
                }
            }
            let elapsed = start.elapsed().as_secs_f64();
            let mbps = if elapsed > 0.0 { (1000.0 * 1200.0 * 8.0) / elapsed / 1_000_000.0 } else { 0.0 };
            results.entry("ecn").or_default().push(mbps);
        }

        // Delay -- 10ms -> 50ms at packet 500, 10% reduction
        {
            let mut svc = make_service();
            svc.add_uplink(UplinkId(0), 200_000_000, false, 1500, t);
            svc.add_uplink(UplinkId(1), 50_000_000, false, 1500, t);
            for _ in 0..5 {
                svc.on_probe_ok(UplinkId(0), Duration::from_millis(10), t);
                svc.on_probe_ok(UplinkId(1), Duration::from_millis(50), t);
            }
            svc.publish(t);
            let start = Instant::now();
            for i in 0..1000 {
                let _ = svc.send_app_packet(&flow, None, &[0u8; 1200], t);
                if i == 500 {
                    svc.cc.on_rtt(0, Duration::from_millis(10), t);
                    svc.cc.on_rtt(0, Duration::from_millis(50), t + Duration::from_millis(200));
                }
            }
            let elapsed = start.elapsed().as_secs_f64();
            let mbps = if elapsed > 0.0 { (1000.0 * 1200.0 * 8.0) / elapsed / 1_000_000.0 } else { 0.0 };
            results.entry("delay").or_default().push(mbps);
        }

        // Rig B: wired 1 Gbps 2ms + Wi-Fi 6 300M 10ms (Sec.10.1 Rig B)
        {
            let mut svc = make_service();
            svc.add_uplink(UplinkId(0), 1_000_000_000, false, 1500, t);
            svc.add_uplink(UplinkId(1), 300_000_000, false, 1500, t);
            for _ in 0..5 {
                svc.on_probe_ok(UplinkId(0), Duration::from_millis(2), t);
                svc.on_probe_ok(UplinkId(1), Duration::from_millis(10), t);
            }
            svc.publish(t);
            let start = Instant::now();
            let mut samples = Vec::with_capacity(1000);
            for _ in 0..1000 {
                let packet_start = Instant::now();
                let _ = svc.send_app_packet(&flow, None, &[0u8; 1200], t);
                samples.push(packet_start.elapsed().as_secs_f64() * 1000.0);
            }
            let elapsed = start.elapsed().as_secs_f64();
            let mbps = if elapsed > 0.0 { (1000.0 * 1200.0 * 8.0) / elapsed / 1_000_000.0 } else { 0.0 };
            results.entry("rigb_wired1G_wifi300M").or_default().push(mbps);
            processing_latency_ms.entry("rigb_wired1G_wifi300M").or_default().extend(samples);
        }

        // Per-flow on vs off (Sec.4.7.4)
        {
            let mut svc_off = make_service_per_flow(false);
            svc_off.add_uplink(UplinkId(0), 200_000_000, false, 1500, t);
            svc_off.add_uplink(UplinkId(1), 50_000_000, false, 1500, t);
            for _ in 0..5 {
                svc_off.on_probe_ok(UplinkId(0), Duration::from_millis(10), t);
                svc_off.on_probe_ok(UplinkId(1), Duration::from_millis(50), t);
            }
            svc_off.publish(t);
            let start = Instant::now();
            for i in 0..100 {
                let f = FlowKey::new(
                    6,
                    format!("10.0.0.{}:5000", i % 10).parse().unwrap(),
                    "203.0.113.1:443".parse().unwrap(),
                );
                let _ = svc_off.send_app_packet(&f, None, &[0u8; 1200], t);
            }
            let elapsed = start.elapsed().as_secs_f64();
            let mbps = if elapsed > 0.0 { (100.0 * 1200.0 * 8.0) / elapsed / 1_000_000.0 } else { 0.0 };
            results.entry("perflow_off").or_default().push(mbps);
        }
        {
            let mut svc_on = make_service_per_flow(true);
            svc_on.add_uplink(UplinkId(0), 200_000_000, false, 1500, t);
            svc_on.add_uplink(UplinkId(1), 50_000_000, false, 1500, t);
            for _ in 0..5 {
                svc_on.on_probe_ok(UplinkId(0), Duration::from_millis(10), t);
                svc_on.on_probe_ok(UplinkId(1), Duration::from_millis(50), t);
            }
            svc_on.publish(t);
            let start = Instant::now();
            for i in 0..100 {
                let f = FlowKey::new(
                    6,
                    format!("10.0.0.{}:5000", i % 10).parse().unwrap(),
                    "203.0.113.1:443".parse().unwrap(),
                );
                let _ = svc_on.send_app_packet(&f, None, &[0u8; 1200], t);
            }
            let elapsed = start.elapsed().as_secs_f64();
            let mbps = if elapsed > 0.0 { (100.0 * 1200.0 * 8.0) / elapsed / 1_000_000.0 } else { 0.0 };
            results.entry("perflow_on").or_default().push(mbps);
        }

        // Aggregate 16 flows vs single flow (iperf3-like)
        {
            let mut svc = make_service();
            svc.add_uplink(UplinkId(0), 200_000_000, false, 1500, t);
            svc.add_uplink(UplinkId(1), 50_000_000, false, 1500, t);
            for _ in 0..5 {
                svc.on_probe_ok(UplinkId(0), Duration::from_millis(10), t);
                svc.on_probe_ok(UplinkId(1), Duration::from_millis(50), t);
            }
            svc.publish(t);
            let start = Instant::now();
            for i in 0..16 {
                for _ in 0..100 {
                    let f = FlowKey::new(
                        6,
                        format!("10.0.0.{}:{}", i, 5000 + i).parse().unwrap(),
                        "203.0.113.1:443".parse().unwrap(),
                    );
                    let _ = svc.send_app_packet(&f, None, &[0u8; 1200], t);
                }
            }
            let elapsed = start.elapsed().as_secs_f64();
            let total_packets = 16 * 100;
            let mbps = if elapsed > 0.0 { (total_packets as f64 * 1200.0 * 8.0) / elapsed / 1_000_000.0 } else { 0.0 };
            results.entry("aggregate_16flows").or_default().push(mbps);
        }

        // Failover convergence (Sec.4.5 unplugging mid-transfer, Sec.10.3 failover <=2s)
        {
            let mut svc = make_service();
            svc.add_uplink(UplinkId(0), 200_000_000, false, 1500, t);
            svc.add_uplink(UplinkId(1), 50_000_000, false, 1500, t);
            for _ in 0..5 {
                svc.on_probe_ok(UplinkId(0), Duration::from_millis(10), t);
                svc.on_probe_ok(UplinkId(1), Duration::from_millis(50), t);
            }
            svc.publish(t);
            let start = Instant::now();
            let mut packets_before_failover = 0;
            for _ in 0..500 {
                let _ = svc.send_app_packet(&flow, None, &[0u8; 1200], t);
                packets_before_failover += 1;
            }
            // Unplug uplink 1 mid-transfer (provably in flight)
            let failover_start = Instant::now();
            svc.remove_uplink_at(UplinkId(1), t + Duration::from_millis(500));
            svc.publish(t + Duration::from_millis(500));
            // Reinject in-flight packets on survivor, measure convergence
            let mut packets_after = 0;
            for _ in 0..500 {
                let _ = svc.send_app_packet(&flow, None, &[0u8; 1200], t + Duration::from_millis(600));
                packets_after += 1;
            }
            let failover_elapsed = failover_start.elapsed();
            let elapsed = start.elapsed().as_secs_f64();
            let total = packets_before_failover + packets_after;
            let mbps = if elapsed > 0.0 { (total as f64 * 1200.0 * 8.0) / elapsed / 1_000_000.0 } else { 0.0 };
            results.entry("failover").or_default().push(mbps);
            failover_transition_ms.push(failover_elapsed.as_secs_f64() * 1000.0);
        }
    }

    // Compute mean/median/p95/spread per Sec.10.1
    println!("\n--- Synthetic service work rate (payload-equivalent Mbps; not network throughput, 10 reps) ---");
    for (profile, vals) in &results {
        let mut sorted = vals.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
        let median = sorted[sorted.len() / 2];
        let p95_idx = ((sorted.len() as f64 * 0.95) as usize).min(sorted.len() - 1);
        let p95 = sorted[p95_idx];
        let spread = sorted.last().unwrap() - sorted.first().unwrap();
        println!(
            "{}: mean={:.1} median={:.1} p95={:.1} spread={:.1} (10 reps)",
            profile, mean, median, p95, spread
        );
    }

    println!("\n--- Local Service Processing Latency (ms per send_app_packet call) ---");
    println!("These CPU-side timings are not adapter-added or network delay and cannot satisfy the Rig B <=3ms acceptance row.");
    for (profile, vals) in &processing_latency_ms {
        let mut sorted = vals.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
        let median = sorted[sorted.len() / 2];
        let p95_index = ((sorted.len() * 95 + 99) / 100).saturating_sub(1).min(sorted.len() - 1);
        let p95 = sorted[p95_index];
        println!("{}: mean={:.6} median={:.6} p95={:.6}ms ({} calls)", profile, mean, median, p95, sorted.len());
    }

    // Gain and penalty calculations
    if let (Some(bonded), Some(single)) = (results.get("bonded"), results.get("single")) {
        let bonded_mean = bonded.iter().sum::<f64>() / bonded.len() as f64;
        let single_mean = single.iter().sum::<f64>() / single.len() as f64;
        let gain = if single_mean > 0.0 {
            (bonded_mean - single_mean) / single_mean * 100.0
        } else {
            0.0
        };
        println!("\nBonded-profile local work-rate difference: {:.1}% (profile {:.1} vs single-profile {:.1}); not a network bonding gain or acceptance result", gain, bonded_mean, single_mean);
    }
    if let (Some(lossy), Some(bonded)) = (results.get("lossy"), results.get("bonded")) {
        let lossy_mean = lossy.iter().sum::<f64>() / lossy.len() as f64;
        let bonded_mean = bonded.iter().sum::<f64>() / bonded.len() as f64;
        let penalty = if bonded_mean > 0.0 {
            (bonded_mean - lossy_mean) / bonded_mean * 100.0
        } else {
            0.0
        };
        println!("Synthetic loss-event workload difference: {:.1}% (loss-event profile {:.1} vs bonded {:.1}); not a measured packet-loss penalty", penalty, lossy_mean, bonded_mean);
    }
    if let (Some(per_on), Some(per_off)) = (results.get("perflow_on"), results.get("perflow_off")) {
        let on_mean = per_on.iter().sum::<f64>() / per_on.len() as f64;
        let off_mean = per_off.iter().sum::<f64>() / per_off.len() as f64;
        let diff = if off_mean > 0.0 { (on_mean - off_mean) / off_mean * 100.0 } else { 0.0 };
        println!("Per-flow on vs off: {:.1}% diff (on {:.1} vs off {:.1}) — on should have better isolation", diff, on_mean, off_mean);
    }
    if let Some(rigb) = results.get("rigb_wired1G_wifi300M") {
        let mean = rigb.iter().sum::<f64>() / rigb.len() as f64;
        println!("Rig B-shaped capacity configuration local work rate: {:.1} payload-equivalent Mbps; no Rig B hardware or real links were exercised", mean);
    }

    // Failover convergence
    if let Some(failover) = results.get("failover") {
        let mean = failover.iter().sum::<f64>() / failover.len() as f64;
        println!("Synthetic failover-profile work rate: {:.1} payload-equivalent Mbps; this code does not measure transfer continuity or end-to-end convergence", mean);
    }
    if !failover_transition_ms.is_empty() {
        failover_transition_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mean = failover_transition_ms.iter().sum::<f64>() / failover_transition_ms.len() as f64;
        let median = failover_transition_ms[failover_transition_ms.len() / 2];
        println!("Local failover state-transition cost: mean={:.6}ms median={:.6}ms (10 reps) — not end-to-end convergence; Rig B measurement required", mean, median);
    }

    // CPU/Gbit requires real traffic and platform sampling; this harness cannot estimate it.
    println!("\n--- CPU/Gbit ---");
    println!("Not measured by this synthetic harness. Use crypto-bench for cipher microbenchmarks and ETW/WPA under real Rig B traffic for CPU/Gbit.");
    println!("TunnelCipher uses ChaCha20Poly1305 with a 16B tag; header is bound as AAD.");
    println!("ETW events: 200 UPLINK_UP, 201 DOWN, 300 BOND_REVERTED, 400 REKEY, 401 ECN_CE, 402 DELAY, 500 FAILOVER, 600 AUTH_FAILED");
    println!("Pacing: 80% of estimated capacity, floor 1 Mbit/s, prevents burst, leaves headroom for probes");
    println!("Secure mode: Noise_IK handshake, pinned keys, auto_accept=false, replay cache, forward secrecy 1h/1M packets");
    println!("Support bundle: cwnd/in_flight per uplink, congestion diag loss/ecn/delay+paths per_path Vec<PathCongestionDiag> cwnd/in_flight/ecn_ce/delay_events/rtt_min/last_ms, rekey diag, relay stats, failover count+convergence ms, ETW export, route table, adapter state");

    println!("\nSynthetic harness complete: service work-rate and software-path latency only. Network throughput, adapter-added delay, CPU/Gbit/s, and end-to-end failover acceptance remain pending real Rig B measurements.");
}
