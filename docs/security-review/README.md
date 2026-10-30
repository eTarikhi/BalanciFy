# BalanciFy security-review package (draft)

**Prepared:** 2026-10-30. **Review status:** not independently reviewed; this package is a reviewer handoff, not a security approval, certification, or release assurance.

## Scope and provenance

The source baseline inspected at preparation was commit `528739405ef08404e48574808d5fcc513feef2bb` on `arena/fb5e8c41-balancify`. The worktree was not clean, and no application/crypto/driver source diff was present at that time. This draft describes source, not an immutable binary. For an actual review, record the final review commit, release SBOM identity and artifact hashes; do not assume this draft is bound to a later binary.

This package describes the repository's intended data/control paths and the source currently present. BalanciFy remains an engineering prototype. The first-party miniport, service, relay and shell have source/build evidence, but production installation, signed driver load, live packet path, HVCI, Driver Verifier, and hardware acceptance are not established.

## Documents

- [Architecture overview](architecture-overview.md)
- [Trust boundaries and threat model](trust-boundaries-threat-model.md)
- [IPC and cryptographic surface](ipc-and-crypto-surface.md)
- [Build and test instructions / evidence limits](build-and-test.md)
- [Draft VEX notes](vex-draft.md) and [OpenVEX-shaped draft scaffold](vex-draft.json)

## Tested vs. not tested

### Evidence already available (scope-limited)

- Hosted Rust workspace tests have passed on Ubuntu and Windows for the recorded source revisions. Hosted WDK x64/ARM64 jobs have passed for commits `5c866ea` and `6a3b633`; this is **hosted-runner build evidence, not isolated-VM acceptance**.
- The crypto source contains Noise_IK-style X25519/ChaCha20-Poly1305/BLAKE2s logic, a contributory-DH check, and unit/regression tests; unit/CI results are not an independent cryptographic review.
- The user supplied a Windows 11 local full-CI summary at HEAD `e63e1439b1ac2ad763e426e7dec249b6d013ee80`; all 12 named stages passed. The Admin harness was then fixed for invalid `$session:` interpolation (and a similar `$exit:` parse defect elsewhere). In the latest IPC output, Admin context failed because the shell was not elevated; Standard context passed, but all IPC connection attempts timed out. No service process/pipe evidence was supplied, so no product IPC bug is established.

### Not established

- Independent security review, formal verification, third-party cryptographic audit, or signed VEX.
- Production key provisioning/custody, production Windows service installation, supported installer lifecycle, signed/certified driver, runtime load, bidirectional packet path, HVCI/Verifier/HLK, clean-machine rollback, or real hardware/field behavior.
- A completed PSScriptAnalyzer or Windows hosted IPC result: Actions run [41038076670](https://github.com/eTarikhi/BalanciFy/actions/runs/41038076670) on `b147226` passed repository PowerShell parsing, then failed at PSScriptAnalyzer; the service build and hosted IPC steps were skipped. Analyzer findings are not available at this inspection. CI IPC checks are intentionally read-only and cannot prove standard-user authorization or admin policy writes.

Reviewers should treat undocumented assumptions and every `NOT RUN`/`BLOCKED` status as open. See `docs/completion-ledger.md` and `docs/release-gates-checklist.md` for the project gate ledger.
