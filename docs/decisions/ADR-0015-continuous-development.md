# ADR-0015: Continuous development without stopping

Date: 2026-10-19
Status: accepted (process)

## Context

User instruction: "continue the development process without stopping to report" — meaning the agent should keep building features, fixing bugs, and pushing CI, not stopping after each small change to report.

Previously, the agent would report after each feature, causing many interruptions. The user wants continuous development.

## Decision

- Implement features in batches, but keep pushing CI and fixing bugs continuously.
- No need to report after each commit — only report when user explicitly asks or when blocked.
- Keep CI green at all times: each push must pass ubuntu+windows+audits.
- Continue roadmap:
  - ECN+delay CC (done)
  - Per-flow sessions (done)
  - Per-class sessions (done)
  - Rate limiting service egress (done)
  - Flow-level capacity (done)
  - Wintun fallback ADR-0009 (done)
  - Installer transactional (done)
  - App shell IPC client (done)
  - ETW stub + support bundle enhanced (done)
  - Throughput harness + benches (done)
  - SBOM + clean-machine test docs (done)
  - Per-session seq tracking (done)
  - Relay ECN echo (done)
  - Crypto bench (done)
  - DRR + pacing benches (done)
- Next:
  - Relay stats for ECN/delay
  - More ADRs
  - Security hardening (zeroize, fuzz)
  - GUI WinUI real implementation (Phase 4)
  - Driver WDF control device + IOCTLs
  - Installer MSIX + MSI bundle
  - Real Rig B measurements

## Consequences

- Positive: continuous progress, CI always green, user sees steady development.
- Negative: more commits, more CI runs, but that's okay.
- Neutral: no change to product policy.

## References

- User instruction 2026-10-19: "continue the development process without stopping to report?"
- Blueprint §10.3 acceptance rows, §4.8 dependency policy.
