//! Balancify first-party datagram lane (blueprint §6.4, §4.8 rev 2).
//!
//! Carriage: UDP datagrams, never TCP (§6.4 rule 4 - no TCP-over-TCP).
//! Every tunnel packet carries (session, path, seq); the receiving endpoint
//! (in-binary server mode, §4.7 T1) releases payloads in order through a
//! reorder buffer whose hold time is capped so one slow path cannot stall a
//! flow (§6.4 rule 3).
//!
//! Crypto: the [`Crypto`] trait isolates the AEAD. Phase 2 tests use
//! [`PassThrough`]; Phase 4 swaps in ChaCha20-Poly1305 under the Noise-style
//! handshake via a vetted vendored crate (§4.8 policy rule c).

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub const HEADER_LEN: usize = 17; // session u64 + path u8 + seq u64

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub session: u64,
    pub path: u8,
    pub seq: u64,
}

impl Header {
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.session.to_be_bytes());
        out.push(self.path);
        out.extend_from_slice(&self.seq.to_be_bytes());
    }
    pub fn decode(buf: &[u8]) -> Option<(Self, &[u8])> {
        if buf.len() < HEADER_LEN { return None; }
        let session = u64::from_be_bytes(buf[0..8].try_into().ok()?);
        let path = buf[8];
        let seq = u64::from_be_bytes(buf[9..17].try_into().ok()?);
        Some((Self { session, path, seq }, &buf[HEADER_LEN..]))
    }
}

/// AEAD boundary (see module docs). Implementations must be thread-safe.
pub trait Crypto: Send + Sync {
    /// seal appends tag material to `out`; open verifies and returns payload.
    fn seal(&self, path: u8, seq: u64, plain: &[u8], out: &mut Vec<u8>);
    fn open(&self, path: u8, seq: u64, cipher: &[u8]) -> Option<Vec<u8>>;
}

/// Test-only null cipher. MUST NOT ship (enforced by a Phase-4 CI audit).
#[derive(Default)]
pub struct PassThrough;
impl Crypto for PassThrough {
    fn seal(&self, _p: u8, _s: u64, plain: &[u8], out: &mut Vec<u8>) { out.extend_from_slice(plain); }
    fn open(&self, _p: u8, _s: u64, cipher: &[u8]) -> Option<Vec<u8>> { Some(cipher.to_vec()) }
}

/// Sender: numbers packets and selects the egress path per packet.
pub struct Sender<C: Crypto> {
    pub session: u64,
    next_seq: u64,
    crypto: C,
    drr: DeficitRoundRobin,
}

impl<C: Crypto> Sender<C> {
    pub fn new(session: u64, crypto: C, paths: Vec<(u8, u64)>) -> Self {
        Self { session, next_seq: 0, crypto, drr: DeficitRoundRobin::new(paths) }
    }
    /// Update path quanta when the control plane republishes weights (§6.1).
    pub fn set_quanta(&mut self, paths: Vec<(u8, u64)>) { self.drr = DeficitRoundRobin::new(paths); }
    /// Emit one tunnel packet for `plain`; returns (path, bytes).
    pub fn send(&mut self, plain: &[u8]) -> (u8, Vec<u8>) {
        let path = self.drr.next();
        let seq = self.next_seq;
        self.next_seq += 1;
        let mut out = Vec::with_capacity(HEADER_LEN + plain.len());
        Header { session: self.session, path, seq }.encode(&mut out);
        self.crypto.seal(path, seq, plain, &mut out);
        (path, out)
    }
}

/// Deficit round robin with quanta proportional to estimated capacity (§6.4 rule 2).
#[derive(Debug, Clone)]
pub struct DeficitRoundRobin {
    paths: Vec<(u8, u64)>, // (path id, quantum bytes/round)
    deficit: Vec<i64>,
    idx: usize,
}

impl DeficitRoundRobin {
    pub fn new(paths: Vec<(u8, u64)>) -> Self {
        let n = paths.len();
        Self { paths, deficit: vec![0; n], idx: 0 }
    }
    /// One packet-sized unit (1400 B proposed) is charged per selection.
    pub fn next(&mut self) -> u8 { self.next_with_cost(1400) }
    pub fn next_with_cost(&mut self, cost: u64) -> u8 {
        assert!(!self.paths.is_empty(), "no healthy paths");
        loop {
            if self.deficit[self.idx] >= cost as i64 {
                self.deficit[self.idx] -= cost as i64;
                let p = self.paths[self.idx].0;
                self.idx = (self.idx + 1) % self.paths.len();
                return p;
            }
            self.deficit[self.idx] += self.paths[self.idx].1 as i64;
            self.idx = (self.idx + 1) % self.paths.len();
        }
    }
}

/// Receiver reorder buffer (§6.4 rule 3): releases in order; on hold-cap expiry
/// releases out-of-order and advances (inner TCP recovers; measured trade-off §10.3).
pub struct ReorderBuffer {
    next_seq: u64,
    hold: BTreeMap<u64, Vec<u8>>,
    cap: Duration,
    stalled_at: Option<Instant>,
    pub released_out_of_order: u64,
}

