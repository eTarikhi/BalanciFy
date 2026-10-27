# ADR-0003 — Tunnel wire v0: explicit frame kinds, reinjection, FlowData

**Status:** Accepted; wire v2 is current (2026-10-20) · **Related:** ADR-0002, ADR-0006, blueprint §6.4–§6.6

## Context

The Phase-2 lane header was `session u64 | path u8 | seq u64` — enough to
stripe and reorder, but it cannot express the control traffic the design
requires: health probes must be *authenticated tunnel control messages, not
raw ICMP* so they survive CGNAT (§6.5), NAT keepalives must exist per path
(§6.6), and capacity estimation (§6.3) plus reinjection (§6.4 rule 5) need
acknowledgements. There is also a correctness gap that only shows up once both
lanes exist: the reorder buffer is per session, so mixing **non-striped**
traffic into the same sequence space lets a slow uplink hold up packets from a
fast one — the exact failure §6.4 rule 3 exists to prevent, inflicted on flows
that never asked to be bonded.

## Decision

1. **Wire v1 header, 20 bytes:** `version u8 | kind u8 | path u8 | flags u8 |
   session u64 | seq u64`. `version` is checked; an unknown version or kind is
   dropped, never guessed at (§9.2 "schema versioned and length-checked").
   Version 1 introduced AEAD nonce separation by frame kind. Version 2 keeps
   that construction and adds a selective FlowAck kind; peers require an exact
   version match and must not interoperate with v1 or the initial v0 framing.
2. **Frame kinds:** `Data` (sequenced, striped, reordered), `Keepalive`,
   `Probe`, `ProbeReply`, `Ack` (cumulative), `FlowData`, and `FlowAck`
   (authenticated selective receipt for one FlowData sequence).
3. **`FlowData` is the flow-level lane (§6.2):** uses an independent per-session
   sequence for nonce uniqueness and a bounded replay window, but is delivered
   immediately, not retransmitted, and not reordered with `Data`. A flow that
   rides one uplink is already ordered by that uplink; the inner TCP handles
   residual reordering. This avoids both nonce reuse and bonded-traffic latency.
4. **Reinjection:** the sender keeps a bounded retransmission window (proposed
   4 MiB), reinjects a packet that is unacknowledged past the RTO (proposed
   120 ms) on a *different* path, at most once per packet. Reinjection is
   counted as loss on the original path, which is what feeds the §6.3 score.
5. **Duplicate mode (§6.4 rule 5)** reuses the same sequence number on a second
   path with `FLAG_DUPLICATE`; the receiver deduplicates by sequence, so the
   inner stack sees one packet.
6. **Verification before buffering:** a frame that fails the AEAD check is
   dropped before it can occupy reorder-buffer memory (§9.1 adversary (b)).
7. **Hold-cap release needs a timer tick.** `Receiver::drain(now)` is part of
   the contract: without it, a flow whose missing packet never returns stalls
   until the peer happens to send again.
8. **Authenticate control before side effects.** ACK, probe, probe reply, and
   keepalive frames must pass AEAD verification, session checks, and a bounded
   per-kind replay window before they can update liveness, RTT, congestion, or
   capacity state. ECN marks are applied only after frame authentication.

## Consequences

- The lane can carry health, keepalive and capacity signalling without a second
  protocol or an ICMP dependency.
- Sequence spaces are separated by frame kind in the AEAD nonce; Data ordering,
  FlowData replay protection, keepalives, and probes do not create gaps in one
  another's counters. A version bump makes the nonce-domain change explicit.
- The receiver needs session state (cap, idle eviction, verify-first), which is
  also the seam where the Phase-4 handshake plugs in.
- v0 and v1 were in-development contracts. Current v2 remains an in-development
  contract; no compatibility promise is made before a release, and unknown
  versions are rejected rather than guessed.

## Wire v2 amendment (2026-10-20)

- Added frame kind `FlowAck` (wire kind 6), with an independent nonce domain.
  It selectively acknowledges one authenticated FlowData sequence; unlike the
  ordered Data ACK it is not cumulative, because FlowData is intentionally not
  reordered by the tunnel.
- FlowData receivers acknowledge fresh frames and resend the same selective ACK
  for authenticated duplicates. Send-side byte/path accounting is session-scoped,
  capped by the existing retransmission byte budget and a 4096-entry limit, and
  is removed on ACK, path loss, sleep/resume, or local rate-limit hold.
- Every finite per-kind nonce sequence (Data, FlowData, Keepalive, Probe) must
  fail closed before integer wrap. Exhaustion cannot reset or reuse a sequence;
  the sender stops emitting that frame kind under the current key.
- The service authenticates and replay-checks FlowAck before recording the
  acknowledged payload bytes for capacity estimation. Wire version 2 explicitly
  rejects v1 peers rather than silently losing ACK-based capacity samples.

## Rejected alternatives

- **Keep the 17-byte header and type the payload instead** — hides control
  frames inside data, so a keepalive would be sequenced and could stall a flow.
- **Reuse the MPTCP lane for control** — the datagram lane is the primary lane
  by directive (§4.8 rev 2) and must be self-sufficient.
- **One sequence space for both lanes** — simple, and wrong: it re-introduces
  head-of-line blocking across uplinks for traffic that was never striped.
