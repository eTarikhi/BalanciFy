# ADR-0005: Per-path congestion control, coupled across paths

- Status: **Accepted** (2026-10-17)
- Relates to: blueprint §6.4 rule 4, §6.3, §10.3 (throughput rows), ADR-0003 (wire v0)

## Context

Blueprint §6.4 rule 4 requires "congestion control per path inside the tunnel
(independent loss/ACK clocks)". Until now the tunnel had none: DRR quanta came
from the capacity estimate alone, so nothing stopped the sender from putting an
unbounded amount of data on a path that was already losing packets. Three
consequences:

1. §10.3's aggregate and single-flow throughput rows are meaningless without a
   window — the number measured would be the rig's buffer, not the link.
2. A lossy path keeps being fed at its scheduled share, so reinjections pile up
   behind the losses they were meant to fix.
3. The tunnel is over UDP on purpose (§6.4 rule 4) so the inner TCP keeps its
   own loss recovery. That only holds if the tunnel does not itself overrun the
   path; otherwise the inner stack sees loss it did not cause and TCP-over-TCP
   meltdown returns through the back door.

## Decision

**Coupled per-path windows, implemented in `crates/core/src/congestion.rs` as
`CongestionController`, owned by the service.**

- **Algorithm: LIA (Linked Increases, RFC 6356).** Below `ssthresh` the window
  grows by the bytes acknowledged (slow start, one doubling per RTT). Above it,
  each acknowledgement on path *i* increases `cwnd_i` by

  ```
  min( alpha * bytes_acked * mss / tot_cwnd ,  bytes_acked * mss / cwnd_i )
  ```

  with `alpha` = number of active paths. The coupling term is the point: the
  *aggregate* window grows no faster than a single-path flow would on a shared
  bottleneck, so a bonded flow does not take N times a fair share. Measured in
  `the_coupled_increase_is_no_faster_than_a_single_path`: over 40 RTTs a single
  path grows 56 000 B and the same volume split over two paths grows 55 608 B.
  Uncoupled per-path NewReno would be about twice that.
- **Loss signal: a reinjection.** The sender already declares a packet lost
  when its RTO expires and re-sends it elsewhere; that path's window halves.
  A burst halves the window at most once per **100 ms** (proposed), because a
  window of losses is one congestion event.
- **Floor: 2 segments** (proposed), so a path can always make progress; initial
  window **10 segments** (RFC 6928).
- **Units: payload bytes.** An acknowledgement can only account for what the
  retransmit table holds, which is the plaintext. The framing and AEAD tag
  overhead (~1.6 % at MTU) is therefore not charged — a deliberate under-count,
  chosen over an inconsistency: the first implementation charged the sealed
  frame on send and the plaintext on acknowledge, and every packet left 20
  phantom bytes in flight until the window stopped reopening.
- **Backpressure, not drops.** When every eligible path is inside its window,
  `send_app_packet` returns `Action::Hold { flow, data }`. The packet was not
  refused (that is decision D3's health case) and not dropped; the miniport
  requeues it. `Action::Hold` is a distinct variant so the caller cannot
  confuse "congested" with "broken".
- **Layering.** The controller lives in `core`; the tunnel only learns which
  paths are blocked (`Sender::send_excluding`, `DeficitRoundRobin` with a
  blocked set) and reports losses (`Sender::drain_losses`). `crates/tunnel`
  keeps zero dependencies.
- **The flow-level lane is not congestion-controlled.** Flow-level packets are
  not sequenced and not retransmitted (§6.2), so there is no ACK clock to run a
  window on. That is a documented gap, not an oversight.

## Consequences

- A lossy uplink sheds load to the others instead of being fed at its scheduled
  share, and the shift is visible in telemetry (`Service::window`,
  `Service::in_flight`).
- Sending is now paced by acknowledgements, so the rig (and the miniport) must
  model a queue. `crates/service/tests/e2e.rs` does: a held packet is retried on
  a later step.
- All constants are the blueprint's *proposed* starting values, lab-tunable and
  never measurements.

## Rejected alternatives

- **Independent NewReno per path.** Simple, and wrong in the common case: two
  paths over one access network would take twice a single flow's share.
  Rejected on fairness, which §10.3 would otherwise have to explain away.
- **OLIA / LIA-with-different-alpha.** OLIA is provably fair under a wider set
  of conditions but needs per-path loss-rate estimates over time. LIA is the
  conservative starting point; switching is a contained change to one formula
  and can be decided from S2c data.
- **A model-based controller (BBR-style).** Needs a delivery-rate estimator and
  a min-RTT tracker; we have per-path RTT EWMA but no rate sampling yet. Worth
  revisiting once S2c produces real delay/loss data — the seam is
  `on_ack(path, bytes, now)`, which already takes `now` for exactly this.
- **Letting the inner TCP handle everything.** Defeated by striping: the tunnel
  reorders, reinjects and occasionally drops, so the inner stack's loss signal
  is not the path's. This is the TCP-over-TCP failure mode §6.4 rule 4 exists to
  avoid.
- **A fixed per-path rate limit.** Ignores actual congestion and would cap a
  healthy link at whatever number we guessed.

## Open items

- `alpha` is the active path count; a path that is blocked but present still
  counts. Revisit if that proves to over-couple.
- The window is not yet in the IPC contract; the shell sees counters, not
  windows. Adding it means a `Counters`/`UplinkInfo` field and a schema note.
- No ECN, no delay term, no pacing (bursts up to a full window can leave at
  once). The §10.3 "added delay and jitter" row is where that will show up.
