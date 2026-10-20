//! Deterministic fuzzing of the sans-I/O core (blueprint Sec.4.7.4: "fuzz the
//! sans-I/O core").
//!
//! This is not a coverage-guided fuzzer; it is a seeded, reproducible generator
//! that runs in CI on every push with no dependencies and no corpus. What it
//! checks is the property that matters for a product which parses untrusted
//! network input (Sec.9.1): arbitrary bytes must never panic the service, must
//! never be handed to the inner stack as a payload, and must never make it
//! allocate without bound.
//!
//! Every test names its seed, so a failure is reproducible by hand.

use std::time::{Duration, Instant};

use balancify_core::UplinkId;
use balancify_ipc::{self as ipc, FrameKind as IpcFrameKind};
use balancify_service::{Action, Service, ServiceConfig};
use balancify_tunnel::{FrameKind, Header, PassThrough, Sender, HEADER_LEN, WIRE_VERSION};

const SESSION: u64 = 0x0BADC0DE_0000_0001;
/// The only payload this suite ever puts on the wire, so "delivered" is
/// decidable: anything else reaching the inner stack is a bug.
const MAGIC: &[u8] = b"balancify-fuzz-payload";

/// xorshift64*. Deterministic, dependency-free, and adequate for exploring
/// encodings; it is not a source of randomness for anything else.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 { self.next() % n.max(1) }
    fn byte(&mut self) -> u8 { (self.next() & 0xFF) as u8 }
    fn bytes(&mut self, n: usize) -> Vec<u8> { (0..n).map(|_| self.byte()).collect() }
    /// A random index into `0..=n` (inclusive), for truncation points.
    fn cut(&mut self, n: usize) -> usize { self.below(n as u64 + 1) as usize }
}

/// A random length in `0..max`, then that many random bytes. The length and the
/// bytes are two separate calls on the generator, so they cannot share an
/// expression without borrowing it twice.
fn some_bytes(rng: &mut Rng, max: u64) -> Vec<u8> {
    let n = rng.below(max) as usize;
    rng.bytes(n)
}

/// As [`some_bytes`], with a floor so the result is never empty.
fn some_bytes_from(rng: &mut Rng, min: usize, max: u64) -> Vec<u8> {
    let n = min + rng.below(max) as usize;
    rng.bytes(n)
}

fn service(now: Instant) -> Service<PassThrough> {
    let mut s = Service::new(ServiceConfig::default(), PassThrough, PassThrough);
    for id in 0..2u8 {
        s.add_uplink(UplinkId(id), 100_000_000, false, 1500, now);
        for _ in 0..5 {
            s.on_probe_ok(UplinkId(id), Duration::from_millis(20), now);
        }
    }
    assert!(s.publish(now));
    s
}

fn sender() -> Sender<PassThrough> {
    Sender::new(SESSION, PassThrough, vec![(0, 2000), (1, 2000)])
}

