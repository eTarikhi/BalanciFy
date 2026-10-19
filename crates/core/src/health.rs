//! Path-health state machine with deliberate hysteresis (blueprint §6.5).
//!
//! Design intent: **fast drop, slow return** — a flapping cellular link must
//! not oscillate flows between uplinks.

use std::time::Duration;

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
    pub up_streak: u32,                 // Probing -> Up            (proposed 5)
    pub down_misses: u32,               // Degraded -> Down         (proposed 3)
    pub recover_intervals: u32,         // Recovering ramp intervals (proposed 3)
    pub degraded_loss: f64,             // loss ratio crossing into Degraded
    pub degraded_rtt_excess: Duration,  // RTT above best-link excess crossing into Degraded
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn resume_from_sleep_reprobes_before_carrying_traffic() {
        let mut m = HealthMachine::new(HealthConfig::default());
        for _ in 0..5 { m.probe_ok(); }
        assert_eq!(m.state(), LinkState::Up);
        m.resume_from_sleep();
        assert_eq!(m.state(), LinkState::Probing);
        assert_eq!(m.weight_factor(), 0.0, "no traffic until the probe streak completes");
    }

    #[test]
    fn adapter_removed_drops_immediately() {
        let mut m = HealthMachine::new(HealthConfig::default());
        for _ in 0..5 { m.probe_ok(); }
        m.adapter_removed();
        assert_eq!(m.state(), LinkState::Down);
        assert_eq!(m.weight_factor(), 0.0);
    }
}
