//! Flow table with affinity (blueprint §6.2 rule 3).
//!
//! A flow stays on its uplink until it idles out (proposed 120 s) or its
//! uplink leaves the healthy set. **No opportunistic migration** — that is what
//! prevents oscillation and server-side session confusion. Latency-sensitive
//! flows are the documented exception: they follow the lowest-delay healthy
//! uplink, because that is the property the class was created for.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::placement::{FlowClass, FlowKey, UplinkId};
use crate::policy::PolicySnapshot;

/// Proposed idle-out (§6.2 rule 3).
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FlowStats {
    pub active: usize,
    pub placements: u64,
    pub affinity_hits: u64,
    pub rehashes: u64,
    pub idle_out: u64,
    pub refused: u64,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    uplink: UplinkId,
    class: FlowClass,
    last_seen: Instant,
}

pub struct FlowTable {
    entries: HashMap<FlowKey, Entry>,
    idle_timeout: Duration,
    stats: FlowStats,
}

impl FlowTable {
    pub fn new(idle_timeout: Duration) -> Self {
        Self { entries: HashMap::new(), idle_timeout, stats: FlowStats::default() }
    }
    pub fn with_defaults() -> Self { Self::new(DEFAULT_IDLE_TIMEOUT) }

    pub fn idle_timeout(&self) -> Duration { self.idle_timeout }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    pub fn stats(&self) -> FlowStats { FlowStats { active: self.entries.len(), ..self.stats } }
    pub fn uplink_of(&self, flow: &FlowKey) -> Option<UplinkId> { self.entries.get(flow).map(|e| e.uplink) }

    /// Place a flow, honouring affinity. Returns `None` when no uplink can carry
    /// it (all down / excluded) — the caller then applies the fail-open or
    /// fail-closed default (§9.2, decision D3).
    pub fn place(
        &mut self,
        flow: &FlowKey,
        class: FlowClass,
        policy: &PolicySnapshot,
        now: Instant,
    ) -> Option<UplinkId> {
        // Latency-sensitive flows re-evaluate: the class exists to track delay.
        if class == FlowClass::LatencySensitive {
            let Some(uplink) = policy.place(flow, class) else {
                self.stats.refused += 1;
                return None;
            };
            self.entries.insert(flow.clone(), Entry { uplink, class, last_seen: now });
            self.stats.placements += 1;
            return Some(uplink);
        }

        // `Entry` is Copy: read it out so the immutable borrow ends before we
        // take a mutable one (the if-let scrutinee would otherwise hold it).
        if let Some(e) = self.entries.get(flow).copied() {
            if policy.is_healthy(e.uplink) && e.class == class {
                if let Some(slot) = self.entries.get_mut(flow) { slot.last_seen = now; }
                self.stats.affinity_hits += 1;
                return Some(e.uplink);
            }
            self.stats.rehashes += 1;
        } else {
            self.stats.placements += 1;
        }

        let Some(uplink) = policy.place(flow, class) else {
            self.stats.refused += 1;
            self.entries.remove(flow);
            return None;
        };
        self.entries.insert(flow.clone(), Entry { uplink, class, last_seen: now });
        Some(uplink)
    }

    /// Idle-out sweep (§6.2 rule 3); returns how many entries were dropped.
    pub fn sweep(&mut self, now: Instant) -> usize {
        let timeout = self.idle_timeout;
        let before = self.entries.len();
        self.entries.retain(|_, e| now.duration_since(e.last_seen) < timeout);
        let dropped = before - self.entries.len();
        self.stats.idle_out += dropped as u64;
        dropped
    }

    /// Drop affinity for every flow that was on `uplink` (adapter removed).
    /// Returns the number of entries dropped; the flows re-place on next packet.
    pub fn forget_uplink(&mut self, uplink: UplinkId) -> usize {
        let before = self.entries.len();
        self.entries.retain(|_, e| e.uplink != uplink);
        let dropped = before - self.entries.len();
        self.stats.rehashes += dropped as u64;
        dropped
    }

