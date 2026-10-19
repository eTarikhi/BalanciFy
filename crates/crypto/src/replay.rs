//! Replay protection for handshake first flights (Sec.9.2, Sec.11.4).
//!
//! A captured first flight (`e || EncryptAndHash(s) || ...`) could be replayed
//! at a new responder session. The handshake itself is fresh because of the
//! ephemeral, but the responder would still perform two DH operations and
//! allocate state. A replay cache makes that cheap to reject.
//!
//! The cache stores the ephemeral public key (32 bytes) from flight 1, or the
//! handshake hash, for a bounded time. If the same ephemeral appears twice
//! within the window, the second flight is rejected.
//!
//! This is sans-I/O: time is an `Instant` argument, storage is in-memory.
//! Production use would persist the cache across restarts or use a Bloom filter
//! for larger scale.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// How long a seen ephemeral is remembered. Longer than the typical handshake
/// timeout (5 s) but short enough to bound memory.
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(60);
/// Maximum number of entries before old ones are evicted.
pub const DEFAULT_CAPACITY: usize = 1024;

#[derive(Debug, Clone)]
struct Entry {
    seen_at: Instant,
}

/// A time-bounded set of recently seen handshake ephemerals.
#[derive(Debug)]
pub struct ReplayCache {
    window: Duration,
    capacity: usize,
    map: HashMap<[u8; 32], Entry>,
    order: VecDeque<[u8; 32]>, // insertion order for eviction
}

impl ReplayCache {
    pub fn new(window: Duration, capacity: usize) -> Self {
        Self { window, capacity: capacity.max(1), map: HashMap::new(), order: VecDeque::new() }
    }

    pub fn with_defaults() -> Self { Self::new(DEFAULT_WINDOW, DEFAULT_CAPACITY) }

    /// Returns true if this ephemeral was seen recently (replay), false if it
    /// is new (and now recorded).
    pub fn check_and_insert(&mut self, ephemeral_public: [u8; 32], now: Instant) -> bool {
        self.evict_expired(now);
        if self.map.contains_key(&ephemeral_public) {
            return true; // replay
        }
        if self.map.len() >= self.capacity {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
        self.map.insert(ephemeral_public, Entry { seen_at: now });
        self.order.push_back(ephemeral_public);
        false
    }

    /// Remove entries older than the window.
    pub fn evict_expired(&mut self, now: Instant) {
        while let Some(front) = self.order.front() {
            if let Some(entry) = self.map.get(front) {
                if now.duration_since(entry.seen_at) <= self.window {
                    break;
                }
            }
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }

    pub fn len(&self) -> usize { self.map.len() }
    pub fn is_empty(&self) -> bool { self.map.is_empty() }

    /// For testing: clear all.
    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ephemeral_is_not_replay() {
        let mut cache = ReplayCache::with_defaults();
        let now = Instant::now();
        let eph = [1u8; 32];
        assert!(!cache.check_and_insert(eph, now), "first time is not replay");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn same_ephemeral_within_window_is_replay() {
        let mut cache = ReplayCache::with_defaults();
        let now = Instant::now();
        let eph = [2u8; 32];
        assert!(!cache.check_and_insert(eph, now));
        assert!(cache.check_and_insert(eph, now + Duration::from_secs(1)), "second time is replay");
        assert_eq!(cache.len(), 1, "replay does not grow the cache");
    }

    #[test]
    fn expired_entries_are_not_replay() {
        let mut cache = ReplayCache::new(Duration::from_secs(10), 100);
        let now = Instant::now();
        let eph = [3u8; 32];
        assert!(!cache.check_and_insert(eph, now));
        // After window, it should be evicted and not considered replay.
        assert!(!cache.check_and_insert(eph, now + Duration::from_secs(11)), "expired entry is not replay");
    }

    #[test]
    fn capacity_evicts_oldest() {
        let mut cache = ReplayCache::new(DEFAULT_WINDOW, 2);
        let now = Instant::now();
        let e1 = [10u8; 32];
        let e2 = [20u8; 32];
        let e3 = [30u8; 32];
        assert!(!cache.check_and_insert(e1, now));
        assert!(!cache.check_and_insert(e2, now));
        assert_eq!(cache.len(), 2);
        assert!(!cache.check_and_insert(e3, now));
        assert_eq!(cache.len(), 2, "capacity bounds memory");
        // e1 was evicted, so it is not replay anymore.
        assert!(!cache.check_and_insert(e1, now), "evicted entry is not replay");
    }

    #[test]
    fn different_ephemerals_are_not_replay() {
        let mut cache = ReplayCache::with_defaults();
        let now = Instant::now();
        for i in 0..10u8 {
            let mut eph = [0u8; 32];
            eph[0] = i;
            assert!(!cache.check_and_insert(eph, now));
        }
        assert_eq!(cache.len(), 10);
    }
}
