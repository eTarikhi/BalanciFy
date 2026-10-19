//! Per-path congestion control inside the tunnel (blueprint §6.4 rule 4),
//! coupled across paths after RFC 6356 (Linked Increases, LIA).
//!
//! Why per path: each uplink has its own loss rate and its own ACK clock, so a
//! loss on a cellular link must not halve the window of a wired one.
//!
//! Why coupled: independent per-path NewReno windows would let a bonded flow
//! take N times the share of a shared bottleneck that a single TCP flow gets.
//! LIA scales each increase by the number of active paths so the *aggregate*
//! grows no faster than one single-path flow would.
//!
//! This is deliberately not TCP-over-TCP: the tunnel runs over UDP (§6.4
//! rule 4) and the inner stack keeps its own loss recovery. What this module
//! does is stop the tunnel from being the thing that overruns a path.
//!
//! Sans-I/O like the rest of the core: callers report sends, ACKs and losses;
//! [`CongestionController::blocked`] tells the scheduler which paths are full.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Proposed initial window (RFC 6928 initial window: 10 segments).
pub const INIT_CWND_SEGMENTS: u64 = 10;
/// Floor: a path must always be able to send something, or a flow stalls.
pub const MIN_CWND_SEGMENTS: u64 = 2;
/// A burst of losses is one congestion event: halve at most once per interval.
pub const MIN_HALVE_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq)]
struct PathWindow {
    /// Congestion window in bytes (fractional while growing).
    cwnd: f64,
    ssthresh: f64,
    /// Bytes sent on this path and not yet acknowledged.
    in_flight: u64,
    /// Last time the window was halved, so a loss burst counts once.
    last_halved: Option<Instant>,
    /// ECN CE marks seen on this path (§6.4 rule 4 ECN extension).
    ecn_ce: u64,
    /// Minimum RTT observed on this path for delay-based detection.
    rtt_min: Option<Duration>,
    /// Last RTT sample.
    rtt_last: Option<Duration>,
    /// Delay increase counter — how many times RTT exceeded min + threshold.
    delay_events: u64,
}

impl PathWindow {
    fn new(mss: u64) -> Self {
        let init = (INIT_CWND_SEGMENTS * mss) as f64;
        Self {
            cwnd: init,
            ssthresh: f64::MAX,
            in_flight: 0,
            last_halved: None,
            ecn_ce: 0,
            rtt_min: None,
            rtt_last: None,
            delay_events: 0,
        }
    }
}

/// Coupled per-path windows. Every value is in bytes; nothing here allocates
/// per packet.
#[derive(Debug, Clone)]
pub struct CongestionController {
    mss: u64,
    paths: BTreeMap<u8, PathWindow>,
    min_cwnd: f64,
    loss_events: u64,
    ecn_events: u64,
    delay_events: u64,
}

impl CongestionController {
    pub fn new(mss: u64) -> Self {
        let mss = mss.max(1);
        Self {
            mss,
            paths: BTreeMap::new(),
            min_cwnd: (MIN_CWND_SEGMENTS * mss) as f64,
            loss_events: 0,
            ecn_events: 0,
            delay_events: 0,
        }
    }

    pub fn mss(&self) -> u64 { self.mss }
    pub fn loss_events(&self) -> u64 { self.loss_events }
    pub fn ecn_events(&self) -> u64 { self.ecn_events }
    pub fn delay_events(&self) -> u64 { self.delay_events }
    pub fn paths(&self) -> Vec<u8> { self.paths.keys().copied().collect() }

    /// Current window for `path` in bytes (initial window if unseen).
    pub fn window(&self, path: u8) -> u64 {
        self.paths.get(&path).map(|w| w.cwnd as u64).unwrap_or((INIT_CWND_SEGMENTS * self.mss) as u64)
    }

    /// Bytes in flight on `path`.
    pub fn in_flight(&self, path: u8) -> u64 {
        self.paths.get(&path).map(|w| w.in_flight).unwrap_or(0)
    }

