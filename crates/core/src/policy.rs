//! Control-plane policy source (blueprint §6.1).
//!
//! One immutable [`PolicySnapshot`] feeds **both** schedulers: the flow
//! scheduler (§6.2) and the striping scheduler (§6.4). Nothing else in the
//! product reads link state directly — that is what keeps the two schedulers
//! from disagreeing during a flap.
//!
//! The engine owns, per uplink: the health state machine (§6.5), the link score
//! (§6.3) and the capacity estimator (§6.3). It is sans-I/O: probes, ACKs and
//! clock values are pushed in by the caller.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::health::{HealthConfig, HealthMachine, LinkState};
use crate::placement::{place, FlowClass, FlowKey, UplinkId};
use crate::scoring::{CapacityEstimator, LinkScore};

/// EWMA alpha for probe RTT/loss (§6.3 proposed value).
pub const EWMA_ALPHA: f64 = 0.2;
/// Striping quantum window: bytes per millisecond of estimated capacity
/// (§6.4 rule 2, proposed), floored at one tunnel MTU.
pub const QUANTUM_WINDOW: Duration = Duration::from_millis(1);
/// Proposed tunnel MTU used as the DRR quantum floor (§6.6).
pub const TUNNEL_MTU: u64 = 1400;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UplinkView {
    pub id: UplinkId,
    pub state: LinkState,
    /// Health-discounted score; 0.0 means "carries no traffic".
    pub weight: f64,
    pub rtt: Duration,
    pub loss: f64,
    pub capacity_bps: u64,
    pub metered: bool,
    /// False when excluded (Down/Probing, or metered and not user-approved §6.6).
    pub eligible: bool,
}

/// Immutable, revisioned snapshot published by the control plane (§6.1).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PolicySnapshot {
    pub revision: u64,
    /// Per-boot hash seed (§6.2 rule 4) — logged in the support bundle.
    pub seed: u64,
    pub best_rtt: Duration,
    pub uplinks: Vec<UplinkView>,
}