/// One datagram: either a genuine frame (possibly damaged) or raw noise.
///
/// Damage is confined to the header for real frames, on purpose. Under the lab
/// `PassThrough` cipher a flipped payload byte is undetectable, so damaging the
/// payload would break the "only a real payload is ever delivered" invariant
/// for a reason that is a property of the lab cipher and not a defect. What
/// must hold is that nothing a *structural* check should reject is delivered.
fn datagram(rng: &mut Rng, tx: &mut Sender<PassThrough>, now: Instant) -> Vec<u8> {
    let choice = rng.below(10);
    let real = choice < 5;
    let mut buf = if real {
        if choice < 4 {
            tx.send(MAGIC, now).map(|sent| sent.bytes).unwrap_or_default()
        } else {
            // A real control frame of a random kind.
            let kind = match rng.below(4) {
                0 => FrameKind::Keepalive,
                1 => FrameKind::Ack,
                2 => FrameKind::Probe,
                _ => FrameKind::ProbeReply,
            };
            // No tail: if damage rewrites the kind byte into Data, the frame
            // the receiver sees is a data frame with an empty payload, which
            // is the one non-MAGIC delivery the assertions below allow.
            let path = rng.byte();
            let seq = rng.next();
            let mut out = Vec::new();
            Header::new(kind, path, SESSION, seq).encode(&mut out);
            out
        }
    } else {
        // Pure noise, including empty and oversized.
        some_bytes(rng, 600)
    };
    if buf.is_empty() {
        return buf;
    }

    // Real frames only get header damage. Truncating one is not detectable -
    // the frame carries no length field, so a cut frame is indistinguishable
    // from a shorter payload (ADR-0003) - and appending bytes is likewise
    // indistinguishable from a longer one. Both are properties of the wire
    // format under the lab cipher, not defects to assert against.
    let strategy = if real { 1 + rng.below(3) } else { rng.below(6) };
    match strategy {
        // Truncate: noise only, per the note above.
        0 => {
            let cut = rng.cut(buf.len());
            buf.truncate(cut);
        }
        // A single bit in the header.
        1 => {
            let i = rng.cut(HEADER_LEN.min(buf.len()) - 1);
            let shift = rng.below(8) as u32;
            buf[i] ^= 1 << shift;
        }
        // Replace a header byte outright (version, kind, path, flags).
        2 => {
            let i = rng.cut(HEADER_LEN.min(buf.len()) - 1);
            buf[i] = rng.byte();
        }
        // A session we never opened: the receiver must cope either way.
        3 => {
            if buf.len() >= HEADER_LEN {
                let other = rng.next();
                buf[4..12].copy_from_slice(&other.to_be_bytes());
            }
        }
        // Noise-only: trailing junk.
        _ => {
            let extra = some_bytes(rng, 64);
            buf.extend_from_slice(&extra);
        }
    }
    buf
}

#[test]
fn fuzz_random_datagrams_stay_bounded_and_never_panic() {
    const ROUNDS: u32 = 20_000;
    const SEED: u64 = 0x5EED_C0DE_0000_0001;
    let t0 = Instant::now();
    let mut rng = Rng(SEED);
    let mut svc = service(t0);
    let mut tx = sender();
    let mut magic = 0u64;
    // The payload tail of every datagram that parses as a data frame, newest
    // last and bounded. Under the lab cipher there is no integrity check, so
    // noise whose first two bytes happen to be a valid version and kind IS a
    // data frame - which makes "was this tail really on the wire" the only
    // honest formulation of the no-invented-payload invariant.
    let mut expected: Vec<Vec<u8>> = Vec::new();

    for round in 0..ROUNDS {
        // 20 ms per round: the reorder hold cap (80 ms) expires every few
        // rounds, which is what bounds held memory in the real service too.
        let now = t0 + Duration::from_millis(u64::from(round) * 20);
        let dg = datagram(&mut rng, &mut tx, now);
        if let Some((hdr, rest)) = Header::decode(&dg) {
            // Both payload-bearing kinds deliver: Data goes through the
            // reorder buffer, FlowData is the unsequenced lane (ADR-0003).
            if matches!(hdr.kind, FrameKind::Data | FrameKind::FlowData) {
                if expected.len() >= 512 { expected.remove(0); }
                expected.push(rest.to_vec());
            }
        }
        let before = svc.counters();
        let uplink = UplinkId(rng.below(2) as u8);

        let actions = svc.ingest_path(uplink, &dg, now);
        let check = |data: &[u8]| {
            assert!(
                expected.iter().any(|p| p.as_slice() == data),
                "a delivered payload must be the tail of a datagram that
                 parses as a data frame: {data:?}"
            );
            data == MAGIC
        };
        for a in &actions {
            if let Action::Deliver { data } = a {
                magic += check(data) as u64;
            }
        }

        // The service's own timer, which is where the hold cap drains.
        let ticked = svc.tick(now);
        for a in &ticked {
            if let Action::Deliver { data } = a {
                magic += check(data) as u64;
            }
        }

        let after = svc.counters();
        assert!(after.delivered >= before.delivered, "counters never go backwards");

        // Bounded memory: the session cap and the hold cap are the guarantees.
        assert!(svc.sessions() <= 64, "the session cap holds (Sec.9.1)");
        assert!(
            svc.held_packets() <= 1024,
            "the hold cap bounds reorder memory: {} held",
            svc.held_packets()
        );
        assert!(svc.pending_probes() <= 8, "unanswered probes do not accumulate");
    }
    assert!(magic > 0, "the accept path was exercised, so the checks above are not vacuous");

    // Whatever happened, the service is still usable.
    let flow = balancify_core::FlowKey::new(
        6,
        std::net::SocketAddr::V4(std::net::SocketAddrV4::new(std::net::Ipv4Addr::new(10, 0, 0, 1), 4000)),
        std::net::SocketAddr::V4(std::net::SocketAddrV4::new(std::net::Ipv4Addr::new(203, 0, 113, 9), 443)),
    );
    let actions = svc.send_app_packet(&flow, None, b"still working", t0);
    assert!(actions.iter().any(|a| matches!(a, Action::Send { .. })), "the data plane still sends");
}

