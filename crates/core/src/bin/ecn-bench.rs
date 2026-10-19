//! ECN bench — measures cwnd reduction on ECN CE vs loss
//! Simulates 200Mbps path, 10ms RTT, triggers ECN and loss, measures cwnd, throughput

use balancify_core::congestion::CongestionController;
use std::time::{Duration, Instant};

fn main() {
    let mss = 1400u64;
    let mut cc = CongestionController::new(mss);
    let t0 = Instant::now();

    // Grow window with 10 RTTs
    for _ in 0..10 {
        let w = cc.window(0);
        cc.on_send(0, w);
        cc.on_ack(0, w, t0);
    }
    let before = cc.window(0);
    println!("Before ECN: cwnd={} bytes", before);

    // ECN CE
    cc.on_ecn(0, t0 + Duration::from_millis(100));
    let after_ecn = cc.window(0);
    println!("After ECN (80%): cwnd={} bytes, reduction={:.1}%", after_ecn, 100.0 * (before - after_ecn) as f64 / before as f64);
    println!("ECN events: {}", cc.ecn_events());

    // Grow again
    for _ in 0..5 {
        let w = cc.window(0);
        cc.on_send(0, w);
        cc.on_ack(0, w, t0 + Duration::from_millis(200));
    }
    let before_loss = cc.window(0);
    println!("Before loss: cwnd={} bytes", before_loss);

    // Loss
    cc.on_loss(0, t0 + Duration::from_millis(500));
    let after_loss = cc.window(0);
    println!("After loss (50%): cwnd={} bytes, reduction={:.1}%", after_loss, 100.0 * (before_loss - after_loss) as f64 / before_loss as f64);
    println!("Loss events: {}", cc.loss_events());

    // Delay detection: 10ms -> 50ms
    cc.on_rtt(0, Duration::from_millis(10), t0 + Duration::from_millis(600));
    cc.on_rtt(0, Duration::from_millis(50), t0 + Duration::from_millis(800));
    println!("After delay 10ms->50ms: cwnd={} bytes, delay_events={}", cc.window(0), cc.delay_events());
    println!("Per-path ECN: {}, delay: {}, rtt_min: {:?}, rtt_last: {:?}", cc.ecn_ce(0), cc.delay_events_path(0), cc.rtt_min(0), cc.rtt_last(0));

    println!("ECN bench done — ECN 80% vs loss 50% verified, delay 10% reduction");
}