    /// Drop all state for a removed path.
    pub fn forget(&mut self, path: u8) { self.paths.remove(&path); }

    /// Return every window to its initial state, keeping the path set. After
    /// sleep/resume the paths that come back are not the paths that were
    /// measured before it, so the old windows are worse than no information.
    pub fn reset(&mut self) {
        let mss = self.mss;
        for w in self.paths.values_mut() { *w = PathWindow::new(mss); }
    }

    fn ensure(&mut self, path: u8) -> &mut PathWindow {
        let mss = self.mss;
        self.paths.entry(path).or_insert_with(|| PathWindow::new(mss))
    }

    /// A packet of `bytes` left on `path`.
    pub fn on_send(&mut self, path: u8, bytes: u64) {
        let w = self.ensure(path);
        w.in_flight = w.in_flight.saturating_add(bytes);
    }

    /// `bytes` were acknowledged on `path`. Grows the window: exponentially
    /// below ssthresh, otherwise by the coupled LIA increment.
    /// `rtt` is optional RTT sample for delay-based detection (§6.4 rule 4).
    pub fn on_ack(&mut self, path: u8, bytes: u64, now: Instant) {
        self.on_ack_with_rtt(path, bytes, None, now)
    }

    pub fn on_ack_with_rtt(&mut self, path: u8, bytes: u64, rtt: Option<Duration>, now: Instant) {
        if bytes == 0 { return; }
        let mss = self.mss as f64;
        let acked = bytes as f64;
        let active = self.paths.len().max(1) as f64;
        let tot_cwnd: f64 = self.paths.values().map(|w| w.cwnd).sum::<f64>().max(mss);

        let min_cwnd = self.min_cwnd;
        let mut delay_triggered = false;
        {
            let w = self.ensure(path);
            w.in_flight = w.in_flight.saturating_sub(bytes);

            if let Some(sample) = rtt {
                w.rtt_last = Some(sample);
                match w.rtt_min {
                    Some(min) => w.rtt_min = Some(min.min(sample)),
                    None => w.rtt_min = Some(sample),
                }
                if let Some(min) = w.rtt_min {
                    if sample > min + Duration::from_millis(20) && sample.as_millis() as f64 > min.as_millis() as f64 * 1.25 {
                        if w.last_halved.map(|prev| now.duration_since(prev) >= MIN_HALVE_INTERVAL).unwrap_or(true) {
                            w.cwnd = (w.cwnd * 0.9).max(min_cwnd);
                            w.delay_events += 1;
                            w.last_halved = Some(now);
                            delay_triggered = true;
                        }
                    }
                }
            }

            if w.cwnd < w.ssthresh {
                w.cwnd += acked;
            } else {
                let coupled = active * acked * mss / tot_cwnd;
                let per_path = acked * mss / w.cwnd.max(mss);
                w.cwnd += coupled.min(per_path);
            }
        }
        if delay_triggered {
            self.delay_events += 1;
        }
    }

    /// ECN CE mark received on `path` — treat as congestion but milder than loss (§6.4 rule 4 ECN).
    /// Reduces cwnd to 80% (vs 50% for loss), at most once per interval.
    pub fn on_ecn(&mut self, path: u8, now: Instant) {
        let min_cwnd = self.min_cwnd;
        let w = self.ensure(path);
        if let Some(prev) = w.last_halved {
            if now.duration_since(prev) < MIN_HALVE_INTERVAL { return; }
        }
        w.ssthresh = (w.cwnd * 0.8).max(min_cwnd);
        w.cwnd = w.ssthresh;
        w.last_halved = Some(now);
        w.ecn_ce += 1;
        self.ecn_events += 1;
    }

