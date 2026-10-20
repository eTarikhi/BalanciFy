# ADR-0011: Per-session sequence tracking

Date: 2026-10-19
Status: accepted

## Context

Per-flow sessions (§4.7.4) originally shared a single global sequence counter (`Sender::next_seq`). This meant seq 0 would appear in multiple sessions if per-flow sessions were enabled, causing retx table collisions (BTreeMap keyed by seq only). The receiver demuxes by session id, so same seq in different sessions is fine for reorder, but sender's retx would overwrite.

## Decision

- Add `session_seqs: BTreeMap<u64, u64>` to `Sender` — tracks per-session seq count for telemetry.
- Keep global `next_seq` as unique across sessions for retx simplicity — `next_seq_for(session)` returns global unique seq but also increments per-session counter.
- Future: retx could be keyed by (session, seq) to allow true per-session seq spaces starting at 0. For now, global unique is simpler and avoids breaking `on_ack` which uses seq only.
- Document that per-flow sessions share global seq space for now, but telemetry shows per-session counts.

## Consequences

- Positive: no retx collision, per-session telemetry available, backward compat.
- Negative: seq numbers are not per-session 0-based, but that's okay because session id separates reorder buffers.
- Neutral: future work can change retx to (session, seq) if needed.

## References

- Blueprint §4.7.4 per-flow sessions, §6.4 retx window bounded.
- Code: `crates/tunnel/src/lib.rs` `session_seqs`, `next_seq_for`.