impl ReorderBuffer {
    pub fn new(cap: Duration) -> Self {
        Self { next_seq: 0, hold: BTreeMap::new(), cap, stalled_at: None, released_out_of_order: 0 }
    }
    pub fn push(&mut self, seq: u64, payload: Vec<u8>) {
        if seq < self.next_seq { return; } // duplicate/late beyond window
        self.hold.insert(seq, payload);
    }
    /// Release everything deliverable at `now`.
    pub fn pop_ready(&mut self, now: Instant) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            if let Some(p) = self.hold.remove(&self.next_seq) {
                self.next_seq += 1;
                self.stalled_at = None;
                out.push(p);
                continue;
            }
            if self.hold.is_empty() { self.stalled_at = None; break; }
            let stalled = *self.stalled_at.get_or_insert(now);
            if now.duration_since(stalled) >= self.cap {
                // cap expiry: skip the gap, release what we have in seq order
                let (_, p) = self.hold.pop_first().expect("non-empty");
                self.released_out_of_order += 1;
                self.next_seq = self.hold.keys().next().copied().unwrap_or(self.next_seq + 1);
                out.push(p);
                self.stalled_at = None;
                continue;
            }
            break;
        }
        out
    }
}

/// Speculative duplication policy (§6.4 rule 5): duplicate small critical packets.
pub fn should_duplicate(plain_len: usize, critical: bool, duplicate_mode: bool) -> bool {
    duplicate_mode && critical && plain_len <= 256
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip_and_truncation() {
        let h = Header { session: 0x1122_3344_5566_7788, path: 3, seq: 42 };
        let mut buf = Vec::new();
        h.encode(&mut buf);
        buf.extend_from_slice(b"payload");
        let (d, rest) = Header::decode(&buf).unwrap();
        assert_eq!(d, h);
        assert_eq!(rest, b"payload");
        assert!(Header::decode(&buf[..HEADER_LEN - 1]).is_none());
    }

    #[test]
    fn drr_proportional_to_quanta() {
        let mut d = DeficitRoundRobin::new(vec![(0, 3000), (1, 1000)]);
        let mut c = [0u32; 2];
        for _ in 0..400 { c[d.next() as usize] += 1; }
        let ratio = c[0] as f64 / c[1] as f64;
        assert!((2.7..3.3).contains(&ratio), "expected ~3:1, got {} ({:?})", ratio, c);
    }

    #[test]
    fn reorder_releases_in_order_and_waits() {
        let mut rb = ReorderBuffer::new(Duration::from_millis(80));
        let t0 = Instant::now();
        rb.push(1, b"one".to_vec());
        rb.push(0, b"zero".to_vec());
        let r = rb.pop_ready(t0);
        assert_eq!(r, vec![b"zero".to_vec(), b"one".to_vec()]);
        // gap: seq 3 arrives before 2 -> held
        rb.push(3, b"three".to_vec());
        assert!(rb.pop_ready(t0).is_empty());
        rb.push(2, b"two".to_vec());
        let r = rb.pop_ready(t0);
        assert_eq!(r, vec![b"two".to_vec(), b"three".to_vec()]);
        assert_eq!(rb.released_out_of_order, 0);
    }

    #[test]
    fn reorder_cap_expiry_does_not_stall_flow() {
        let mut rb = ReorderBuffer::new(Duration::from_millis(40));
        let t0 = Instant::now();
        rb.push(1, b"one".to_vec());   // seq 0 lost
        rb.push(2, b"two".to_vec());
        assert!(rb.pop_ready(t0).is_empty());
        let r = rb.pop_ready(t0 + Duration::from_millis(45));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0], b"one".to_vec());
        assert_eq!(rb.released_out_of_order, 1);
        let r = rb.pop_ready(t0 + Duration::from_millis(46));
        assert_eq!(r, vec![b"two".to_vec()]);
    }

    #[test]
    fn sender_numbers_and_spreads() {
        let mut s = Sender::new(7, PassThrough, vec![(0, 3000), (1, 1000)]);
        let mut seen_paths = std::collections::HashSet::new();
        let mut seqs = Vec::new();
        for i in 0..20 {
            let (p, bytes) = s.send(format!("pkt{i}").as_bytes());
            let (h, rest) = Header::decode(&bytes).unwrap();
            assert_eq!(h.session, 7);
            assert_eq!(h.path, p);
            seqs.push(h.seq);
            seen_paths.insert(p);
            assert_eq!(rest, format!("pkt{i}").as_bytes());
        }
        assert_eq!(seqs, (0..20).collect::<Vec<u64>>());
        assert_eq!(seen_paths.len(), 2);
    }

    #[test]
    fn duplicate_policy_rules() {
        assert!(should_duplicate(64, true, true));
        assert!(!should_duplicate(1000, true, true), "large packets never duplicated");
        assert!(!should_duplicate(64, false, true));
        assert!(!should_duplicate(64, true, false), "mode off"));
    }
}
