# ADR-0004 — IPC contract v1: first-party framing, not JSON

**Status:** Accepted (Phase 3, milestone M6) · **Related:** ADR-0002, blueprint §8.1, §9.2

## Context

The shell and the service are separate processes at different privilege levels
(§4.4 B1). §9.2 requires the pipe to be ACL'd, the caller's identity to be
validated on connect, and the **message schema to be versioned and
length-checked**. The shell must never open a socket or touch the driver
(§8.1), so this contract is the shell's entire view of the product.

The obvious implementation — JSON over the pipe with `serde_json` — collides
with ADR-0002: a JSON library is neither first-party, nor a vendored fork we
maintain, nor a vetted cryptography crate.

## Decision

1. **First-party binary framing**, 6-byte header:
   `version u8 (=1) | kind u8 | payload length u32 BE`, payload capped at
   1 MiB. One decode entry point returns `need more bytes` for a partial frame
   and an error for a bad version, unknown kind or over-long length. A corrupt
   stream drops the connection; it is never partially parsed.
2. **Three message namespaces** — request (opcode), response (tag), event
   (tag) — deliberately numbered independently so a misaligned stream cannot
   turn a response into a plausible request.
3. **No floats and no unbounded nesting on the wire.** Durations are
   milliseconds, ratios are per-mille, weights are per-mille of the best
   uplink. Fixed-width, big-endian, explicit counts with a ceiling
   (256 uplinks, 4096 rules).
4. **Counters only** (§9.2 privacy rule): the contract has no field in which a
   destination, hostname or payload could be expressed. New warning conditions
   are numeric codes, so an older shell degrades instead of breaking.
5. The contract is **transport-independent**: the same bytes run over the
   Windows named pipe and over a socket pair in CI on Linux, which is why the
   shell protocol is testable before the Windows shell exists.

## Consequences

- Zero third-party runtime dependencies in `balancify-ipc`; the crate has no
  `[dependencies]` at all.
- Every message has a hand-written encoder/decoder pair, so each needs a
  round-trip test — the crate's test suite is the schema's conformance suite,
  including truncation at every offset.
- The .NET shell gets a small hand-written decoder instead of
  `System.Text.Json` attributes. That is a real cost, paid once, in exchange
  for the dependency policy holding without an exception.
- Schema changes require a version bump and a compatibility statement, not a
  silent field addition.

## Rejected alternatives

- **JSON + serde_json** — readable and free on the .NET side, but adds a
  third-party runtime dependency the policy does not allow, and brings
  unbounded-depth parsing onto a privileged boundary.
- **Cap'n Proto / FlatBuffers / protobuf** — same policy problem, larger.
- **COM / WinRT ABI** — Windows-only, so the contract could not be tested on
  the Linux CI runners, and it couples the shell to a Windows-only surface
  that the driver-free MVP does not otherwise need.
- **Reusing the tunnel framing for IPC** — different trust boundary, different
  lifetime rules, and no reason to share a version counter.
