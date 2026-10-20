//! Flow classification rules (blueprint Sec.6.2 step 1).
//!
//! Classification is 5-tuple + (where obtainable) process + port/DNS/SNI rule
//! match. Classes: `latency-sensitive` (DNS, voice, game ports) -> lowest-delay
//! healthy uplink; `bulk` -> hashed; `pinned` -> user rule.
//!
//! Rules are first-party and evaluated in priority order; the first match wins.
//! The default set is the curated latency-sensitive list from Sec.6.2/Sec.6.4 rule 5.

use std::net::SocketAddr;

use crate::placement::{FlowClass, FlowKey, UplinkId};

/// Rule identifiers below this value are reserved for built-in curated rules.
pub const FIRST_CUSTOM_RULE_ID: u32 = 1_000;

/// Inclusive destination-port range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortRange {
    pub start: u16,
    pub end: u16,
}

impl PortRange {
    pub fn one(port: u16) -> Self { Self { start: port, end: port } }
    pub fn range(start: u16, end: u16) -> Self { Self { start, end } }
    pub fn contains(&self, port: u16) -> bool { port >= self.start && port <= self.end }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Stable identifier used by the IPC contract and the Rules screen.
    pub id: u32,
    pub name: String,
    pub class: FlowClass,
    /// Lower value = evaluated first.
    pub priority: u32,
    /// IP protocol number (`None` = any): 6 TCP, 17 UDP, 1 ICMP.
    pub proto: Option<u8>,
    pub dst_port: Option<PortRange>,
    /// Suffix match on the SNI/server name learned for the flow, e.g. "meet.example".
    pub sni_suffix: Option<String>,
    /// Eligible for duplicate mode (Sec.6.4 rule 5: small critical packets).
    pub duplicate_eligible: bool,
}

impl Rule {
    pub fn new(id: u32, name: impl Into<String>, class: FlowClass) -> Self {
        Self {
            id,
            name: name.into(),
            class,
            priority: 100,
            proto: None,
            dst_port: None,
            sni_suffix: None,
            duplicate_eligible: false,
        }
    }
    pub fn proto(mut self, p: u8) -> Self { self.proto = Some(p); self }
    pub fn port(mut self, r: PortRange) -> Self { self.dst_port = Some(r); self }
    pub fn sni(mut self, suffix: impl Into<String>) -> Self { self.sni_suffix = Some(suffix.into()); self }
    pub fn priority(mut self, p: u32) -> Self { self.priority = p; self }
    pub fn duplicate(mut self, yes: bool) -> Self { self.duplicate_eligible = yes; self }

    /// `sni` is the server name learned for this flow, when the control plane
    /// has one (DNS/SNI observation); `None` never matches an SNI rule.
    pub fn matches(&self, flow: &FlowKey, sni: Option<&str>) -> bool {
        if let Some(p) = self.proto { if flow.proto != p { return false; } }
        if let Some(r) = self.dst_port { if !r.contains(flow.dst.port()) { return false; } }
        if let Some(suffix) = &self.sni_suffix {
            match sni {
                Some(name) => {
                    let name = name.to_ascii_lowercase();
                    let suffix = suffix.to_ascii_lowercase();
                    if !(name == suffix || name.ends_with(&format!(".{suffix}"))) { return false; }
                }
                None => return false,
            }
        }
        true
    }
}

/// Ordered rule list. `classify` returns the class of the first matching rule,
/// or [`FlowClass::Bulk`] when nothing matches (Sec.6.2 default).
#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    rules: Vec<Rule>,
    revision: u64,
}

impl RuleSet {
    pub fn new() -> Self { Self::default() }

    /// The curated default set (Sec.6.2 latency-sensitive list + Sec.6.4 rule 5
    /// duplicate candidates). Ids below 1000 are reserved for built-ins.
    pub fn curated() -> Self {
        let mut s = Self::new();
        s.add(
            Rule::new(1, "dns", FlowClass::LatencySensitive)
                .port(PortRange::one(53))
                .priority(10)
                .duplicate(true),
        );
        s.add(
            Rule::new(2, "stun-turn", FlowClass::LatencySensitive)
                .proto(17)
                .port(PortRange::range(3478, 3479))
                .priority(11)
                .duplicate(true),
        );
        s.add(
            Rule::new(3, "voice-media", FlowClass::LatencySensitive)
                .proto(17)
                .port(PortRange::range(50_000, 50_100))
                .priority(12),
        );
        s.add(
            Rule::new(4, "https-bulk", FlowClass::Bulk)
                .proto(6)
                .port(PortRange::one(443))
                .priority(90),
        );
        s
    }