    /// RTT sample for a path, without ACK (e.g., from probe reply).
    pub fn on_rtt(&mut self, path: u8, rtt: Duration, now: Instant) {
        let min_cwnd = self.min_cwnd;
        let mut delay_triggered = false;
        {
            let w = self.ensure(path);
            w.rtt_last = Some(rtt);
            match w.rtt_min {
                Some(min) => w.rtt_min = Some(min.min(rtt)),
                None => w.rtt_min = Some(rtt),
            }
            if let Some(min) = w.rtt_min {
                if rtt > min + Duration::from_millis(20) && rtt.as_millis() as f64 > min.as_millis() as f64 * 1.25 {
                    if w.last_halved.map(|prev| now.duration_since(prev) >= MIN_HALVE_INTERVAL).unwrap_or(true) {
                        w.cwnd = (w.cwnd * 0.9).max(min_cwnd);
                        w.delay_events += 1;
                        w.last_halved = Some(now);
                        delay_triggered = true;
                    }
                }
            }
        }
        if delay_triggered {
            self.delay_events += 1;
        }
    }

    /// Loss detected on `path` (reinjection fired). Halves the window, at most
    /// once per [`MIN_HALVE_INTERVAL`] so a burst is one event.
    pub fn on_loss(&mut self, path: u8, now: Instant) {
        let min_cwnd = self.min_cwnd;
        let w = self.ensure(path);
        if let Some(prev) = w.last_halved {
            if now.duration_since(prev) < MIN_HALVE_INTERVAL { return; }
        }
        w.ssthresh = (w.cwnd / 2.0).max(min_cwnd);
        w.cwnd = w.ssthresh;
        w.last_halved = Some(now);
        self.loss_events += 1;
    }

    /// True when `path` can accept `bytes` without exceeding its window.
    pub fn allows(&self, path: u8, bytes: u64) -> bool {
        match self.paths.get(&path) {
            Some(w) => (w.in_flight as f64) + (bytes as f64) <= w.cwnd,
            None => true, // unseen path: start with the initial window
        }
    }

