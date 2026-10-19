//! Packet pacing in front of the congestion window (Sec.6.4 rule 4, Sec.10.3).
//!
//! The congestion window stops the tunnel from overrunning a path, but it does
//! not stop it from sending a whole window as one burst at line rate. That
//! burst can overflow the bottleneck queue and show up as added delay in the
//! Sec.10.3 throughput rows. Pacing spreads the window over an RTT.
//!
//! This is sans-I/O: callers tell the pacer when they sent bytes and ask when
//! they may send next. The service loop holds packets until the pacer's
//! deadline, rather than dropping them.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// How far ahead the pacer may schedule. Beyond this, the caller should hold
/// the packet rather than sleep.
const MAX_PACING_DELAY: Duration = Duration::from_millis(100);

/// A single-path pacer: token-bucket style, but time-based.
/// `rate_bps = 0` means disabled (no pacing), which is the default before
/// capacity is known.
#[derive(Debug, Clone)]
pub struct Pacer {
    /// Pacing rate in bytes per second. 0 = disabled.
    rate_bps: u64,
    /// When the next packet may be sent.
    next_send: Option<Instant>,
    /// Last time we sent, for rate calculation.
    last_send: Option<Instant>,
}

impl Pacer {
    pub fn new(rate_bps: u64) -> Self {
        Self { rate_bps, next_send: None, last_send: None }
    }

    pub fn rate_bps(&self) -> u64 { self.rate_bps }

    pub fn set_rate(&mut self, rate_bps: u64) { self.rate_bps = rate_bps; }

    /// Record that `bytes` were sent at `now`, and compute when the next send
    /// may happen. Returns the earliest time the next packet may be sent.
    pub fn on_send(&mut self, bytes: u64, now: Instant) -> Instant {
        if self.rate_bps == 0 {
            self.last_send = Some(now);
            return now;
        }
        // Time needed to pace these bytes at current rate: bytes / (rate_bps)
        let ns = (bytes as u128 * 1_000_000_000u128) / (self.rate_bps as u128);
        let delay = Duration::from_nanos(ns.min(MAX_PACING_DELAY.as_nanos()) as u64);

        let next = now + delay;
        self.next_send = Some(next);
        self.last_send = Some(now);
        next
    }

    /// When can we send next? None if we can send now.
    pub fn next_send_time(&self, now: Instant) -> Option<Instant> {
        if self.rate_bps == 0 { return None; }
        if let Some(next) = self.next_send {
            if next > now { Some(next) } else { None }
        } else {
            None
        }
    }

    /// How long to wait before sending? Zero if can send now.
    pub fn delay_until_next(&self, now: Instant) -> Duration {
        self.next_send_time(now).map(|t| t.duration_since(now)).unwrap_or(Duration::ZERO)
    }

    /// True if we can send now.
    pub fn can_send(&self, now: Instant) -> bool { self.next_send_time(now).is_none() }

    /// Reset after sleep/resume or path change.
    pub fn reset(&mut self) {
        self.next_send = None;
        self.last_send = None;
    }
}

/// Per-path pacing controller.
#[derive(Debug, Clone, Default)]
pub struct PacingController {
    pacers: BTreeMap<u8, Pacer>,
    default_rate_bps: u64,
}

impl PacingController {
    pub fn new(default_rate_bps: u64) -> Self {
        Self { pacers: BTreeMap::new(), default_rate_bps }
    }

    pub fn with_default_rate(mut self, rate_bps: u64) -> Self {
        self.default_rate_bps = rate_bps;
        self
    }

    fn ensure(&mut self, path: u8) -> &mut Pacer {
        let default_rate = self.default_rate_bps;
        self.pacers.entry(path).or_insert_with(|| Pacer::new(default_rate))
    }

    pub fn set_rate(&mut self, path: u8, rate_bps: u64) {
        self.ensure(path).set_rate(rate_bps);
    }

    pub fn on_send(&mut self, path: u8, bytes: u64, now: Instant) -> Instant {
        self.ensure(path).on_send(bytes, now)
    }

    pub fn can_send(&self, path: u8, now: Instant) -> bool {
        self.pacers.get(&path).map(|p| p.can_send(now)).unwrap_or(true)
    }

    pub fn delay_until_next(&self, path: u8, now: Instant) -> Duration {
        self.pacers.get(&path).map(|p| p.delay_until_next(now)).unwrap_or(Duration::ZERO)
    }

    pub fn next_send_time(&self, path: u8, now: Instant) -> Option<Instant> {
        self.pacers.get(&path).and_then(|p| p.next_send_time(now))
    }