    /// Insert or replace a rule by its stable id, then restore evaluation order.
    /// A rule set is id-unique: updating an existing rule must never leave a
    /// shadowed duplicate that makes removal or UI state ambiguous.
    pub fn add(&mut self, rule: Rule) -> u64 {
        self.rules.retain(|existing| existing.id != rule.id);
        self.rules.push(rule);
        self.sort();
        self.bump()
    }

    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.rules.len();
        self.rules.retain(|r| r.id != id);
        if self.rules.len() != before { self.bump(); true } else { false }
    }

    pub fn pin(&mut self, id: u32, name: impl Into<String>, uplink: UplinkId) -> u64 {
        self.add(
            Rule::new(id, name, FlowClass::Pinned(uplink))
                .priority(1)
                .duplicate(false),
        )
    }

    pub fn rules(&self) -> &[Rule] { &self.rules }
    pub fn revision(&self) -> u64 { self.revision }

    pub fn classify(&self, flow: &FlowKey, sni: Option<&str>) -> FlowClass {
        self.rules
            .iter()
            .find(|r| r.matches(flow, sni))
            .map(|r| r.class)
            .unwrap_or(FlowClass::Bulk)
    }

    /// Whether the matched rule opts this flow into duplicate mode (Sec.6.4 rule 5).
    pub fn duplicate_eligible(&self, flow: &FlowKey, sni: Option<&str>) -> bool {
        self.rules
            .iter()
            .find(|r| r.matches(flow, sni))
            .map(|r| r.duplicate_eligible)
            .unwrap_or(false)
    }

    fn sort(&mut self) {
        self.rules.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.id.cmp(&b.id)));
    }
    fn bump(&mut self) -> u64 { self.revision += 1; self.revision }
}

/// Convenience for callers that only have addresses.
pub fn flow_of(proto: u8, src: SocketAddr, dst: SocketAddr) -> FlowKey { FlowKey::new(proto, src, dst) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::sa;

    fn f(proto: u8, port: u16) -> FlowKey { FlowKey::new(proto, sa(40_000), sa(port)) }

    #[test]
    fn curated_rules_classify_dns_as_latency_sensitive() {
        let rs = RuleSet::curated();
        assert_eq!(rs.classify(&f(17, 53), None), FlowClass::LatencySensitive);
        assert_eq!(rs.classify(&f(6, 53), None), FlowClass::LatencySensitive, "TCP DNS too");
        assert_eq!(rs.classify(&f(17, 3478), None), FlowClass::LatencySensitive);
        assert_eq!(rs.classify(&f(6, 443), None), FlowClass::Bulk);
        assert_eq!(rs.classify(&f(6, 8080), None), FlowClass::Bulk, "unmatched defaults to bulk");
    }

    #[test]
    fn priority_order_wins_and_pin_beats_class_rules() {
        let mut rs = RuleSet::curated();
        rs.pin(500, "pin-dns-to-uplink-2", UplinkId(2));
        assert_eq!(rs.classify(&f(17, 53), None), FlowClass::Pinned(UplinkId(2)));
        assert_eq!(rs.rules().first().unwrap().id, 500, "priority 1 sorts first");
        assert!(rs.remove(500));
        assert_eq!(rs.classify(&f(17, 53), None), FlowClass::LatencySensitive);
    }

    #[test]
    fn sni_rules_match_suffix_only() {
        let mut rs = RuleSet::new();
        rs.add(
            Rule::new(10, "meet", FlowClass::LatencySensitive)
                .sni("meet.example")
                .priority(5),
        );
        let flow = f(6, 443);
        assert_eq!(rs.classify(&flow, Some("meet.example")), FlowClass::LatencySensitive);
        assert_eq!(rs.classify(&flow, Some("eu.meet.example")), FlowClass::LatencySensitive);
        assert_eq!(rs.classify(&flow, Some("notmeet.example")), FlowClass::Bulk, "not a suffix boundary");
        assert_eq!(rs.classify(&flow, Some("EVIL-MEET.EXAMPLE")), FlowClass::Bulk);
        assert_eq!(rs.classify(&flow, None), FlowClass::Bulk, "no SNI observed => no match");
    }

    #[test]
    fn duplicate_eligibility_follows_the_rule() {
        let rs = RuleSet::curated();
        assert!(rs.duplicate_eligible(&f(17, 53), None));
        assert!(!rs.duplicate_eligible(&f(6, 443), None));
    }

    #[test]
    fn adding_an_existing_id_replaces_instead_of_creating_a_duplicate() {
        let mut rs = RuleSet::new();
        rs.add(Rule::new(42, "old", FlowClass::Bulk).priority(50));
        let revision = rs.revision();

        rs.add(Rule::new(42, "updated", FlowClass::LatencySensitive).priority(5));

        assert_eq!(rs.rules().len(), 1);
        assert_eq!(rs.revision(), revision + 1);
        assert_eq!(rs.rules()[0].name, "updated");
        assert_eq!(rs.rules()[0].class, FlowClass::LatencySensitive);
        assert_eq!(rs.rules()[0].priority, 5);
        assert!(rs.remove(42));
        assert!(rs.rules().is_empty());
    }

    #[test]
    fn edits_bump_revision_for_ipc_events() {
        let mut rs = RuleSet::new();
        let r0 = rs.revision();
        rs.add(Rule::new(1, "a", FlowClass::Bulk));
        assert!(rs.revision() > r0);
        let r1 = rs.revision();
        assert!(!rs.remove(999));
        assert_eq!(rs.revision(), r1, "no-op removal must not churn the revision");
    }

    #[test]
    fn port_range_boundaries() {
        let r = PortRange::range(50_000, 50_100);
        assert!(r.contains(50_000) && r.contains(50_100));
        assert!(!r.contains(49_999) && !r.contains(50_101));
    }
}