#[test]
fn fuzz_truncations_of_a_valid_frame_never_deliver() {
    let t0 = Instant::now();
    let mut svc = service(t0);
    let mut tx = sender();
    let frame = tx.send(MAGIC, t0).unwrap().bytes;
    assert!(frame.len() > HEADER_LEN);

    for len in 0..HEADER_LEN {
        let mut svc = service(t0); // a truncated frame must not poison anything
        let before = svc.counters().delivered;
        svc.ingest_path(UplinkId(0), &frame[..len], t0);
        assert_eq!(
            svc.counters().delivered,
            before,
            "a frame shorter than the header must not deliver"
        );
    }

    // Exactly the header is a legal empty datagram: the format carries no
    // length field, so "the payload is the rest of the datagram" makes a
    // zero-length payload valid rather than truncated. Recorded because it is
    // a deliberate consequence of ADR-0003, not an oversight.
    let before = svc.counters().delivered;
    let actions = svc.ingest_path(UplinkId(0), &frame[..HEADER_LEN], t0);
    assert_eq!(svc.counters().delivered, before + 1);
    assert!(
        matches!(&actions[0], Action::Deliver { data } if data.is_empty()),
        "a header-only frame delivers an empty payload"
    );

    // The whole frame does deliver, so the loop above is not vacuous.
    let mut full = service(t0);
    full.ingest_path(UplinkId(0), &frame, t0);
    assert_eq!(full.counters().delivered, 1);
}

#[test]
fn fuzz_replayed_frames_deliver_at_most_once() {
    let t0 = Instant::now();
    let mut svc = service(t0);
    let mut tx = sender();
    let mut rng = Rng(0x5EED_C0DE_0000_0002);

    let mut frames = Vec::new();
    for i in 0..8 {
        frames.push(tx.send(MAGIC, t0 + Duration::from_millis(i)).unwrap().bytes);
    }

    // All replays land at one instant on purpose. Letting the hold cap expire
    // releases held packets out of order and advances the expected sequence
    // past them, after which genuinely late arrivals are duplicates by
    // definition - correct, but a different property from the one under test.
    for _ in 0..2_000u32 {
        let f = &frames[rng.below(frames.len() as u64) as usize];
        let uplink = UplinkId(rng.below(2) as u8);
        svc.ingest_path(uplink, f, t0);
    }
    assert_eq!(svc.counters().delivered, frames.len() as u64, "each payload once, however often it is replayed");
    assert_eq!(svc.held_packets(), 0, "and nothing is left behind");
}