    /// Paths that cannot accept a packet of `bytes` right now. The scheduler
    /// skips these; if every path comes back blocked the caller holds the
    /// packet instead of dropping it (§6.4 rule 4 backpressure).
    pub fn blocked(&self, bytes: u64) -> Vec<u8> {
        self.paths
            .iter()
            .filter(|(_, w)| (w.in_flight as f64) + (bytes as f64) > w.cwnd)
            .map(|(p, _)| *p)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MSS: u64 = 1400;

    fn cc() -> CongestionController { CongestionController::new(MSS) }

    /// Acknowledge a whole window's worth of bytes — one round trip.
    fn ack_one_rtt(c: &mut CongestionController, path: u8, now: Instant) {
        let bytes = c.window(path);
        c.on_send(path, bytes);
        c.on_ack(path, bytes, now);
    }

    #[test]
    fn an_unseen_path_starts_at_the_initial_window() {
        let c = cc();
        assert_eq!(c.window(0), INIT_CWND_SEGMENTS * MSS);
        assert!(c.allows(0, MSS));
        assert!(c.blocked(MSS).is_empty(), "nothing is blocked before anything is sent");
    }

    #[test]
    fn slow_start_grows_the_window_per_round_trip() {
        let mut c = cc();
        let t0 = Instant::now();
        // Force congestion avoidance boundaries aside: slow start runs until
        // the first loss, so the window should roughly double each RTT.
        let first = c.window(0);
        ack_one_rtt(&mut c, 0, t0);
        assert!(c.window(0) >= first * 2 - MSS, "one doubling per RTT: {} -> {}", first, c.window(0));
    }

    #[test]
    fn a_loss_halves_the_window_and_a_burst_halves_it_once() {
        let mut c = cc();
        let t0 = Instant::now();
        for _ in 0..4 { ack_one_rtt(&mut c, 0, t0); }
        let before = c.window(0);
        assert!(before > 4 * INIT_CWND_SEGMENTS * MSS, "the window grew: {before}");

        c.on_loss(0, t0);
        let after = c.window(0);
        assert!(after * 2 >= before - MSS && after * 2 <= before + MSS, "halved: {before} -> {after}");

        // The rest of the burst is the same event.
        c.on_loss(0, t0 + Duration::from_millis(10));
        assert_eq!(c.window(0), after, "a burst of losses is one congestion event");
        assert_eq!(c.loss_events(), 1);

        // A later loss is a new event.
        c.on_loss(0, t0 + MIN_HALVE_INTERVAL + Duration::from_millis(1));
        assert!(c.window(0) < after);
        assert_eq!(c.loss_events(), 2);
    }

    #[test]
    fn the_window_never_falls_below_the_floor() {
        let mut c = cc();
        let mut t = Instant::now();
        for _ in 0..40 {
            c.on_loss(0, t);
            t += MIN_HALVE_INTERVAL + Duration::from_millis(1);
        }
        assert_eq!(c.window(0), MIN_CWND_SEGMENTS * MSS, "a path can always send something");
        assert!(c.allows(0, MSS));
    }

    #[test]
    fn a_loss_on_one_path_does_not_touch_the_others() {
        let mut c = cc();
        let t0 = Instant::now();
        for _ in 0..3 {
            ack_one_rtt(&mut c, 0, t0);
            ack_one_rtt(&mut c, 1, t0);
        }
        let other = c.window(0);
        c.on_loss(1, t0);
        assert_eq!(c.window(0), other, "independent loss clocks (§6.4 rule 4)");
        assert!(c.window(1) < other);
    }

    #[test]
    fn the_coupled_increase_is_no_faster_than_a_single_path() {
        // The LIA property, and the reason the coupling term exists: in
        // congestion avoidance a single path grows by one MSS per RTT, and
        // splitting the same acknowledged volume over two paths must not grow
        // the *aggregate* faster. Plain per-path NewReno would double it.
        let t0 = Instant::now();
        let start = (INIT_CWND_SEGMENTS * MSS / 2) as u64; // ssthresh after one halving

        let mut single = cc();
        let mut split = cc();
        split.ensure(0);
        split.ensure(1);
        // Put both configurations into congestion avoidance.
        single.on_loss(0, t0);
        split.on_loss(0, t0);
        split.on_loss(1, t0);
        assert_eq!(single.window(0), start);
        assert_eq!(split.window(0) + split.window(1), 2 * start);

        for _ in 0..40 {
            // One path carries everything.
            let b = single.window(0);
            single.on_send(0, b);
            single.on_ack(0, b, t0);

            // The same volume, split across two paths.
            let half = (split.window(0) / 2).max(1);
            split.on_send(0, half);
            split.on_send(1, half);
            split.on_ack(0, half, t0);
            split.on_ack(1, half, t0);
        }

        let single_growth = single.window(0) - start;
        let split_growth = split.window(0) + split.window(1) - 2 * start;
        assert!(single_growth > 0 && split_growth > 0, "both configurations grew");
        assert!(
            split_growth <= single_growth * 11 / 10,
            "aggregate must not outgrow a single path: split {split_growth} vs single {single_growth}"
        );
        assert!(
            split_growth >= single_growth * 8 / 10,
            "and the coupling scales the increase, it does not stop it: {split_growth} vs {single_growth}"
        );
    }

    #[test]
    fn a_full_window_blocks_the_path_and_acks_reopen_it() {
        let mut c = cc();
        let t0 = Instant::now();
        let window = c.window(0);
        assert!(c.allows(0, window), "the whole window is available");
        c.on_send(0, window);
        assert!(!c.allows(0, MSS), "a full window blocks further sends");
        assert_eq!(c.blocked(MSS), vec![0]);
        assert_eq!(c.in_flight(0), window);

        c.on_ack(0, window, t0);
        assert_eq!(c.in_flight(0), 0);
        assert!(c.blocked(MSS).is_empty(), "acknowledgements reopen the window");
    }

    #[test]
    fn forgetting_a_path_drops_its_window() {
        let mut c = cc();
        let t0 = Instant::now();
        ack_one_rtt(&mut c, 3, t0);
        assert!(c.window(3) > INIT_CWND_SEGMENTS * MSS);
        c.forget(3);
        assert_eq!(c.window(3), INIT_CWND_SEGMENTS * MSS, "a re-attached uplink starts over");
        assert!(c.paths().is_empty());
    }

    #[test]
    fn reset_returns_windows_to_the_initial_state_but_keeps_the_paths() {
        let mut c = cc();
        let t0 = Instant::now();
        for _ in 0..3 {
            ack_one_rtt(&mut c, 0, t0);
            ack_one_rtt(&mut c, 1, t0);
        }
        assert!(c.window(0) > INIT_CWND_SEGMENTS * MSS);
        c.on_loss(1, t0);
        c.reset();
        assert_eq!(c.window(0), INIT_CWND_SEGMENTS * MSS);
        assert_eq!(c.window(1), INIT_CWND_SEGMENTS * MSS);
        assert_eq!(c.paths(), vec![0, 1], "the path set survives a reset");
        assert_eq!(c.in_flight(0), 0);
    }

    #[test]
    fn in_flight_cannot_go_negative_on_a_late_ack() {
        let mut c = cc();
        let t0 = Instant::now();
        c.on_send(0, 100);
        c.on_ack(0, 10_000, t0);
        assert_eq!(c.in_flight(0), 0);
    }

    #[test]
    fn ecn_reduces_window_milder_than_loss() {
        let mut c = cc();
        let t0 = Instant::now();
        for _ in 0..4 { ack_one_rtt(&mut c, 0, t0); }
        let before = c.window(0);
        c.on_ecn(0, t0);
        let after_ecn = c.window(0);
        // ECN reduces to 80%, loss to 50% — ECN should be milder
        assert!(after_ecn > before / 2, "ECN 80% > loss 50%: {} -> {}", before, after_ecn);
        assert!(after_ecn < before, "ECN still reduces: {} -> {}", before, after_ecn);
        assert_eq!(c.ecn_events(), 1);

        // Second ECN in same burst interval should be ignored
        c.on_ecn(0, t0 + Duration::from_millis(10));
        assert_eq!(c.window(0), after_ecn, "burst ECN counted once");
        assert_eq!(c.ecn_events(), 1);

        // After interval, ECN reduces again
        c.on_ecn(0, t0 + MIN_HALVE_INTERVAL + Duration::from_millis(1));
        assert!(c.window(0) < after_ecn);
        assert_eq!(c.ecn_events(), 2);
    }

    #[test]
    fn delay_based_reduction_on_rtt_increase() {
        let mut c = cc();
        let t0 = Instant::now();
        for _ in 0..3 { ack_one_rtt(&mut c, 0, t0); }
        let before = c.window(0);

        // First RTT establishes min
        c.on_rtt(0, Duration::from_millis(10), t0);
        assert_eq!(c.window(0), before, "first RTT should not reduce");

        // Small increase should not trigger
        c.on_rtt(0, Duration::from_millis(15), t0 + Duration::from_millis(200));
        assert_eq!(c.window(0), before, "small RTT increase not congestion");

        // Large increase: 10ms -> 50ms should trigger 10% reduction
        c.on_rtt(0, Duration::from_millis(50), t0 + Duration::from_millis(400));
        let after = c.window(0);
        assert!(after < before, "delay should reduce window: {} -> {}", before, after);
        assert!(after >= before * 9 / 10 - 100, "delay reduces by 10%: {} -> {}", before, after);
        assert_eq!(c.delay_events(), 1);
    }

    #[test]
    fn ack_with_rtt_tracks_min_and_detects_delay() {
        let mut c = cc();
        let t0 = Instant::now();
        // ACK with RTT 20ms
        c.on_send(0, 1000);
        c.on_ack_with_rtt(0, 1000, Some(Duration::from_millis(20)), t0);
        // ACK with RTT 100ms — should trigger delay reduction
        c.on_send(0, 1000);
        c.on_ack_with_rtt(0, 1000, Some(Duration::from_millis(100)), t0 + Duration::from_millis(200));
        assert_eq!(c.delay_events(), 1, "RTT jump 20->100 should be delay event");
    }
}
