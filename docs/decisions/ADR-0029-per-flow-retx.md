# ADR-0029: Per-flow retx keyed by (session, seq) + per-session seq spaces

Date: 2026-10-19
Status: accepted

## Context

Previously, `Sender` in `crates/tunnel/src/lib.rs` used global unique seq across sessions to keep retx simple:

- `next_seq: u64` global
- `session_seqs: BTreeMap<u64, u64>` tracked per-session for telemetry but not used for retx key
- `retx: BTreeMap<u64, RetxEntry>` keyed only by seq, not (session, seq)
- `next_seq_for(session)` returned global unique, incremented both global and per-session

This simplified retx but meant per-flow sessions (Sec.4.7.4) shared sequence space, risking collisions if seq spaces overlapped, and made true per-session sequencing impossible (ADR-0011 noted future per-session seq spaces).

Per blueprint Sec.4.7.4, per-flow sessions should have independent seq spaces for true per-session demux, with retx keyed by (session, seq).

## Decision

- `Sent` struct now includes `session: u64` field (previously only path, seq, kind, bytes)
- `RetxEntry` now includes `session: u64`
- `Sender` fields:
  - Remove `next_seq: u64` global
  - `retx: BTreeMap<(u64, u64), RetxEntry>` keyed by (session, seq)
  - `session_seqs: BTreeMap<u64, u64>` remains, now authoritative for per-session seq
- Methods:
  - `next_seq_for(session) -> u64`: returns per-session seq, increments per-session counter (true per-session space)
  - `next_seq() -> u64`: returns next seq for default session (`self.session`) via session_seqs
  - `next_seq_for_session(session) -> u64`: public accessor for per-session next seq
  - `remember(session, seq, plain, path, now)`: inserts into retx keyed by (session, seq)
  - `forget_path(path)`: filters retx by path, collects keys (session, seq) where path matches
  - `on_ack(up_to, now)`: delegates to `on_ack_for_session(self.session, up_to, now)` for backward compat
  - `on_ack_for_session(session, up_to, now)`: collects keys in range (session,0)..=(session,up_to), removes, returns per-path bytes
  - `reinjections_due(now)`: collects due keys (session, seq) where reinjections < max and sent_at >= rto, reinjects via `frame_with_session` preserving session
  - `send_excluding(plain, now, blocked)`: delegates to `send_excluding_with_session(plain, self.session, now, blocked)`
  - `duplicate(plain, seq, path)`: delegates to `duplicate_with_session(plain, self.session, seq, path)` for backward compat
  - `duplicate_with_session(plain, session, seq, path)`: new, uses `frame_with_session` with session
  - `send_flow_with_session`: includes session in Sent
  - `send_excluding_with_session`: uses `next_seq_for(session)`, `remember(session, seq, ...)`
  - `keepalive`, `probe`, `probe_reply`, `ack`, `ack_with_flags`, `ack_for_session`, `ack_with_ecn_for_session`: all include session in Sent
- Service integration (`crates/service/src/lib.rs`):
  - `send_app_packet`: when duplicate_mode, if per-flow/per-class sessions enabled, use `duplicate_with_session(payload, sess, sent.seq, other)` else `duplicate`
  - `ingest_path`: Ack handling uses `on_ack_for_session(hdr.session, hdr.seq, now)` instead of `on_ack(hdr.seq, now)`
- Tests: existing tunnel tests still pass because they use default session; new per-session behavior covered by existing per-flow session tests in service

## Consequences

- Positive: true per-session seq spaces, retx keyed by (session, seq) per ADR-0011, enables per-flow session demux without collision, aligns with RFC 8684 Sec.3.3 subflow vs connection-level ack separation
- Negative: `Sent` now has 5 fields, but all call sites updated; `on_ack` old signature kept for backward compat but delegates to default session
- Neutral: no change to wire format (header already has session), only internal retx keying

## References

- Blueprint Sec.4.7.4 per-flow sessions, Sec.6.4 DRR + retx, ADR-0011 per-session sequencing, RFC 8684 Sec.3.3
- Code: `crates/tunnel/src/lib.rs` `Sender`, `Sent`, `RetxEntry`, `crates/service/src/lib.rs` `send_app_packet`, `ingest_path`
- CI: 4d05142 success (ubuntu+windows+audits)