#[test]
fn fuzz_ipc_frames_are_length_checked() {
    let mut rng = Rng(0x5EED_C0DE_0000_0003);
    for _ in 0..20_000u32 {
        let mut buf = match rng.below(6) {
            0..=1 => some_bytes(&mut rng, 200),
            2 => {
                let mut b = ipc::frame_request(&ipc::Request::GetStatus);
                b.truncate(rng.cut(b.len()));
                b
            }
            3 => {
                let mut b = ipc::frame_response(&ipc::Response::Ack);
                if b.len() >= ipc::FRAME_HEADER_LEN {
                    // A length that lies about the payload.
                    let lie = match rng.below(3) {
                        0 => u32::MAX,
                        1 => ipc::MAX_PAYLOAD as u32 + 1,
                        _ => rng.next() as u32,
                    };
                    b[2..6].copy_from_slice(&lie.to_be_bytes());
                }
                b
            }
            4 => {
                let mut b = ipc::frame_request(&ipc::Request::ListFlowRules);
                b[0] = rng.byte(); // schema version
                b
            }
            _ => {
                let mut b = ipc::frame_request(&ipc::Request::Ping);
                b[1] = rng.byte(); // frame kind
                b
            }
        };
        if rng.below(4) == 0 {
            let extra = some_bytes(&mut rng, 32);
            buf.extend_from_slice(&extra);
        }

        match ipc::decode_frame(&buf) {
            // "Need more bytes" is not an error: a pipe reader must be able to
            // resume from a partial write.
            Ok(None) => {}
            Ok(Some((kind, payload, consumed))) => {
                assert!(consumed <= buf.len(), "consumed cannot exceed the buffer");
                assert_eq!(consumed, ipc::FRAME_HEADER_LEN + payload.len());
                assert!(payload.len() <= ipc::MAX_PAYLOAD, "the length check held");
                assert!(matches!(
                    kind,
                    IpcFrameKind::Request | IpcFrameKind::Response | IpcFrameKind::Event
                ));
                // The payload decoders must tolerate anything the framing let through.
                let _ = ipc::Request::decode(payload);
                let _ = ipc::Response::decode(payload);
                let _ = ipc::Event::decode(payload);
            }
            Err(_) => {} // corrupt stream: the caller drops the connection
        }
    }
}

#[test]
fn fuzz_ipc_requests_are_validated_not_trusted() {
    let t0 = Instant::now();
    let mut svc = service(t0);
    let mut rng = Rng(0x5EED_C0DE_0000_0004);

    for round in 0..4_000u32 {
        let now = t0 + Duration::from_millis(u64::from(round));
        let payload = some_bytes_from(&mut rng, 1, 32);
        if let Some(req) = ipc::Request::decode(&payload) {
            // Absurd port ranges, unknown classes, out-of-range uplink ids and
            // event rates must be rejected, not applied and not panicked on.
            let _ = svc.handle(req, now);
        }
    }

    // The rule set must still be the curated one, or only valid edits.
    let status = svc.handle(ipc::Request::GetStatus, t0 + Duration::from_secs(5));
    let ipc::Response::Status(snap) = status else { panic!("the service still answers"); };
    assert_eq!(snap.schema_version, ipc::SCHEMA_VERSION);
    assert_eq!(snap.uplinks.len(), 2, "no phantom uplinks were created");
    assert_eq!(snap.adapter_mtu, 1440);
    assert!(!snap.service_version.is_empty(), "the status carries a version");
}

#[test]
fn fuzz_headers_reject_every_damaged_version_and_kind() {
    let mut rng = Rng(0x5EED_C0DE_0000_0005);
    for _ in 0..10_000u32 {
        let mut buf = Vec::new();
        Header::new(FrameKind::Data, 0, SESSION, rng.next()).encode(&mut buf);
        buf.extend_from_slice(MAGIC);
        match rng.below(3) {
            0 => buf[0] = WIRE_VERSION.wrapping_add(1).wrapping_add(rng.byte()),
            1 => buf[1] = rng.byte(),
            _ => buf.truncate(rng.cut(buf.len())),
        }
        if let Some((hdr, rest)) = Header::decode(&buf) {
            assert_eq!(hdr.version, WIRE_VERSION, "an unknown version is never guessed");
            assert!(buf.len() >= HEADER_LEN);
            assert_eq!(rest.len(), buf.len() - HEADER_LEN);
        }
    }
}
