//! Bond auto-disable rule (blueprint Sec.6.4 rule 6).
//!
//! "If measured single-flow gain < proposed 15 % over best-uplink for a flow
//! class on current link mix, the scheduler reverts that class to flow-level
//! placement and records the decision (telemetry counter, opt-out)."
//!
//! This is the guard that keeps a 40 ms delay-gap pair (cellular vs Wi-Fi) from
//! paying a reordering tax for nothing (Sec.10.3 "single-flow on mismatched links"
//! row). The decision is measured, not assumed: it needs a full window of
//! samples before it reverts, and it re-arms when the link mix changes.

use std::collections::BTreeMap;

use crate::placement::FlowClass;

/// Proposed revert threshold (Sec.6.4 rule 6).
pub const PROPOSED_GAIN_THRESHOLD: f64 = 0.15;
/// Proposed samples required before a revert is allowed.
pub const PROPOSED_MIN_SAMPLES: u32 = 10;

/// The unit the rule is written against: a *flow class*, not a single flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BondClass {
    Bulk,
    LatencySensitive,
}

impl BondClass {
    /// Pinned flows are not bonded, so the rule does not apply to them.
    pub fn of(class: FlowClass) -> Option<Self> {
        match class {
            FlowClass::Bulk => Some(BondClass::Bulk),
            FlowClass::LatencySensitive => Some(BondClass::LatencySensitive),
            FlowClass::Pinned(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BondDecision {
    pub class: BondClass,
    pub enabled: bool,
    /// EWMA of (bonded goodput / best single uplink) - 1.
    pub gain: f64,
    /// True on the sample that flipped the decision (drives the telemetry event).
    pub flipped: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BondCounters {
    pub samples: u64,
    pub evaluations: u64,
    pub reverts: u64,
    pub rearms: u64,
}

#[derive(Debug, Clone, Copy)]
struct ClassState {
    gain_ewma: f64,
    samples: u32,
    enabled: bool,
    initialised: bool,
}

pub struct BondGainTracker {
    threshold: f64,
    min_samples: u32,
    alpha: f64,
    classes: BTreeMap<BondClass, ClassState>,
    counters: BondCounters,
}

impl Default for BondGainTracker {
    fn default() -> Self { Self::new(PROPOSED_GAIN_THRESHOLD, PROPOSED_MIN_SAMPLES) }
}

impl BondGainTracker {
    pub fn new(threshold: f64, min_samples: u32) -> Self {
        Self { threshold, min_samples, alpha: 0.3, classes: BTreeMap::new(), counters: BondCounters::default() }
    }

    pub fn threshold(&self) -> f64 { self.threshold }
    pub fn counters(&self) -> BondCounters { self.counters }
    /// Classes default to bonded (opt-out semantics, Sec.6.4 rule 6).
    pub fn enabled(&self, class: BondClass) -> bool {
        self.classes.get(&class).map(|c| c.enabled).unwrap_or(true)
    }
    pub fn gain(&self, class: BondClass) -> Option<f64> {
        self.classes.get(&class).filter(|c| c.initialised).map(|c| c.gain_ewma)
    }

    /// Feed one measurement window: `bonded_bps` is the class's goodput while
    /// striped across uplinks; `best_uplink_bps` is the best single uplink's
    /// goodput for the same window (Sec.10.2 baseline B1).
    pub fn observe(&mut self, class: BondClass, bonded_bps: f64, best_uplink_bps: f64) -> BondDecision {
        self.counters.samples += 1;
        let state = self.classes.entry(class).or_insert(ClassState {
            gain_ewma: 0.0,
            samples: 0,
            enabled: true,
            initialised: false,
        });
        if best_uplink_bps <= 0.0 {
            // No usable baseline: measure again, do not decide.
            return BondDecision { class, enabled: state.enabled, gain: state.gain_ewma, flipped: false };
        }
        let gain = bonded_bps / best_uplink_bps - 1.0;
        if state.initialised {
            state.gain_ewma = state.gain_ewma * (1.0 - self.alpha) + gain * self.alpha;
        } else {
            state.gain_ewma = gain;
            state.initialised = true;
        }
        state.samples = state.samples.saturating_add(1);

        let mut flipped = false;
        if state.enabled && state.samples >= self.min_samples && state.gain_ewma < self.threshold {
            state.enabled = false;
            self.counters.reverts += 1;
            flipped = true;
        }
        self.counters.evaluations += 1;
        BondDecision { class, enabled: state.enabled, gain: state.gain_ewma, flipped }
    }

    /// Re-arm every class after the uplink mix changes (an uplink added or
    /// removed): the old measurement no longer describes the new mix.
    pub fn reset_on_link_mix_change(&mut self) {
        for state in self.classes.values_mut() {
            state.enabled = true;
            state.samples = 0;
            state.initialised = false;
        }
        self.counters.rearms += 1;
    }

    /// Explicit user opt-in/opt-out overrides the measurement (Sec.6.4 rule 6).
    pub fn set_enabled(&mut self, class: BondClass, enabled: bool) {
        self.classes.entry(class).or_insert(ClassState { gain_ewma: 0.0, samples: 0, enabled, initialised: false }).enabled = enabled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_flows_are_not_bond_classes() {
        use crate::placement::UplinkId;
        assert_eq!(BondClass::of(FlowClass::Bulk), Some(BondClass::Bulk));
        assert_eq!(BondClass::of(FlowClass::LatencySensitive), Some(BondClass::LatencySensitive));
        assert_eq!(BondClass::of(FlowClass::Pinned(UplinkId(1))), None);
    }

    #[test]
    fn bonding_is_on_by_default() {
        let t = BondGainTracker::default();
        assert!(t.enabled(BondClass::Bulk));
        assert_eq!(t.gain(BondClass::Bulk), None, "nothing measured yet");
    }

    #[test]
    fn matched_links_keep_bonding() {
        let mut t = BondGainTracker::default();
        // 1.9x best uplink == +90 % gain: two matched links working as designed
        for _ in 0..20 {
            let d = t.observe(BondClass::Bulk, 190_000_000.0, 100_000_000.0);
            assert!(d.enabled);
            assert!(!d.flipped);
        }
        assert!(t.gain(BondClass::Bulk).unwrap() > 0.85);
        assert_eq!(t.counters().reverts, 0);
    }

    #[test]
    fn mismatched_pair_reverts_after_enough_samples() {
        let mut t = BondGainTracker::default();
        // 40 ms gap pair: reordering tax eats the gain (Sec.10.3 row 3)
        let mut flipped_at = None;
        for i in 0..30u32 {
            let d = t.observe(BondClass::Bulk, 102_000_000.0, 100_000_000.0);
            if d.flipped { flipped_at = Some(i); break; }
        }
        let at = flipped_at.expect("must revert");
        assert!(at >= 9, "no revert before the minimum sample window, flipped at {at}");
        assert!(!t.enabled(BondClass::Bulk));
        assert_eq!(t.counters().reverts, 1);
        // stays reverted
        let d = t.observe(BondClass::Bulk, 102_000_000.0, 100_000_000.0);
        assert!(!d.enabled && !d.flipped);
        assert_eq!(t.counters().reverts, 1, "revert is recorded once");
    }

    #[test]
    fn link_mix_change_rearms_the_class() {
        let mut t = BondGainTracker::default();
        for _ in 0..15 { t.observe(BondClass::Bulk, 100_000_000.0, 100_000_000.0); }
        assert!(!t.enabled(BondClass::Bulk));
        t.reset_on_link_mix_change();
        assert!(t.enabled(BondClass::Bulk), "new mix gets a fresh measurement window");
        assert_eq!(t.gain(BondClass::Bulk), None);
        assert_eq!(t.counters().rearms, 1);
    }

    #[test]
    fn missing_baseline_does_not_decide() {
        let mut t = BondGainTracker::default();
        for _ in 0..20 {
            let d = t.observe(BondClass::Bulk, 50_000_000.0, 0.0);
            assert!(d.enabled && !d.flipped);
        }
        assert_eq!(t.counters().reverts, 0);
    }

    #[test]
    fn classes_are_tracked_independently() {
        let mut t = BondGainTracker::default();
        for _ in 0..15 {
            t.observe(BondClass::Bulk, 100_000_000.0, 100_000_000.0);
            t.observe(BondClass::LatencySensitive, 190_000_000.0, 100_000_000.0);
        }
        assert!(!t.enabled(BondClass::Bulk));
        assert!(t.enabled(BondClass::LatencySensitive));
    }

    #[test]
    fn user_override_wins() {
        let mut t = BondGainTracker::default();
        for _ in 0..15 { t.observe(BondClass::Bulk, 190_000_000.0, 100_000_000.0); }
        assert!(t.enabled(BondClass::Bulk));
        t.set_enabled(BondClass::Bulk, false);
        assert!(!t.enabled(BondClass::Bulk));
        let d = t.observe(BondClass::Bulk, 190_000_000.0, 100_000_000.0);
        assert!(!d.enabled, "a good measurement must not silently re-enable a user opt-out");
    }
}