impl PolicySnapshot {
    pub fn eligible(&self) -> impl Iterator<Item = &UplinkView> + '_ {
        self.uplinks.iter().filter(|u| u.eligible)
    }
    pub fn has_capacity(&self) -> bool { self.eligible().next().is_some() }
    pub fn view(&self, id: UplinkId) -> Option<&UplinkView> { self.uplinks.iter().find(|u| u.id == id) }
    pub fn is_healthy(&self, id: UplinkId) -> bool { self.view(id).map(|u| u.eligible).unwrap_or(false) }

    /// Flow-scheduler view: rows for [`place`].
    ///
    /// An excluded uplink is reported as `Down` here whatever its health state
    /// says, so *every* placement class (bulk, latency-sensitive, pinned)
    /// refuses it — zeroing the weight alone would only stop the WRH path and
    /// a metered link would still win the lowest-delay contest (§6.6). The
    /// truthful state stays on [`UplinkView`] for the GUI.
    pub fn rows(&self) -> Vec<(UplinkId, f64, Duration, LinkState)> {
        self.uplinks
            .iter()
            .map(|u| {
                let effective = if u.eligible { u.state } else { LinkState::Down };
                (u.id, if u.eligible { u.weight } else { 0.0 }, u.rtt, effective)
            })
            .collect()
    }

    pub fn place(&self, flow: &FlowKey, class: FlowClass) -> Option<UplinkId> {
        place(flow, class, &self.rows())
    }

    /// Striping-scheduler view: DRR quanta proportional to capacity (§6.4 rule 2).
    pub fn quanta(&self) -> Vec<(u8, u64)> {
        self.eligible()
            .map(|u| {
                let bytes_per_ms = u.capacity_bps / 8 / 1000;
                (u.id.0, bytes_per_ms.max(TUNNEL_MTU))
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct PolicyConfig {
    pub health: HealthConfig,
    /// §6.6: metered links are excluded unless the user allows bonding over them.
    pub allow_metered: bool,
}

impl Default for PolicyConfig {
    fn default() -> Self { Self { health: HealthConfig::default(), allow_metered: false } }
}

#[derive(Debug, Clone)]
pub struct UplinkConfig {
    pub capacity_bps: u64,
    pub metered: bool,
    pub initial_rtt: Duration,
}

impl UplinkConfig {
    pub fn new(capacity_bps: u64) -> Self {
        Self { capacity_bps, metered: false, initial_rtt: Duration::from_millis(30) }
    }
}

struct UplinkRuntime {
    health: HealthMachine,
    score: LinkScore,
    capacity: CapacityEstimator,
    metered: bool,
}

/// The policy source. Deterministic: uplinks are kept in a `BTreeMap` so
/// snapshots do not depend on insertion order.
pub struct PolicyEngine {
    cfg: PolicyConfig,
    uplinks: BTreeMap<UplinkId, UplinkRuntime>,
    seed: u64,
    revision: u64,
    published: PolicySnapshot,
}

impl PolicyEngine {
    /// `seed` is the per-boot hash seed (§6.2 rule 4); pass a fresh random
    /// value at service start and log it in the support bundle.
    pub fn new(cfg: PolicyConfig, seed: u64) -> Self {
        Self { cfg, uplinks: BTreeMap::new(), seed, revision: 0, published: PolicySnapshot { seed, ..Default::default() } }
    }

    pub fn seed(&self) -> u64 { self.seed }
    pub fn config(&self) -> &PolicyConfig { &self.cfg }
    pub fn set_allow_metered(&mut self, allow: bool) { self.cfg.allow_metered = allow; }
    /// Last published snapshot (revision only advances when content changes).
    pub fn published(&self) -> &PolicySnapshot { &self.published }

    pub fn add_uplink(&mut self, id: UplinkId, cfg: UplinkConfig, now: Instant) {
        self.uplinks.insert(
            id,
            UplinkRuntime {
                health: HealthMachine::new(self.cfg.health.clone()),
                score: LinkScore::new(cfg.initial_rtt, 0.0, cfg.capacity_bps),
                capacity: CapacityEstimator::new(cfg.capacity_bps, now),
                metered: cfg.metered,
            },
        );
    }

    /// Adapter removal (§4.5): immediate Down, flows rehash on the next publish.
    pub fn remove_uplink(&mut self, id: UplinkId) {
        if let Some(u) = self.uplinks.get_mut(&id) { u.health.adapter_removed(); }
    }
    pub fn drop_uplink(&mut self, id: UplinkId) -> bool { self.uplinks.remove(&id).is_some() }

    pub fn set_metered(&mut self, id: UplinkId, metered: bool) {
        if let Some(u) = self.uplinks.get_mut(&id) { u.metered = metered; }
    }

    /// Sleep/resume (§4.5): every uplink re-probes before carrying traffic.
    pub fn resume_from_sleep(&mut self) {
        for u in self.uplinks.values_mut() { u.health.resume_from_sleep(); }
    }

    pub fn on_probe_ok(&mut self, id: UplinkId, rtt: Duration, now: Instant) {
        let Some(u) = self.uplinks.get_mut(&id) else { return; };
        u.health.probe_ok();
        u.score.observe(rtt, false, EWMA_ALPHA);
        let _ = now;
    }

    pub fn on_probe_miss(&mut self, id: UplinkId) {
        let Some(u) = self.uplinks.get_mut(&id) else { return; };
        u.health.probe_miss();
        u.score.observe(u.score.rtt_ewma, true, EWMA_ALPHA);
    }

    /// Passive learning from tunnel ACKs (§6.3a, §6.5 "passive ACK learning").
    pub fn on_ack(&mut self, id: UplinkId, bytes: u64, now: Instant) {
        if let Some(u) = self.uplinks.get_mut(&id) { u.capacity.on_ack(bytes, now); }
    }

    /// Bounded idle probe result (§6.3b).
    pub fn on_idle_probe(&mut self, id: UplinkId, bytes: u64, rtt: Duration, now: Instant) {
        if let Some(u) = self.uplinks.get_mut(&id) {
            u.capacity.on_probe_result(bytes, rtt, now);
            u.score.observe(rtt, false, EWMA_ALPHA);
        }
    }

    /// Restart every uplink's ACK-pacing window at `now` (§4.5 sleep/resume,
    /// §6.6 re-binding). See [`CapacityEstimator::restart_window`].
    pub fn restart_measurements(&mut self, now: Instant) {
        for u in self.uplinks.values_mut() { u.capacity.restart_window(now); }
    }

    /// Record that this uplink carried traffic, so it is measured by ACK
    /// pacing instead of being probed (§6.3a/b).
    pub fn note_traffic(&mut self, id: UplinkId, now: Instant) {
        if let Some(u) = self.uplinks.get_mut(&id) { u.capacity.note_traffic(now); }
    }

    /// Flow-level lane (§6.2): count flow packets for capacity even without ACK clock.
    pub fn on_flow_packet(&mut self, id: UplinkId, bytes: u64, now: Instant) {
        if let Some(u) = self.uplinks.get_mut(&id) { u.capacity.on_flow_packet(bytes, now); }
    }

    /// Ask whether an idle capacity probe may be sent, and how large (§6.3
    /// bounded probing). Returns `Some((uplink, bytes))`.
    pub fn request_idle_probe(&mut self, want_bytes: u64, now: Instant) -> Option<(UplinkId, u64)> {
        let ids: Vec<UplinkId> = self.uplinks.keys().copied().collect();
        for id in ids {
            let u = self.uplinks.get_mut(&id)?;
            if let Some(bytes) = u.capacity.request_idle_probe(want_bytes, now) { return Some((id, bytes)); }
        }
        None
    }

    /// Build and (if it changed) publish the snapshot. The revision advances
    /// only on a real change, so the shell's 1 Hz subscription stays quiet.
    pub fn publish(&mut self, now: Instant) -> PolicySnapshot {
        let best_rtt = self.best_rtt();
        let uplinks: Vec<UplinkView> = self
            .uplinks
            .iter()
            .map(|(id, u)| {
                let excluded = u.metered && !self.cfg.allow_metered;
                let factor = u.health.weight_factor();
                // Score against the *live* capacity estimate, not the last value
                // a probe happened to copy in: ACK pacing moves capacity between
                // probes (§6.3a) and that must reach the weights immediately.
                let mut scored = u.score.clone();
                scored.capacity_bps = u.capacity.capacity_bps();
                let score = scored.score(best_rtt);
                let weight = if excluded { 0.0 } else { score * factor };
                UplinkView {
                    id: *id,
                    state: u.health.state(),
                    weight,
                    rtt: u.score.rtt_ewma,
                    loss: u.score.loss_ewma,
                    capacity_bps: u.capacity.capacity_bps(),
                    metered: u.metered,
                    eligible: weight > 0.0,
                }
            })
            .collect();

        let candidate = PolicySnapshot { revision: 0, seed: self.seed, best_rtt, uplinks };
        if same_content(&self.published, &candidate) {
            return self.published.clone();
        }
        self.revision += 1;
        let published = PolicySnapshot { revision: self.revision, ..candidate };
        self.published = published.clone();
        let _ = now;
        published
    }

    /// Quality-driven degradation needs the current best RTT (§6.5 threshold is
    /// "excess over best link"); call after probing a round.
    pub fn apply_quality(&mut self, best_rtt: Duration) {
        for u in self.uplinks.values_mut() {
            let excess = u.score.rtt_ewma.saturating_sub(best_rtt);
            u.health.quality(u.score.loss_ewma, excess);
        }
    }

    fn best_rtt(&self) -> Duration {
        self.uplinks
            .values()
            .filter(|u| matches!(u.health.state(), LinkState::Up | LinkState::Degraded))
            .map(|u| u.score.rtt_ewma)
            .min()
            .unwrap_or(Duration::ZERO)
    }
}

fn same_content(a: &PolicySnapshot, b: &PolicySnapshot) -> bool {
    a.seed == b.seed && a.best_rtt == b.best_rtt && a.uplinks == b.uplinks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::sa;

    fn engine() -> PolicyEngine {
        let mut e = PolicyEngine::new(PolicyConfig::default(), 0xABCD);
        let t0 = Instant::now();
        e.add_uplink(UplinkId(0), UplinkConfig::new(100_000_000), t0);
        e.add_uplink(UplinkId(1), UplinkConfig::new(50_000_000), t0);
        e
    }

    fn bring_up(e: &mut PolicyEngine, id: UplinkId, rtt: Duration) {
        let t0 = Instant::now();
        for _ in 0..5 { e.on_probe_ok(id, rtt, t0); }
    }

    #[test]
    fn nothing_is_eligible_until_probes_succeed() {
        let mut e = engine();
        let s = e.publish(Instant::now());
        assert!(!s.has_capacity(), "Probing uplinks carry no traffic (§6.5)");
        assert!(s.place(&FlowKey::new(6, sa(1), sa(443)), FlowClass::Bulk).is_none());
    }

    #[test]
    fn snapshot_places_flows_and_exposes_quanta() {
        let mut e = engine();
        bring_up(&mut e, UplinkId(0), Duration::from_millis(10));
        bring_up(&mut e, UplinkId(1), Duration::from_millis(30));
        let s = e.publish(Instant::now());
        assert!(s.has_capacity());
        let f = FlowKey::new(6, sa(1), sa(443));
        assert!(s.place(&f, FlowClass::Bulk).is_some());
        assert_eq!(s.place(&f, FlowClass::LatencySensitive), Some(UplinkId(0)), "lowest RTT wins");
        let q = s.quanta();
        assert_eq!(q.len(), 2);
        assert!(q[0].1 > q[1].1, "quanta proportional to capacity: {q:?}");
        assert!(q.iter().all(|(_, v)| *v >= TUNNEL_MTU));
    }

    #[test]
    fn revision_only_moves_on_real_change() {
        let mut e = engine();
        bring_up(&mut e, UplinkId(0), Duration::from_millis(10));
        let r1 = e.publish(Instant::now()).revision;
        assert!(r1 > 0);
        let r2 = e.publish(Instant::now()).revision;
        assert_eq!(r1, r2, "identical content must not churn the revision");
        bring_up(&mut e, UplinkId(1), Duration::from_millis(30));
        let r3 = e.publish(Instant::now()).revision;
        assert!(r3 > r2, "a newly Up uplink is a real change");
    }

    #[test]
    fn failed_uplink_loses_flows_but_others_keep_theirs() {
        let mut e = engine();
        bring_up(&mut e, UplinkId(0), Duration::from_millis(10));
        bring_up(&mut e, UplinkId(1), Duration::from_millis(30));
        let s0 = e.publish(Instant::now());
        let flows: Vec<FlowKey> = (0..200).map(|i| FlowKey::new(6, sa(i), sa(443))).collect();
        let before: Vec<Option<UplinkId>> = flows.iter().map(|f| s0.place(f, FlowClass::Bulk)).collect();
        assert!(before.iter().any(|p| *p == Some(UplinkId(1))), "both links should be used");

        // uplink 1 fails: one miss degrades, three misses drop it (§6.5)
        for _ in 0..3 { e.on_probe_miss(UplinkId(1)); }
        let s1 = e.publish(Instant::now());
        assert_eq!(s1.view(UplinkId(1)).unwrap().state, LinkState::Down);
        assert!(!s1.is_healthy(UplinkId(1)));
        let after: Vec<Option<UplinkId>> = flows.iter().map(|f| s1.place(f, FlowClass::Bulk)).collect();
        for (b, a) in before.iter().zip(after.iter()) {
            if *b != Some(UplinkId(1)) { assert_eq!(b, a, "WRH must not move unrelated flows"); }
            assert_eq!(*a, Some(UplinkId(0)), "everything lands on the surviving link");
        }
    }

    #[test]
    fn metered_uplink_excluded_until_user_allows() {
        let mut e = engine();
        e.set_metered(UplinkId(1), true);
        bring_up(&mut e, UplinkId(0), Duration::from_millis(30));
        bring_up(&mut e, UplinkId(1), Duration::from_millis(12));
        let s = e.publish(Instant::now());
        assert!(!s.is_healthy(UplinkId(1)), "metered excluded by default (§6.6)");
        let dns = FlowKey::new(17, sa(1), sa(53));
        assert_eq!(s.place(&dns, FlowClass::LatencySensitive), Some(UplinkId(0)), "the excluded link must not win on delay");
        assert_eq!(s.place(&dns, FlowClass::Pinned(UplinkId(1))), None, "pinned to an excluded link => no placement");
        assert_eq!(s.view(UplinkId(1)).unwrap().state, LinkState::Up, "the GUI still sees the truthful state");
        e.set_allow_metered(true);
        let s = e.publish(Instant::now());
        assert!(s.is_healthy(UplinkId(1)));
        assert_eq!(s.place(&dns, FlowClass::LatencySensitive), Some(UplinkId(1)));
    }

    #[test]
    fn ack_pacing_lifts_capacity_and_therefore_weight() {
        let mut e = engine();
        bring_up(&mut e, UplinkId(0), Duration::from_millis(10));
        bring_up(&mut e, UplinkId(1), Duration::from_millis(10));
        let w_before = e.publish(Instant::now()).view(UplinkId(1)).unwrap().weight;
        // uplink 1 is loaded: 4 MiB acknowledged every 100 ms => ~335 Mbit/s
        let t0 = Instant::now();
        for i in 1..=6 { e.on_ack(UplinkId(1), 4 * 1024 * 1024, t0 + Duration::from_millis(100 * i)); }
        let s = e.publish(Instant::now());
        let cap1 = s.view(UplinkId(1)).unwrap().capacity_bps;
        let w1 = s.view(UplinkId(1)).unwrap().weight;
        // one window of 20 MiB/500 ms == 335 Mbit/s, EWMA'd against the 50 Mbit/s seed
        assert!(cap1 > 120_000_000, "measured capacity should far exceed the 50 Mbit/s seed, got {cap1}");
        assert!(w1 > w_before, "weight must follow the measured capacity ({w1} vs {w_before})");
        assert!(w1 > s.view(UplinkId(0)).unwrap().weight, "loaded link outweighs the idle seed");
    }

    #[test]
    fn idle_probe_budget_is_enforced_per_uplink() {
        let mut e = engine();
        let t0 = Instant::now();
        let (id, bytes) = e.request_idle_probe(64 * 1024, t0).expect("first probe allowed");
        assert_eq!(id, UplinkId(0));
        assert_eq!(bytes, 64 * 1024);
        // same uplink is now out of budget; the engine tries the next one
        let next = e.request_idle_probe(64 * 1024, t0);
        assert_eq!(next.map(|(i, _)| i), Some(UplinkId(1)));
        assert!(e.request_idle_probe(64 * 1024, t0).is_none(), "all budgets spent");
    }

    #[test]
    fn sleep_resume_reprobes_everything() {
        let mut e = engine();
        bring_up(&mut e, UplinkId(0), Duration::from_millis(10));
        bring_up(&mut e, UplinkId(1), Duration::from_millis(30));
        assert!(e.publish(Instant::now()).has_capacity());
        e.resume_from_sleep();
        let s = e.publish(Instant::now());
        assert!(!s.has_capacity(), "no traffic until probes complete (§4.5)");
    }

    #[test]
    fn quality_degrades_a_lossy_link() {
        let mut e = engine();
        bring_up(&mut e, UplinkId(0), Duration::from_millis(10));
        bring_up(&mut e, UplinkId(1), Duration::from_millis(10));
        for _ in 0..12 { e.on_probe_miss(UplinkId(0)); e.on_probe_ok(UplinkId(0), Duration::from_millis(10), Instant::now()); }
        e.apply_quality(Duration::from_millis(10));
        let s = e.publish(Instant::now());
        assert_eq!(s.view(UplinkId(0)).unwrap().state, LinkState::Degraded);
        assert!(s.view(UplinkId(0)).unwrap().weight < s.view(UplinkId(1)).unwrap().weight);
    }
}
