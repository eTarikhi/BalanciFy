# ADR-0002 — Dependency policy: fully self-contained product (rev 2.1)

**Status:** Accepted (owner directive 2026-10-17) · **Supersedes:** parts of ADR-0001 rev 1

## Decision
Every runtime component of Balancify is (a) first-party code, (b) a vendored
fork maintained in-house under a permissive licence, or (c) a vetted pinned
cryptography crate. The installer provisions the complete working environment
on a clean offline machine (§4.8 rev 2.1): driver, service, embedded endpoint
components, bundled GUI runtimes, with atomic rollback. Distribution channels
(winget, Store) are never dependencies.

## Consequences
- First-party NDIS miniport from Phase 2; WHCP attestation + HVCI on the
  critical path (roadmap rev 2).
- Wintun/WinDivert/OpenMPTCProuter/Speedify/Peplink excluded as runtime
  components; single time-boxed exception (§11.2) for schedule rescue.
- mptcp-rs retained only as an optional vendored module (standard interop).
- CI gains a dependency audit (SBOM diff) each release; PassThrough crypto is
  banned from shipped builds by audit.

## Rejected alternatives
- Bundling third-party signed kernel binaries (loss of control over signing/updates).
- Relying on external bonding services (violates directive; egress/privacy cost).
- Store-delivered runtimes (violates "no other app/solution" criterion).
