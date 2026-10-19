//! Link scoring and capacity estimation (blueprint §6.3).
//!
//! Score = capacity x penalty(loss) x penalty(excess RTT), penalties monotonic.
//! The score feeds WRH weights (§6.2) and the striping quanta (§6.4 rule 2).
//!
//! Capacity comes from two sources, per §6.3:
//! (a) tunnel ACK pacing while the link is loaded, and
//! (b) bounded packet-pair/train probes while it is idle — bounded because
//!     probes cost the user's bytes (proposed cap: 64 KiB/min).

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct LinkScore {
    pub rtt_ewma: Duration,
    pub loss_ewma: f64,
    pub capacity_bps: u64,
}

impl LinkScore {
    pub fn new(rtt: Duration, loss: f64, capacity_bps: u64) -> Self {
        Self { rtt_ewma: rtt, loss_ewma: loss, capacity_bps: capacity_bps }
    }
    /// EWMA update, alpha = 0.2 (proposed §6.3).
    pub fn observe(&mut self, rtt: Duration, lost: bool, alpha: f64) {
        self.rtt_ewma = self.rtt_ewma.mul_f64(1.0 - alpha) + rtt.mul_f64(alpha);
        self.loss_ewma = self.loss_ewma * (1.0 - alpha) + (if lost { 1.0 } else { 0.0 }) * alpha;
    }
    /// score = capacity x penalty(loss) x penalty(excess RTT); monotonic penalties.
    pub fn score(&self, best_rtt: Duration) -> f64 {
        let excess = self.rtt_ewma.saturating_sub(best_rtt).as_secs_f64();
        let loss_pen = 1.0 / (1.0 + 10.0 * self.loss_ewma);
        let rtt_pen = 1.0 / (1.0 + 20.0 * excess);
        self.capacity_bps as f64 * loss_pen * rtt_pen
    }
}

/// Capacity estimator (§6.3). Sans-I/O: the caller supplies `now`.
#[derive(Debug, Clone)]
pub struct CapacityEstimator {
    capacity_bps: u64,
    window_bytes: u64,
    window_start: Instant,
    /// Averaging window for ACK pacing (proposed 500 ms).
    window: Duration,
    /// EWMA smoothing for a new capacity sample (proposed alpha 0.3).
    alpha: f64,
    probe_bytes_this_minute: u64,
    probe_minute_start: Instant,
    /// When this link was last probed *or* last carried traffic.
    last_probe_at: Option<Instant>,
    /// Proposed budget: <= 64 KiB/min of probe traffic (§6.3).
    pub probe_budget_bytes_per_min: u64,
    /// Minimum spacing between idle probes (proposed 5 s). Without it a
    /// service ticking at 1 Hz probes every tick and spends the whole byte
    /// budget in a minute (§6.3b).
    pub probe_interval: Duration,
}

impl CapacityEstimator {
    pub fn new(initial_bps: u64, now: Instant) -> Self {
        Self {
            capacity_bps: initial_bps,
            window_bytes: 0,
            window_start: now,
            window: Duration::from_millis(500),
            alpha: 0.3,
            probe_bytes_this_minute: 0,
            probe_minute_start: now,
            last_probe_at: None,
            probe_budget_bytes_per_min: 64 * 1024,
            probe_interval: Duration::from_secs(5),
        }
    }

    pub fn capacity_bps(&self) -> u64 { self.capacity_bps }

    /// (a) ACK pacing: credit `bytes` acknowledged at `now`; the window rolls
    /// into a capacity sample when it elapses.
    pub fn on_ack(&mut self, bytes: u64, now: Instant) {
        self.window_bytes += bytes;
        self.roll(now);
    }

    /// (b) Idle probe request. Returns the granted probe size, or `None` when
    /// the per-minute probe budget is exhausted (§6.3 bounded probing).
    pub fn request_idle_probe(&mut self, want_bytes: u64, now: Instant) -> Option<u64> {
        // Cadence first: a probe is for a link nothing else is measuring, and
        // it must not be re-issued on every tick (§6.3b).
        if let Some(prev) = self.last_probe_at {
            if now.duration_since(prev) < self.probe_interval { return None; }
        }
        if now.duration_since(self.probe_minute_start) >= Duration::from_secs(60) {
            self.probe_minute_start = now;
            self.probe_bytes_this_minute = 0;
        }
        let left = self.probe_budget_bytes_per_min.saturating_sub(self.probe_bytes_this_minute);
        let granted = want_bytes.min(left);
        if granted == 0 { return None; }
        self.probe_bytes_this_minute += granted;
        self.last_probe_at = Some(now);
        Some(granted)
    }

    /// Discard the partially-filled ACK window and restart it at `now`.
    ///
    /// Sleep or a NAT re-binding makes elapsed time meaningless: a window that
    /// spans the gap divides the bytes by hours and produces a near-zero rate
    /// sample, which drags the estimate (and therefore the DRR weight) towards
    /// zero for a link that is perfectly healthy. Probing is allowed again
    /// immediately, since nothing is being measured any more.
    pub fn restart_window(&mut self, now: Instant) {
        self.window_bytes = 0;
        self.window_start = now;
        self.last_probe_at = None;
    }

    /// Traffic on this link means ACK pacing is already measuring it (§6.3a),
    /// so the next idle probe is pushed out by one interval — the same rule the
    /// keepalive scheduler applies (§6.6).
    pub fn note_traffic(&mut self, now: Instant) { self.last_probe_at = Some(now); }

    /// Feed the result of an idle probe: capacity = bytes * 8 / rtt.
    /// A zero-byte probe carries no capacity information (an RTT-only reply
    /// must not drag the estimate towards zero).
    pub fn on_probe_result(&mut self, bytes: u64, rtt: Duration, _now: Instant) {
        if rtt.is_zero() || bytes == 0 { return; }
        let sample = (bytes as f64 * 8.0) / rtt.as_secs_f64();
        self.blend(sample);
    }

