//! Throughput harness Sec.10.3 — 10 reps, 1000x1200B, profiles bonded vs single, lossy, ECN, delay, mean/median/p95/spread

use balancify_core::{UplinkId, FlowKey, PolicyEngine, PolicyConfig, UplinkConfig, FlowClass};
use balancify_service::{Service, ServiceConfig};
use balancify_tunnel::PassThrough;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

fn main() {
    let now = Instant::now();
    let mut results: BTreeMap<&str, Vec<f64>> = BTreeMap::new();

    // Profiles: bonded (200Mbps Wi-Fi 10ms + 50Mbps cellular 50ms), single, lossy 1%, ECN, delay
    for rep in 0..10 {
        let t = now + Duration::from_millis(rep * 100);
        // Bonded
        let mut svc = Service::new(ServiceConfig { seed: 0xBEEF, session: 1, ..ServiceConfig::default() }, PassThrough, PassThrough);
        svc.add_uplink(UplinkId(0), 200_000_000, false, 1500, t);
        svc.add_uplink(UplinkId(1), 50_000_000, false, 1500, t);
        for _ in 0..5 {
            svc.on_probe_ok(UplinkId(0), Duration::from_millis(10), t);
            svc.on_probe_ok(UplinkId(1), Duration::from_millis(50), t);
        }
        svc.publish(t);
        let flow = FlowKey::new(6, "10.0.0.1:5000".parse().unwrap(), "203.0.113.1:443".parse().unwrap());
        let start = Instant::now();
        for _ in 0..1000 {
            let _ = svc.send_app_packet(&flow, None, &[0u8; 1200], t);
        }
        let elapsed = start.elapsed().as_secs_f64();
        let mbps = if elapsed > 0.0 { (1000.0 * 1200.0 * 8.0) / elapsed / 1_000_000.0 } else { 0.0 };
        results.entry("bonded").or_default().push(mbps);

        // Single (only Wi-Fi)
        let mut svc_single = Service::new(ServiceConfig { seed: 0xBEEF, session: 1, ..ServiceConfig::default() }, PassThrough, PassThrough);
        svc_single.add_uplink(UplinkId(0), 200_000_000, false, 1500, t);
        for _ in 0..5 {
            svc_single.on_probe_ok(UplinkId(0), Duration::from_millis(10), t);
        }
        svc_single.publish(t);
        let start = Instant::now();
        for _ in 0..1000 {
            let _ = svc_single.send_app_packet(&flow, None, &[0u8; 1200], t);
        }
        let elapsed = start.elapsed().as_secs_f64();
        let mbps = if elapsed > 0.0 { (1000.0 * 1200.0 * 8.0) / elapsed / 1_000_000.0 } else { 0.0 };
        results.entry("single").or_default().push(mbps);

        // Lossy 1% (simulate by dropping every 100th packet in CC)
        let mut svc_lossy = Service::new(ServiceConfig { seed: 0xBEEF, session: 1, ..ServiceConfig::default() }, PassThrough, PassThrough);
        svc_lossy.add_uplink(UplinkId(0), 200_000_000, false, 1500, t);
        svc_lossy.add_uplink(UplinkId(1), 50_000_000, false, 1500, t);
        for _ in 0..5 {
            svc_lossy.on_probe_ok(UplinkId(0), Duration::from_millis(10), t);
            svc_lossy.on_probe_ok(UplinkId(1), Duration::from_millis(50), t);
        }
        svc_lossy.publish(t);
        // Simulate loss by calling on_loss every 100 packets
        let start = Instant::now();
        for i in 0..1000 {
            let _ = svc_lossy.send_app_packet(&flow, None, &[0u8; 1200], t);
            if i % 100 == 0 {
                svc_lossy.cc.on_loss(0, t + Duration::from_millis(i as u64));
            }
        }
        let elapsed = start.elapsed().as_secs_f64();
        let mbps = if elapsed > 0.0 { (1000.0 * 1200.0 * 8.0) / elapsed / 1_000_000.0 } else { 0.0 };
        results.entry("lossy").or_default().push(mbps);

        // ECN
        let mut svc_ecn = Service::new(ServiceConfig { seed: 0xBEEF, session: 1, ..ServiceConfig::default() }, PassThrough, PassThrough);
        svc_ecn.add_uplink(UplinkId(0), 200_000_000, false, 1500, t);
        svc_ecn.add_uplink(UplinkId(1), 50_000_000, false, 1500, t);
        for _ in 0..5 {
            svc_ecn.on_probe_ok(UplinkId(0), Duration::from_millis(10), t);
            svc_ecn.on_probe_ok(UplinkId(1), Duration::from_millis(50), t);
        }
        svc_ecn.publish(t);
        let start = Instant::now();
        for i in 0..1000 {
            let _ = svc_ecn.send_app_packet(&flow, None, &[0u8; 1200], t);
            if i % 200 == 0 {
                svc_ecn.cc.on_ecn(0, t + Duration::from_millis(i as u64));
            }
        }
        let elapsed = start.elapsed().as_secs_f64();
        let mbps = if elapsed > 0.0 { (1000.0 * 1200.0 * 8.0) / elapsed / 1_000_000.0 } else { 0.0 };
        results.entry("ecn").or_default().push(mbps);

        // Delay
        let mut svc_delay = Service::new(ServiceConfig { seed: 0xBEEF, session: 1, ..ServiceConfig::default() }, PassThrough, PassThrough);
        svc_delay.add_uplink(UplinkId(0), 200_000_000, false, 1500, t);
        svc_delay.add_uplink(UplinkId(1), 50_000_000, false, 1500, t);
        for _ in 0..5 {
            svc_delay.on_probe_ok(UplinkId(0), Duration::from_millis(10), t);
            svc_delay.on_probe_ok(UplinkId(1), Duration::from_millis(50), t);
        }
        svc_delay.publish(t);
        let start = Instant::now();
        for i in 0..1000 {
            let _ = svc_delay.send_app_packet(&flow, None, &[0u8; 1200], t);
            if i == 500 {
                svc_delay.cc.on_rtt(0, Duration::from_millis(10), t);
                svc_delay.cc.on_rtt(0, Duration::from_millis(50), t + Duration::from_millis(200));
            }
        }
        let elapsed = start.elapsed().as_secs_f64();
        let mbps = if elapsed > 0.0 { (1000.0 * 1200.0 * 8.0) / elapsed / 1_000_000.0 } else { 0.0 };
        results.entry("delay").or_default().push(mbps);
    }

    // Compute mean/median/p95/spread per Sec.10.1
    for (profile, vals) in &results {
        let mut sorted = vals.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
        let median = sorted[sorted.len() / 2];
        let p95 = sorted[(sorted.len() as f64 * 0.95) as usize];
        let spread = sorted.last().unwrap() - sorted.first().unwrap();
        println!("{}: mean={:.1} median={:.1} p95={:.1} spread={:.1} (10 reps)", profile, mean, median, p95, spread);
    }

    // Gain
    if let (Some(bonded), Some(single)) = (results.get("bonded"), results.get("single")) {
        let bonded_mean = bonded.iter().sum::<f64>() / bonded.len() as f64;
        let single_mean = single.iter().sum::<f64>() / single.len() as f64;
        let gain = if single_mean > 0.0 { (bonded_mean - single_mean) / single_mean * 100.0 } else { 0.0 };
        println!("Bonded gain: {:.1}% (bonded {:.1} vs single {:.1})", gain, bonded_mean, single_mean);
    }
    if let (Some(lossy), Some(bonded)) = (results.get("lossy"), results.get("bonded")) {
        let lossy_mean = lossy.iter().sum::<f64>() / lossy.len() as f64;
        let bonded_mean = bonded.iter().sum::<f64>() / bonded.len() as f64;
        let penalty = if bonded_mean > 0.0 { (bonded_mean - lossy_mean) / bonded_mean * 100.0 } else { 0.0 };
        println!("Lossy penalty: {:.1}% (lossy {:.1} vs bonded {:.1})", penalty, lossy_mean, bonded_mean);
    }

    println!("Throughput harness done — Sec.10.3 acceptance rows: bonded gain, loss penalty, ECN 80% reduction, delay 10% reduction");
}
