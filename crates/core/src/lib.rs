//! Balancify control-plane core (first-party, per blueprint §4.8 / §6).
//!
//! Implements, sans-I/O and platform-independent:
//! - the path-health state machine with deliberate hysteresis (§6.5),
//! - link scoring (EWMA RTT/loss + capacity estimate) (§6.3),
//! - flow placement by weighted rendezvous hashing and latency-class override (§6.2).
//!
//! All numeric constants are the blueprint's *proposed* starting values (§6 preamble).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

// ---------------------------------------------------------------- health (§6.5)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Probing,
    Up,
    Degraded,
    Down,
    Recovering,
}

/// Proposed defaults (§6.5); tunable per uplink.
#[derive(Debug, Clone)]
pub struct HealthConfig {
    pub up_streak: u32,          // Probing -> Up            (proposed 5)
    pub down_misses: u32,       // Degraded -> Down          (proposed 3)
    pub recover_intervals: u32, // Recovering ramp intervals (proposed 3)
    pub degraded_loss: f64,     // loss ratio crossing into Degraded
    pub degraded_rtt_excess: Duration, // RTT above best-link excess crossing into Degraded
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            up_streak: 5,
            down_misses: 3,
            recover_intervals: 3,
            degraded_loss: 0.02,
            degraded_rtt_excess: Duration::from_millis(50),
        }
    }
}

/// Fast drop, slow return: failures move state immediately on streak expiry,
/// recovery ramps (§6.5 design intent).
#[derive(Debug, Clone)]
pub struct HealthMachine {
    cfg: HealthConfig,
    state: LinkState,
    ok_streak: u32,
    miss_streak: u32,
    ramp: u32,
}

impl HealthMachine {
    pub fn new(cfg: HealthConfig) -> Self {
        Self { cfg, state: LinkState::Probing, ok_streak: 0, miss_streak: 0, ramp: 0 }
    }
    pub fn state(&self) -> LinkState { self.state }

    pub fn probe_ok(&mut self) {
        self.ok_streak += 1;
        self.miss_streak = 0;
        match self.state {
            LinkState::Probing => {
                if self.ok_streak >= self.cfg.up_streak { self.state = LinkState::Up; }
            }
            LinkState::Down => { self.state = LinkState::Recovering; self.ramp = 0; }
            LinkState::Recovering => {
                self.ramp += 1;
                if self.ramp >= self.cfg.recover_intervals { self.state = LinkState::Up; self.ramp = 0; }
            }
            LinkState::Degraded => { self.state = LinkState::Up; }
            LinkState::Up => {}
        }
    }

    pub fn probe_miss(&mut self) {
        self.miss_streak += 1;
        self.ok_streak = 0;
        match self.state {
            LinkState::Degraded => {
                if self.miss_streak >= self.cfg.down_misses { self.state = LinkState::Down; }
            }
            LinkState::Up => { self.state = LinkState::Degraded; }
            LinkState::Recovering => { self.state = LinkState::Down; }
            LinkState::Probing | LinkState::Down => {}
        }
    }

    /// Quality-driven degradation while Up (loss / excess RTT vs best link).
    pub fn quality(&mut self, loss: f64, rtt_excess_over_best: Duration) {
        if self.state != LinkState::Up { return; }
        if loss > self.cfg.degraded_loss || rtt_excess_over_best > self.cfg.degraded_rtt_excess {
            self.state = LinkState::Degraded;
            self.miss_streak = 0;
        }
    }

    pub fn adapter_removed(&mut self) { self.state = LinkState::Down; self.ok_streak = 0; self.ramp = 0; }
    pub fn resume_from_sleep(&mut self) { self.state = LinkState::Probing; self.ok_streak = 0; self.miss_streak = 0; }

