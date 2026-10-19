//! MTU / MSS handling (blueprint §6.6).
//!
//! "Virtual adapter MTU = min(path MTU) − tunnel header; clamp TCP MSS
//! accordingly; pass ICMP (incl. PTB) so PMTUD works end-to-end."
//!
//! The adapter MTU must be derived from the *narrowest live path*, and it has
//! to be recomputed when a path drops out — otherwise every packet to the
//! narrow path is fragmented or black-holed.

/// RFC 791 minimum IPv4 MTU: below this, IPv4 cannot be served without
/// fragmentation, so the configuration is not viable.
pub const IPV4_MIN_MTU: u16 = 576;
/// IPv4 header + TCP header.
pub const MSS_OVERHEAD_V4: u16 = 40;
/// IPv6 header + TCP header (the conservative clamp for dual-stack §6.6).
pub const MSS_OVERHEAD_V6: u16 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MtuPolicy {
    /// Tunnel header bytes added to every payload packet on the wire
    /// (framing + encapsulation + AEAD tag).
    pub tunnel_overhead: u16,
}

impl MtuPolicy {
    pub fn new(tunnel_overhead: u16) -> Self { Self { tunnel_overhead } }

    /// Adapter MTU from the set of live path MTUs; `None` when no path is up.
    pub fn adapter_mtu(&self, path_mtus: &[u16]) -> Option<u16> {
        let min = path_mtus.iter().copied().min()?;
        Some(min.saturating_sub(self.tunnel_overhead))
    }

    /// True when the adapter MTU can carry IPv4 at all (§6.6).
    pub fn is_viable(adapter_mtu: u16) -> bool { adapter_mtu >= IPV4_MIN_MTU }

    /// Dual-stack safe MSS clamp (§6.6): the smaller IPv6-based value.
    pub fn mss_clamp(adapter_mtu: u16) -> u16 { adapter_mtu.saturating_sub(MSS_OVERHEAD_V6) }
    pub fn mss_clamp_v4(adapter_mtu: u16) -> u16 { adapter_mtu.saturating_sub(MSS_OVERHEAD_V4) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_mtu_follows_the_narrowest_path() {
        let p = MtuPolicy::new(60); // header + outer UDP/IP + AEAD tag
        assert_eq!(p.adapter_mtu(&[1500, 1500]), Some(1440));
        assert_eq!(p.adapter_mtu(&[1500, 1430]), Some(1370), "narrowest live path wins");
        assert_eq!(p.adapter_mtu(&[]), None, "no path => no adapter MTU");
    }

    #[test]
    fn losing_a_path_recomputes_the_mtu() {
        let p = MtuPolicy::new(60);
        let both = p.adapter_mtu(&[1500, 1280]).unwrap();
        let after = p.adapter_mtu(&[1500]).unwrap();
        assert_eq!(both, 1220);
        assert_eq!(after, 1440, "the narrow path going away must widen the adapter again");
    }

    #[test]
    fn mss_clamp_leaves_room_for_headers() {
        assert_eq!(MtuPolicy::mss_clamp(1440), 1380);
        assert_eq!(MtuPolicy::mss_clamp_v4(1440), 1400);
        assert_eq!(MtuPolicy::mss_clamp(10), 0, "saturating, never wraps");
    }

    #[test]
    fn viability_floor_is_the_ipv4_minimum() {
        assert!(MtuPolicy::is_viable(IPV4_MIN_MTU));
        assert!(!MtuPolicy::is_viable(IPV4_MIN_MTU - 1));
    }

    #[test]
    fn overhead_larger_than_path_saturates() {
        let p = MtuPolicy::new(2000);
        assert_eq!(p.adapter_mtu(&[1500]), Some(0));
        assert!(!MtuPolicy::is_viable(0));
    }
}