    pub fn probe_bytes_this_minute(&self) -> u64 { self.probe_bytes_this_minute }

    fn roll(&mut self, now: Instant) {
        let elapsed = now.duration_since(self.window_start);
        if elapsed < self.window { return; }
        let sample = (self.window_bytes as f64 * 8.0) / elapsed.as_secs_f64();
        self.window_bytes = 0;
        self.window_start = now;
        if sample > 0.0 { self.blend(sample); }
    }

    fn blend(&mut self, sample: f64) {
        let old = self.capacity_bps as f64;
        let next = old * (1.0 - self.alpha) + sample * self.alpha;
        self.capacity_bps = next.max(0.0) as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_penalties_monotonic() {
        let best = Duration::from_millis(10);
        let a = LinkScore::new(best, 0.0, 100_000_000);
        let b = LinkScore::new(Duration::from_millis(60), 0.0, 100_000_000);
        let c = LinkScore::new(best, 0.1, 100_000_000);
        assert!(a.score(best) > b.score(best));
        assert!(a.score(best) > c.score(best));
    }

    #[test]
    fn ewma_moves_toward_observation() {
        let mut s = LinkScore::new(Duration::from_millis(10), 0.0, 1_000_000);
        for _ in 0..10 { s.observe(Duration::from_millis(50), false, 0.2); }
        assert!(s.rtt_ewma > Duration::from_millis(40), "ewma should approach 50 ms, got {:?}", s.rtt_ewma);
        s.observe(Duration::from_millis(50), true, 0.2);
        assert!(s.loss_ewma > 0.0);
    }

    #[test]
    fn ack_pacing_estimates_capacity() {
        let t0 = Instant::now();
        let mut e = CapacityEstimator::new(1_000_000, t0);
        // 125 KiB acknowledged over 500 ms == 2 Mbit/s
        for i in 1..=5 {
            e.on_ack(25 * 1024, t0 + Duration::from_millis(100 * i));
        }
        let got = e.capacity_bps();
        assert!(got > 1_000_000, "capacity should rise above the seed, got {got}");
        assert!(got < 4_000_000, "and stay in the right order of magnitude, got {got}");
    }

    #[test]
    fn idle_probes_respect_budget() {
        let t0 = Instant::now();
        let mut e = CapacityEstimator::new(1_000_000, t0);
        let step = e.probe_interval;
        assert_eq!(e.request_idle_probe(32 * 1024, t0), Some(32 * 1024));
        assert_eq!(e.request_idle_probe(32 * 1024, t0 + step), Some(32 * 1024));
        assert_eq!(e.request_idle_probe(1024, t0 + step * 2), None, "64 KiB/min budget exhausted");
        // budget resets after a minute
        let t1 = t0 + Duration::from_secs(61);
        assert_eq!(e.request_idle_probe(1024, t1), Some(1024));
    }

    #[test]
    fn idle_probes_have_a_cadence() {
        let t0 = Instant::now();
        let mut e = CapacityEstimator::new(1_000_000, t0);
        let interval = e.probe_interval;
        assert_eq!(e.request_idle_probe(1024, t0), Some(1024));
        assert_eq!(
            e.request_idle_probe(1024, t0 + Duration::from_secs(1)),
            None,
            "a link must not be re-probed on every tick"
        );
        assert_eq!(e.request_idle_probe(1024, t0 + interval + Duration::from_millis(1)), Some(1024));
    }

    #[test]
    fn traffic_suppresses_the_idle_probe() {
        // A link that is carrying data is already measured by ACK pacing
        // (§6.3a); probing it would spend budget and perturb the estimate.
        let t0 = Instant::now();
        let mut e = CapacityEstimator::new(1_000_000, t0);
        let interval = e.probe_interval;
        assert_eq!(e.request_idle_probe(1024, t0), Some(1024));
        e.note_traffic(t0 + interval);
        assert_eq!(e.request_idle_probe(1024, t0 + interval + Duration::from_millis(1)), None);
        assert_eq!(e.request_idle_probe(1024, t0 + interval * 2 + Duration::from_millis(1)), Some(1024));
    }

    #[test]
    fn a_window_spanning_a_sleep_gap_does_not_collapse_the_estimate() {
        // 100 Mbit/s over a 500 ms window is 6.25 MB.
        let t0 = Instant::now();
        let woke = t0 + Duration::from_secs(3600);
        let sample = 6_250_000u64;

        let mut fresh = CapacityEstimator::new(100_000_000, t0);
        fresh.on_ack(sample, t0);
        fresh.restart_window(woke);
        fresh.on_ack(sample, woke + Duration::from_millis(500));
        assert!(
            fresh.capacity_bps() > 90_000_000,
            "a restarted window measures the new session: {}",
            fresh.capacity_bps()
        );

        let mut stale = CapacityEstimator::new(100_000_000, t0);
        stale.on_ack(sample, t0);
        stale.on_ack(sample, woke + Duration::from_millis(500));
        assert!(
            stale.capacity_bps() < 80_000_000,
            "the same bytes over an hour-long window really do collapse the estimate: {}",
            stale.capacity_bps()
        );
    }

    #[test]
    fn probe_result_updates_capacity() {
        let t0 = Instant::now();
        let mut e = CapacityEstimator::new(0, t0);
        // 8 KiB in 8 ms == 8 Mbit/s
        e.on_probe_result(8 * 1024, Duration::from_millis(8), t0);
        assert!(e.capacity_bps() > 2_000_000, "got {}", e.capacity_bps());
    }
}