    /// Weight multiplier used by the scheduler (§6.3/§6.5): Down/Probing carry no traffic,
    /// Recovering ramps, Degraded is discounted.
    pub fn weight_factor(&self) -> f64 {
        match self.state {
            LinkState::Up => 1.0,
            LinkState::Degraded => 0.5,
            LinkState::Recovering => (self.ramp as f64 + 1.0) / (self.cfg.recover_intervals as f64 + 1.0),
            LinkState::Probing | LinkState::Down => 0.0,
        }
    }
}

// ---------------------------------------------------------------- scoring (§6.3)

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

// ---------------------------------------------------------------- placement (§6.2)

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UplinkId(pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowClass { Bulk, LatencySensitive, Pinned(UplinkId) }

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FlowKey {
    pub proto: u8,
    pub src: SocketAddr,
    pub dst: SocketAddr,
}

impl FlowKey {
    pub fn new(proto: u8, src: SocketAddr, dst: SocketAddr) -> Self { Self { proto, src, dst } }
    pub fn hash64(&self) -> u64 {
        let mut h = DefaultHasher::new();
        self.proto.hash(&mut h);
        self.src.hash(&mut h);
        self.dst.hash(&mut h);
        h.finish()
    }
}

/// Weighted rendezvous hashing: max over uplinks of weight * u(flow, uplink),
/// u uniform in (0,1]. Property: adding/removing an uplink moves only the flows
/// that were on it (§6.2).
pub fn wrh_place(flow: u64, uplinks: &[(UplinkId, f64)]) -> Option<UplinkId> {
    let mut best: Option<(f64, UplinkId)> = None;
    for (id, w) in uplinks {
        if *w <= 0.0 { continue; }
        let mut h = DefaultHasher::new();
        flow.hash(&mut h);
        id.hash(&mut h);
        let u01 = (h.finish() as f64) / (u64::MAX as f64);
        let u = u01.clamp(f64::MIN_POSITIVE, 1.0);
        // rendezvous score: w / -ln(u)  (equivalent monotone form of WRH)
        let s = *w / -u.ln();
        if best.map(|(bs, _)| s > bs).unwrap_or(true) { best = Some((s, *id)); }
    }
    best.map(|(_, id)| id)
}

/// Classifier entry point (§6.2): pinned > latency-sensitive (lowest delay) > WRH bulk.
pub fn place(
    flow: &FlowKey,
    class: FlowClass,
    uplinks: &[(UplinkId, f64, Duration, LinkState)], // (id, weight, rtt, state)
) -> Option<UplinkId> {
    match class {
        FlowClass::Pinned(id) => uplinks.iter().find(|u| u.0 == id && u.3 != LinkState::Down).map(|u| u.0),
        FlowClass::LatencySensitive => uplinks
            .iter()
            .filter(|u| u.3 == LinkState::Up || u.3 == LinkState::Degraded)
            .min_by_key(|u| u.2)
            .map(|u| u.0),
        FlowClass::Bulk => {
            // Unhealthy uplinks carry zero weight: WRH then moves only the
            // flows that were on a link when it leaves the healthy set (§6.2).
            let w: Vec<(UplinkId, f64)> = uplinks
                .iter()
                .map(|u| (u.0, if matches!(u.3, LinkState::Down | LinkState::Probing) { 0.0 } else { u.1 }))
                .collect();
            wrh_place(flow.hash64(), &w)
        }
    }
}

fn _assert_ip_used(_i: IpAddr) {} // keep IpAddr import honest for future 5-tuple widening

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    fn sa(p: u16) -> SocketAddr { SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), p)) }

    #[test]
    fn health_hysteresis_fast_drop_slow_return() {
        let mut m = HealthMachine::new(HealthConfig::default());
        assert_eq!(m.state(), LinkState::Probing);
        for _ in 0..4 { m.probe_ok(); }
        assert_eq!(m.state(), LinkState::Probing); // 5th needed
        m.probe_ok();
        assert_eq!(m.state(), LinkState::Up);
        // one miss degrades immediately (fast drop)
        m.probe_miss();
        assert_eq!(m.state(), LinkState::Degraded);
        m.probe_miss(); m.probe_miss();
        assert_eq!(m.state(), LinkState::Down);   // 3 misses total
        // slow return: one ok -> Recovering, then ramp intervals -> Up
        m.probe_ok();
        assert_eq!(m.state(), LinkState::Recovering);
        assert!(m.weight_factor() > 0.0 && m.weight_factor() < 1.0);
        m.probe_ok(); m.probe_ok(); m.probe_ok();
        assert_eq!(m.state(), LinkState::Up);
    }

    #[test]
    fn health_quality_degrades_up_link() {
        let mut m = HealthMachine::new(HealthConfig::default());
        for _ in 0..5 { m.probe_ok(); }
        m.quality(0.05, Duration::from_millis(1));
        assert_eq!(m.state(), LinkState::Degraded);
    }

    #[test]
    fn wrh_only_moves_flows_of_removed_uplink() {
        let ups = vec![(UplinkId(0), 1.0), (UplinkId(1), 1.0), (UplinkId(2), 1.0)];
        let flows: Vec<u64> = (0..300).map(|i| FlowKey::new(6, sa(i), sa(443)).hash64()).collect();
        let before: Vec<Option<UplinkId>> = flows.iter().map(|f| wrh_place(*f, &ups)).collect();
        let after_ups = vec![(UplinkId(0), 1.0), (UplinkId(2), 1.0)]; // uplink 1 removed
        let after: Vec<Option<UplinkId>> = flows.iter().map(|f| wrh_place(*f, &after_ups)).collect();
        for (b, a) in before.iter().zip(after.iter()) {
            if b.as_ref() != Some(&UplinkId(1)) { assert_eq!(b, a, "flow not on removed link must not move"); }
        }
        // and flows on the removed link did move
        assert!(before.iter().zip(after.iter()).any(|(b, a)| b.as_ref() == Some(&UplinkId(1)) && b != a));
    }

    #[test]
    fn wrh_respects_weights_and_zero_weight() {
        let ups = vec![(UplinkId(0), 3.0), (UplinkId(1), 1.0), (UplinkId(2), 0.0)];
        let mut c = [0u32; 3];
        for i in 0..1000u16 {
            let f = FlowKey::new(17, sa(i), sa(53)).hash64();
            let id = wrh_place(f, &ups).unwrap();
            c[id.0 as usize] += 1;
        }
        assert_eq!(c[2], 0, "zero-weight uplink must never receive flows");
        assert!(c[0] > c[1] * 2, "3:1 weights should skew strongly, got {:?})", c);
    }

    #[test]
    fn latency_sensitive_picks_lowest_delay_healthy() {
        let ups = vec![
            (UplinkId(0), 9.0, Duration::from_millis(40), LinkState::Up),
            (UplinkId(1), 1.0, Duration::from_millis(12), LinkState::Up),
            (UplinkId(2), 5.0, Duration::from_millis(5), LinkState::Down),
        ];
        let f = FlowKey::new(17, sa(1), sa(53));
        assert_eq!(place(&f, FlowClass::LatencySensitive, &ups), Some(UplinkId(1)));
        assert_eq!(place(&f, FlowClass::Pinned(UplinkId(2)), &ups), None, "pinned to Down link => no placement");
        let b = place(&f, FlowClass::Bulk, &ups);
        assert!(b == Some(UplinkId(0)) || b == Some(UplinkId(1)));
    }

    #[test]
    fn score_penalties_monotonic() {
        let best = Duration::from_millis(10);
        let a = LinkScore::new(best, 0.0, 100_000_000);
        let b = LinkScore::new(Duration::from_millis(60), 0.0, 100_000_000);
        let c = LinkScore::new(best, 0.1, 100_000_000);
        assert!(a.score(best) > b.score(best));
        assert!(a.score(best) > c.score(best));
    }
}