    /// Paths that are paced (cannot send now).
    pub fn paced_paths(&self, now: Instant) -> Vec<u8> {
        self.pacers.iter().filter(|(_, p)| !p.can_send(now)).map(|(id, _)| *id).collect()
    }

    pub fn forget(&mut self, path: u8) { self.pacers.remove(&path); }

    pub fn reset(&mut self) {
        for p in self.pacers.values_mut() { p.reset(); }
    }

    pub fn reset_path(&mut self, path: u8) {
        if let Some(p) = self.pacers.get_mut(&path) { p.reset(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Instant { Instant::now() }

    #[test]
    fn pacer_paces_at_configured_rate() {
        let mut pacer = Pacer::new(1_000_000); // 1 MB/s
        let t0 = now();
        // 1000 bytes at 1 MB/s = 1 ms
        let next = pacer.on_send(1000, t0);
        assert!(next > t0);
        let delay = next.duration_since(t0);
        assert!(delay >= Duration::from_micros(900) && delay <= Duration::from_millis(2),
                "1000 bytes at 1 MB/s should be ~1 ms, got {delay:?}");
        assert!(!pacer.can_send(t0), "cannot send immediately after");
        assert!(pacer.can_send(next), "can send at next time");
        assert!(pacer.can_send(next + Duration::from_millis(1)), "can send after");
    }

    #[test]
    fn pacer_allows_send_when_idle() {
        let pacer = Pacer::new(1_000_000);
        assert!(pacer.can_send(now()), "idle pacer can send");
        assert_eq!(pacer.delay_until_next(now()), Duration::ZERO);
    }

    #[test]
    fn pacer_respects_max_delay() {
        let mut pacer = Pacer::new(1); // 1 B/s, very slow
        let t0 = now();
        // 1 MB at 1 B/s would be 11 days, but we cap at 100 ms
        let next = pacer.on_send(1_000_000, t0);
        let delay = next.duration_since(t0);
        assert!(delay <= MAX_PACING_DELAY, "pacing delay capped at {MAX_PACING_DELAY:?}, got {delay:?}");
    }

    #[test]
    fn pacing_controller_per_path() {
        let mut ctrl = PacingController::new(10_000_000);
        let t0 = now();
        ctrl.on_send(0, 1000, t0);
        ctrl.on_send(1, 1000, t0);
        // Both paced
        assert!(!ctrl.can_send(0, t0));
        assert!(!ctrl.can_send(1, t0));
        // After delay, can send
        let t1 = t0 + Duration::from_millis(10);
        assert!(ctrl.can_send(0, t1));
        assert!(ctrl.can_send(1, t1));
    }

    #[test]
    fn pacing_controller_forget_and_reset() {
        let mut ctrl = PacingController::new(1_000_000);
        let t0 = now();
        ctrl.on_send(0, 1000, t0);
        assert!(!ctrl.can_send(0, t0));
        ctrl.forget(0);
        assert!(ctrl.can_send(0, t0), "forgotten path can send");
        ctrl.on_send(1, 1000, t0);
        ctrl.reset();
        assert!(ctrl.can_send(1, t0), "reset clears pacing");
    }

    #[test]
    fn pacing_rate_update() {
        let mut ctrl = PacingController::new(1_000_000);
        let t0 = now();
        ctrl.set_rate(0, 10_000_000); // 10x faster
        let next_fast = ctrl.on_send(0, 1000, t0);
        ctrl.reset_path(0);
        ctrl.set_rate(0, 1_000_000);
        let next_slow = ctrl.on_send(0, 1000, t0);
        assert!(next_fast < next_slow, "faster rate => shorter delay");
    }

    #[test]
    fn pacing_prevents_burst() {
        // Simulate sending a whole window (14 KB) at 10 Mbit/s without pacing:
        // it would leave as one burst. With pacing at 10 Mbit/s, it should be
        // spread over ~11 ms.
        let mut pacer = Pacer::new(10_000_000 / 8); // 10 Mbit/s = 1.25 MB/s
        let mut t = now();
        let window = 14_000u64;
        let mut sends = Vec::new();
        // Send window as 10 packets of 1400 bytes
        for _ in 0..10 {
            if !pacer.can_send(t) {
                t = pacer.next_send_time(t).unwrap();
            }
            let next = pacer.on_send(1400, t);
            sends.push(t);
            t = next;
        }
        let total_time = sends.last().unwrap().duration_since(sends[0]);
        // 14 KB at 1.25 MB/s = ~11 ms
        assert!(total_time >= Duration::from_millis(8), "pacing spreads burst: {total_time:?}");
        assert!(total_time <= Duration::from_millis(20), "but not too much: {total_time:?}");
        let _ = window; // avoid unused
    }
}
