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

        let congestion = CongestionDiag {
            loss_events: svc.cc.loss_events(),
            ecn_events: svc.cc.ecn_events(),
            delay_events: svc.cc.delay_events(),
            paths: svc.cc.paths(),
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
            "{{\"timestamp\":\"{}\",\"service_version\":\"{}\",\"schema_version\":{},\"policy_revision\":{},\"active_flows\":{},\"sessions\":{},\"relay_current\":{},\"failovers\":{},\"loss\":{},\"ecn\":{},\"delay\":{},\"rekey_packets\":{}}}",
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
            self.rekey.packets_since_rekey
        )
    }
}
