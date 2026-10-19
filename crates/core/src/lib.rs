//! Balancify control-plane core (first-party, per blueprint §4.8 / §6).
//!
//! Implements, sans-I/O and platform-independent:
//! - the path-health state machine with deliberate hysteresis (§6.5),
//! - link scoring and capacity estimation (§6.3),
//! - per-path congestion windows, coupled across paths (§6.4 rule 4),
//! - flow placement by weighted rendezvous hashing and latency-class override (§6.2),
//! - classification rules (§6.2 step 1) and the flow table with affinity (§6.2 rule 3),
//! - the policy source that both schedulers read (§6.1),
//! - the bond auto-disable rule (§6.4 rule 6),
//! - MTU/MSS derivation and per-path keepalives (§6.6).
//!
//! All numeric constants are the blueprint's *proposed* starting values (§6
//! preamble) — they are lab-tunable, not measurements.
//!
//! # Sans-I/O
//! Nothing in this crate opens a socket, reads a clock of its own accord, or
//! spawns a task. Callers push observations (`on_probe_ok`, `on_ack`, …) and a
//! monotonic `now`; the crate returns decisions. That keeps the whole control
//! plane unit-testable off Windows and fuzzable in CI (§4.7.4).

pub mod bond;
pub mod congestion;
pub mod flowtable;
pub mod health;
pub mod keepalive;
pub mod mtu;
pub mod pacing;
pub mod placement;
pub mod policy;
pub mod ratelimit;
pub mod rules;
pub mod scoring;

pub use bond::{BondClass, BondCounters, BondDecision, BondGainTracker};
pub use congestion::{
    CongestionController, INIT_CWND_SEGMENTS, MIN_CWND_SEGMENTS, MIN_HALVE_INTERVAL,
};
pub use pacing::{Pacer, PacingController};
pub use ratelimit::{RateLimiter, TokenBucket};
pub use flowtable::{FlowStats, FlowTable, DEFAULT_IDLE_TIMEOUT};
pub use health::{HealthConfig, HealthMachine, LinkState};
pub use keepalive::{
    path_changed, KeepaliveScheduler, PathObservation, DEFAULT_KEEPALIVE_INTERVAL,
    MAX_KEEPALIVE_INTERVAL,
};
pub use mtu::{MtuPolicy, IPV4_MIN_MTU, MSS_OVERHEAD_V4, MSS_OVERHEAD_V6};
pub use placement::{place, wrh_place, FlowClass, FlowKey, UplinkId};
pub use policy::{
    PolicyConfig, PolicyEngine, PolicySnapshot, UplinkConfig, UplinkView, EWMA_ALPHA, TUNNEL_MTU,
};
pub use rules::{PortRange, Rule, RuleSet};
pub use scoring::{CapacityEstimator, LinkScore};

/// Crate version surfaced through the IPC contract (§9.2: schema versioned).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
pub(crate) mod testutil {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    /// Deterministic source address for tests: 10.0.0.1:`p`.
    pub(crate) fn sa(p: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), p))
    }

    /// Deterministic peer address for tests: 203.0.113.9:`p` (TEST-NET-3).
    pub(crate) fn peer(p: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 9), p))
    }
}

#[cfg(test)]
mod integration {
    //! Cross-module checks: the pieces must agree, not just work in isolation.

    use super::*;
    use std::time::{Duration, Instant};

    fn up(e: &mut PolicyEngine, id: UplinkId, rtt: Duration) {
        let now = Instant::now();
        for _ in 0..5 {
            e.on_probe_ok(id, rtt, now);
        }
    }

    #[test]
    fn classified_flow_travels_rule_to_table_to_snapshot() {
        let now = Instant::now();
        let mut e = PolicyEngine::new(PolicyConfig::default(), 0x5EED);
        e.add_uplink(UplinkId(0), UplinkConfig::new(200_000_000), now);
        e.add_uplink(UplinkId(1), UplinkConfig::new(60_000_000), now);
        up(&mut e, UplinkId(0), Duration::from_millis(35));
        up(&mut e, UplinkId(1), Duration::from_millis(9));
        let snap = e.publish(now);

        let rules = RuleSet::curated();
        let mut table = FlowTable::with_defaults();

        let dns = FlowKey::new(17, testutil::sa(5000), testutil::peer(53));
        let tls = FlowKey::new(6, testutil::sa(5001), testutil::peer(443));
        assert_eq!(rules.classify(&dns, None), FlowClass::LatencySensitive);
        assert_eq!(rules.classify(&tls, None), FlowClass::Bulk);

        assert_eq!(table.place(&dns, rules.classify(&dns, None), &snap, now), Some(UplinkId(1)));
        let bulk = table.place(&tls, rules.classify(&tls, None), &snap, now);
        assert!(matches!(bulk, Some(UplinkId(0)) | Some(UplinkId(1))));
        // and the affinity holds on the next packet
        assert_eq!(table.place(&tls, rules.classify(&tls, None), &snap, now), bulk);
    }

    #[test]
    fn striping_quanta_come_from_the_same_snapshot_as_placement() {
        let now = Instant::now();
        let mut e = PolicyEngine::new(PolicyConfig::default(), 1);
        e.add_uplink(UplinkId(0), UplinkConfig::new(200_000_000), now);
        e.add_uplink(UplinkId(1), UplinkConfig::new(50_000_000), now);
        up(&mut e, UplinkId(0), Duration::from_millis(10));
        up(&mut e, UplinkId(1), Duration::from_millis(10));
        let snap = e.publish(now);

        let q = snap.quanta();
        assert_eq!(q.len(), 2);
        let ratio = q[0].1 as f64 / q[1].1 as f64;
        assert!(ratio > 3.0, "4:1 capacity should give ~4:1 quanta, got {ratio}");
        // the same snapshot must also be usable for placement
        assert!(snap.place(&FlowKey::new(6, testutil::sa(1), testutil::peer(443)), FlowClass::Bulk).is_some());
    }

    #[test]
    fn mtu_follows_path_loss_while_flows_keep_moving() {
        let now = Instant::now();
        let policy = MtuPolicy::new(60);
        assert_eq!(policy.adapter_mtu(&[1500, 1430]), Some(1370));
        assert_eq!(policy.adapter_mtu(&[1500]), Some(1440));

        let mut e = PolicyEngine::new(PolicyConfig::default(), 2);
        e.add_uplink(UplinkId(0), UplinkConfig::new(100_000_000), now);
        e.add_uplink(UplinkId(1), UplinkConfig::new(100_000_000), now);
        up(&mut e, UplinkId(0), Duration::from_millis(10));
        up(&mut e, UplinkId(1), Duration::from_millis(10));
        let mut table = FlowTable::with_defaults();
        let snap = e.publish(now);
        let flows: Vec<FlowKey> = (0..60).map(|i| FlowKey::new(6, testutil::sa(6000 + i), testutil::peer(443))).collect();
        for f in &flows { assert!(table.place(f, FlowClass::Bulk, &snap, now).is_some()); }

        // one adapter goes away: MTU widens, flows re-place, table shrinks
        e.remove_uplink(UplinkId(1));
        let snap = e.publish(now);
        assert!(!snap.is_healthy(UplinkId(1)));
        assert!(table.forget_uplink(UplinkId(1)) > 0);
        for f in &flows {
            assert_eq!(table.place(f, FlowClass::Bulk, &snap, now), Some(UplinkId(0)));
        }
    }
}
