//! Per-path keepalives and path-change handling (blueprint §6.6).
//!
//! "Per-path NAT keepalives (proposed ≤ 25 s); detect changed NAT mapping as a
//! *path change* (re-key path, keep session)."
//!
//! Probes are authenticated tunnel control messages, not raw ICMP, so they
//! survive CGNAT (§6.5).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

/// Proposed NAT keepalive ceiling (§6.6).
pub const MAX_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(25);
/// Proposed keepalive cadence: half the ceiling, so a single lost keepalive
/// cannot let a NAT binding expire.
pub const DEFAULT_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(12);

/// Schedules per-path keepalives. Any outbound traffic on a path refreshes the
/// NAT binding, so only genuinely idle paths generate keepalive traffic.
#[derive(Debug, Clone)]
pub struct KeepaliveScheduler {
    interval: Duration,
    next_due: BTreeMap<u8, Instant>,
    sent: u64,
}

impl KeepaliveScheduler {
    /// `interval` is clamped to the §6.6 ceiling (25 s).
    pub fn new(interval: Duration) -> Self {
        Self { interval: interval.min(MAX_KEEPALIVE_INTERVAL), next_due: BTreeMap::new(), sent: 0 }
    }
    pub fn with_defaults() -> Self { Self::new(DEFAULT_KEEPALIVE_INTERVAL) }
    pub fn interval(&self) -> Duration { self.interval }
    pub fn sent(&self) -> u64 { self.sent }

    /// True when a keepalive is due on `path` at `now` (and arms the next one).
    pub fn due(&mut self, path: u8, now: Instant) -> bool {
        let due_at = *self.next_due.entry(path).or_insert(now + self.interval);
        if now < due_at { return false; }
        self.next_due.insert(path, now + self.interval);
        self.sent += 1;
        true
    }

    /// Real traffic refreshed the binding; push the next keepalive out.
    pub fn on_traffic(&mut self, path: u8, now: Instant) {
        self.next_due.insert(path, now + self.interval);
    }

    /// Path removed (adapter unplugged): stop scheduling for it.
    pub fn forget(&mut self, path: u8) { self.next_due.remove(&path); }

    /// Make the listed paths due immediately. Sleep/resume and NAT re-binding
    /// invalidate the mapping, so the binding has to be re-established before
    /// traffic is offered on it again (§6.6).
    pub fn expire_all(&mut self, paths: &[u8], now: Instant) {
        for p in paths { self.next_due.insert(*p, now); }
    }

    /// Paths currently armed.
    pub fn paths(&self) -> Vec<u8> { self.next_due.keys().copied().collect() }
}

/// Observed local/source endpoint of a path, used to detect NAT re-binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathObservation {
    pub path: u8,
    pub local: SocketAddr,
}

/// A changed source address/port on a path is a *path change*: re-key the path,
/// keep the session (§6.6). Returns `true` when the mapping changed.
pub fn path_changed(previous: Option<&SocketAddr>, observed: SocketAddr) -> bool {
    match previous {
        None => false,          // first observation establishes the mapping
        Some(prev) => *prev != observed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    fn addr(port: u16) -> SocketAddr { SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 5), port)) }

    #[test]
    fn keepalive_interval_is_capped_at_25s() {
        let s = KeepaliveScheduler::new(Duration::from_secs(600));
        assert_eq!(s.interval(), MAX_KEEPALIVE_INTERVAL, "§6.6 ceiling");
        assert_eq!(KeepaliveScheduler::with_defaults().interval(), Duration::from_secs(12));
    }

    #[test]
    fn keepalive_fires_once_per_interval_per_path() {
        let mut s = KeepaliveScheduler::new(Duration::from_secs(12));
        let t0 = Instant::now();
        s.on_traffic(0, t0); // arms path 0 for t0+12
        s.on_traffic(1, t0);
        assert!(!s.due(0, t0), "a freshly armed path is not due immediately");
        assert!(!s.due(0, t0 + Duration::from_secs(5)));
        assert!(s.due(0, t0 + Duration::from_secs(13)));
        assert!(!s.due(0, t0 + Duration::from_secs(14)), "armed for another full interval");
        assert!(s.due(1, t0 + Duration::from_secs(13)), "paths are scheduled independently");
        assert!(!s.due(1, t0 + Duration::from_secs(20)));
        assert!(s.due(1, t0 + Duration::from_secs(26)));
        assert_eq!(s.sent(), 3);
        assert_eq!(s.paths(), vec![0, 1]);
    }

    #[test]
    fn traffic_suppresses_the_keepalive() {
        let mut s = KeepaliveScheduler::new(Duration::from_secs(12));
        let t0 = Instant::now();
        s.on_traffic(0, t0 + Duration::from_secs(10));
        assert!(!s.due(0, t0 + Duration::from_secs(13)));
        assert!(s.due(0, t0 + Duration::from_secs(23)));
    }

    #[test]
    fn forgetting_a_path_stops_its_keepalives() {
        let mut s = KeepaliveScheduler::new(Duration::from_secs(12));
        let t0 = Instant::now();
        s.on_traffic(3, t0);
        s.forget(3);
        assert!(s.paths().is_empty());
        assert!(!s.due(3, t0 + Duration::from_secs(60)), "re-arms from scratch");
        assert!(s.due(3, t0 + Duration::from_secs(73)));
    }

    #[test]
    fn nat_rebinding_is_a_path_change_not_a_session_change() {
        let a = addr(41_000);
        let b = addr(41_001);
        assert!(!path_changed(None, a), "first observation is not a change");
        assert!(!path_changed(Some(&a), a));
        assert!(path_changed(Some(&a), b), "NAT re-bound the port => re-key, keep session");
    }

    #[test]
    fn expire_all_makes_paths_due_immediately() {
        let t0 = Instant::now();
        let mut s = KeepaliveScheduler::with_defaults();
        assert!(!s.due(0, t0), "the first call arms the timer rather than firing");
        assert!(!s.due(0, t0 + Duration::from_secs(1)));
        s.expire_all(&[0, 1], t0 + Duration::from_secs(2));
        assert!(s.due(0, t0 + Duration::from_secs(2)), "resume re-establishes the mapping at once");
        assert!(s.due(1, t0 + Duration::from_secs(2)));
        assert!(!s.due(0, t0 + Duration::from_secs(3)), "then it runs on its normal cadence");
    }
}
