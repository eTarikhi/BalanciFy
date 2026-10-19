//! Support bundle generation (§8.2 Diagnostics, §9.2 redaction)
//!
//! One-click bundle: logs (ETW export), route table, adapter state, version/signing info, redaction applied.
//! Redaction: no destinations, no content (§9.2) — only counters, error codes, and hashed flow keys.

use crate::{Service, Counters};
use std::time::Instant;

/// Redacted flow key for diagnostics — hash of 5-tuple, not the tuple itself.
#[derive(Debug, Clone)]
pub struct RedactedFlow {
    pub hash: u64,
    pub class: u8,
    pub uplink: u8,
}

/// Support bundle (JSON-serializable for IPC, redacted).
#[derive(Debug, Clone)]
pub struct SupportBundle {
    pub timestamp: String,
    pub service_version: String,
    pub schema_version: u8,
    pub policy_revision: u64,
    pub counters: Counters,
    pub uplinks: Vec<UplinkDiag>,
    pub active_flows: usize,
    pub pending_probes: usize,
    pub sessions: usize,
    pub held_packets: usize,
    pub relay_current: Option<String>,
    pub relay_failover_count: u64,
    pub rekey: RekeyDiag,
    pub congestion: CongestionDiag,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct UplinkDiag {
    pub id: u8,
    pub state: String,
    pub rtt_ms: u32,
    pub loss_permille: u16,
    pub capacity_bps: u64,
    pub weight_permille: u16,
    pub metered: bool,
    pub eligible: bool,
    pub cwnd: u64,
    pub in_flight: u64,
}

#[derive(Debug, Clone)]
pub struct RekeyDiag {
    pub packets_since_rekey: u64,
    pub last_rekey_ago_secs: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct CongestionDiag {
    pub loss_events: u64,
    pub ecn_events: u64,
    pub delay_events: u64,
    pub paths: Vec<u8>,
    pub per_path: Vec<PathCongestionDiag>,
}

#[derive(Debug, Clone)]
pub struct PathCongestionDiag {
    pub id: u8,
    pub cwnd: u64,
    pub in_flight: u64,
    pub ecn_ce: u64,
    pub delay_events: u64,
    pub rtt_min_ms: Option<u64>,
    pub rtt_last_ms: Option<u64>,
}

impl SupportBundle {
    pub fn collect<C: balancify_tunnel::Crypto + Clone>(svc: &Service<C>, now: Instant) -> Self {
        let snap = svc.snapshot();
        let counters = svc.counters();
        let uplinks = snap.uplinks.iter().map(|u| {
            let cwnd = svc.cc.window(u.id.0 as u8) as u64;
            let in_flight = svc.cc.in_flight(u.id.0 as u8) as u64;
            UplinkDiag {
                id: u.id.0,
                state: format!("{:?}", u.state),
                rtt_ms: u.rtt.as_millis().min(u32::MAX as u128) as u32,
                loss_permille: (u.loss * 1000.0).round().min(1000.0) as u16,
                capacity_bps: u.capacity_bps,
                weight_permille: 0,
                metered: u.metered,
                eligible: u.eligible,
                cwnd,
                in_flight,
            }
        }).collect();

        let rekey = RekeyDiag {
            packets_since_rekey: svc.packets_since_rekey(),
            last_rekey_ago_secs: svc.last_rekey().map(|t| now.duration_since(t).as_secs()),
        };

        let paths = svc.cc.paths();
        let per_path = paths.iter().map(|p| {
            PathCongestionDiag {
                id: *p,
                cwnd: svc.cc.window(*p),
                in_flight: svc.cc.in_flight(*p),
                ecn_ce: svc.cc.ecn_ce(*p),
                delay_events: svc.cc.delay_events_path(*p),
                rtt_min_ms: svc.cc.rtt_min(*p).map(|d| d.as_millis() as u64),
                rtt_last_ms: svc.cc.rtt_last(*p).map(|d| d.as_millis() as u64),
            }
        }).collect();
        let congestion = CongestionDiag {
            loss_events: svc.cc.loss_events(),
            ecn_events: svc.cc.ecn_events(),
            delay_events: svc.cc.delay_events(),
            paths,
            per_path,
        };

        SupportBundle {
            timestamp: format!("{:?}", now),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            schema_version: balancify_ipc::SCHEMA_VERSION,
            policy_revision: snap.revision,
            counters,
            uplinks,
            active_flows: svc.active_flows(),
            pending_probes: svc.pending_probes(),
            sessions: svc.sessions(),
            held_packets: svc.held_packets(),
            relay_current: svc.relay_current().map(|a| a.to_string()),
            relay_failover_count: svc.relay_failover_count(),
            rekey,
            congestion,
            warnings: Vec::new(),
        }
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\"timestamp\":\"{}\",\"service_version\":\"{}\",\"schema_version\":{},\"policy_revision\":{},\"active_flows\":{},\"sessions\":{},\"relay_current\":{},\"failovers\":{},\"loss\":{},\"ecn\":{},\"delay\":{},\"rekey_packets\":{},\"per_path\":{}}}",
            self.timestamp,
            self.service_version,
            self.schema_version,
            self.policy_revision,
            self.active_flows,
            self.sessions,
            self.relay_current.as_ref().map(|s| format!("\"{}\"", s)).unwrap_or_else(|| "null".to_string()),
            self.relay_failover_count,
            self.congestion.loss_events,
            self.congestion.ecn_events,
            self.congestion.delay_events,
            self.rekey.packets_since_rekey,
            self.congestion.per_path.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Service, ServiceConfig};
    use balancify_core::UplinkId;
    use balancify_tunnel::PassThrough;
    use std::time::{Duration, Instant};

    #[test]
    fn bundle_includes_per_path_ecn_delay() {
        let now = Instant::now();
        let cfg = ServiceConfig::default();
        let mut svc = Service::new(cfg, PassThrough, PassThrough);
        svc.add_uplink(UplinkId(0), 100_000_000, false, 1500, now);
        for _ in 0..3 {
            svc.on_probe_ok(UplinkId(0), Duration::from_millis(10), now);
        }
        svc.publish(now);
        svc.cc.on_ecn(0, now);
        svc.cc.on_rtt(0, Duration::from_millis(10), now);
        svc.cc.on_rtt(0, Duration::from_millis(50), now + Duration::from_millis(200));
        let bundle = SupportBundle::collect(&svc, now + Duration::from_millis(300));
        assert_eq!(bundle.congestion.ecn_events, 1);
        assert_eq!(bundle.congestion.delay_events, 1);
        assert_eq!(bundle.congestion.per_path.len(), 1);
        assert_eq!(bundle.congestion.per_path[0].ecn_ce, 1);
        assert_eq!(bundle.congestion.per_path[0].delay_events, 1);
        assert!(bundle.congestion.per_path[0].rtt_min_ms.is_some());
        let json = bundle.to_json();
        assert!(json.contains("ecn"));
        assert!(json.contains("per_path"));
    }
}