    pub fn clear(&mut self) { self.entries.clear(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::LinkState;
    use crate::testutil::sa;
    use std::time::Duration;

    fn snapshot(uplinks: Vec<(UplinkId, f64, Duration, LinkState)>) -> PolicySnapshot {
        PolicySnapshot {
            revision: 1,
            seed: 7,
            best_rtt: uplinks.iter().map(|u| u.2).min().unwrap_or(Duration::ZERO),
            uplinks: uplinks
                .into_iter()
                .map(|(id, weight, rtt, state)| crate::policy::UplinkView {
                    id,
                    state,
                    weight,
                    rtt,
                    loss: 0.0,
                    capacity_bps: 100_000_000,
                    metered: false,
                    eligible: weight > 0.0,
                })
                .collect(),
        }
    }

    fn two_up() -> PolicySnapshot {
        snapshot(vec![
            (UplinkId(0), 1.0, Duration::from_millis(10), LinkState::Up),
            (UplinkId(1), 1.0, Duration::from_millis(40), LinkState::Up),
        ])
    }

    #[test]
    fn affinity_holds_while_the_uplink_is_healthy() {
        let mut t = FlowTable::with_defaults();
        let s = two_up();
        let now = Instant::now();
        let flows: Vec<FlowKey> = (0..50).map(|i| FlowKey::new(6, sa(i), sa(443))).collect();
        let first: Vec<Option<UplinkId>> =
            flows.iter().map(|f| t.place(f, FlowClass::Bulk, &s, now)).collect();
        // A second, *different* snapshot with the same healthy set must not move
        // anyone, even if the weights (and therefore the WRH winner) changed.
        let s2 = snapshot(vec![
            (UplinkId(0), 9.0, Duration::from_millis(10), LinkState::Up),
            (UplinkId(1), 1.0, Duration::from_millis(40), LinkState::Up),
        ]);
        let second: Vec<Option<UplinkId>> =
            flows.iter().map(|f| t.place(f, FlowClass::Bulk, &s2, now)).collect();
        assert_eq!(first, second, "no opportunistic migration (§6.2 rule 3)");
        assert_eq!(t.stats().affinity_hits, 50);
        assert_eq!(t.stats().rehashes, 0);
    }

    #[test]
    fn flows_rehash_only_when_their_uplink_leaves_the_healthy_set() {
        let mut t = FlowTable::with_defaults();
        let s = two_up();
        let now = Instant::now();
        let flows: Vec<FlowKey> = (0..80).map(|i| FlowKey::new(6, sa(i), sa(443))).collect();
        let first: Vec<Option<UplinkId>> =
            flows.iter().map(|f| t.place(f, FlowClass::Bulk, &s, now)).collect();
        assert!(first.iter().any(|p| *p == Some(UplinkId(1))));

        let s_down = snapshot(vec![
            (UplinkId(0), 1.0, Duration::from_millis(10), LinkState::Up),
            (UplinkId(1), 0.0, Duration::from_millis(40), LinkState::Down),
        ]);
        let second: Vec<Option<UplinkId>> =
            flows.iter().map(|f| t.place(f, FlowClass::Bulk, &s_down, now)).collect();
        for (b, a) in first.iter().zip(second.iter()) {
            assert_eq!(*a, Some(UplinkId(0)));
            if *b == Some(UplinkId(0)) { assert_eq!(b, a, "flows on the healthy link must not move"); }
        }
        assert!(t.stats().rehashes > 0);
    }

    #[test]
    fn idle_out_drops_the_entry() {
        let mut t = FlowTable::new(Duration::from_secs(120));
        let s = two_up();
        let now = Instant::now();
        let f = FlowKey::new(6, sa(1), sa(443));
        assert!(t.place(&f, FlowClass::Bulk, &s, now).is_some());
        assert_eq!(t.sweep(now + Duration::from_secs(119)), 0);
        assert_eq!(t.sweep(now + Duration::from_secs(121)), 1, "idles out after 120 s");
        assert!(t.is_empty());
        assert_eq!(t.stats().idle_out, 1);
    }

    #[test]
    fn latency_sensitive_follows_the_fastest_link() {
        let mut t = FlowTable::with_defaults();
        let now = Instant::now();
        let f = FlowKey::new(17, sa(1), sa(53));
        assert_eq!(t.place(&f, FlowClass::LatencySensitive, &two_up(), now), Some(UplinkId(0)));
        // fastest link changes -> the flow follows (documented exception)
        let s = snapshot(vec![
            (UplinkId(0), 1.0, Duration::from_millis(60), LinkState::Up),
            (UplinkId(1), 1.0, Duration::from_millis(8), LinkState::Up),
        ]);
        assert_eq!(t.place(&f, FlowClass::LatencySensitive, &s, now), Some(UplinkId(1)));
    }

    #[test]
    fn pinned_rule_change_moves_the_flow() {
        let mut t = FlowTable::with_defaults();
        let now = Instant::now();
        let f = FlowKey::new(6, sa(2), sa(443));
        assert_eq!(t.place(&f, FlowClass::Pinned(UplinkId(0)), &two_up(), now), Some(UplinkId(0)));
        assert_eq!(t.place(&f, FlowClass::Pinned(UplinkId(1)), &two_up(), now), Some(UplinkId(1)));
    }

    #[test]
    fn no_capacity_refuses_placement() {
        let mut t = FlowTable::with_defaults();
        let now = Instant::now();
        let dead = snapshot(vec![(UplinkId(0), 0.0, Duration::from_millis(10), LinkState::Down)]);
        let f = FlowKey::new(6, sa(3), sa(443));
        assert_eq!(t.place(&f, FlowClass::Bulk, &dead, now), None);
        assert_eq!(t.stats().refused, 1);
        assert!(t.is_empty(), "a refused flow keeps no stale affinity");
    }

    #[test]
    fn forget_uplink_clears_stale_affinity() {
        let mut t = FlowTable::with_defaults();
        let now = Instant::now();
        let s = two_up();
        let flows: Vec<FlowKey> = (0..40).map(|i| FlowKey::new(6, sa(i), sa(443))).collect();
        for f in &flows { t.place(f, FlowClass::Bulk, &s, now); }
        let dropped = t.forget_uplink(UplinkId(1));
        assert!(dropped > 0 && dropped < 40);
        assert_eq!(t.len(), 40 - dropped);
    }
}
