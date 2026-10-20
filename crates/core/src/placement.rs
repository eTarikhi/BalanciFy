//! Flow placement: weighted rendezvous hashing with class overrides (blueprint Sec.6.2).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;
use std::time::Duration;

use crate::health::LinkState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UplinkId(pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowClass {
    Bulk,
    LatencySensitive,
    Pinned(UplinkId),
}

/// A flow's 5-tuple identity. `Ord` is derived so the flow table and any
/// telemetry that lists flows iterate deterministically.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
/// that were on it (Sec.6.2).
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

/// Classifier entry point (Sec.6.2): pinned > latency-sensitive (lowest delay) > WRH bulk.
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
            // flows that were on a link when it leaves the healthy set (Sec.6.2).
            let w: Vec<(UplinkId, f64)> = uplinks
                .iter()
                .map(|u| (u.0, if matches!(u.3, LinkState::Down | LinkState::Probing) { 0.0 } else { u.1 }))
                .collect();
            wrh_place(flow.hash64(), &w)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    pub(crate) fn sa(p: u16) -> SocketAddr { SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), p)) }

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
}
