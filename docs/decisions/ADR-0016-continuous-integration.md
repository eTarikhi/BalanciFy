# ADR-0016: Continuous integration with ubuntu+windows+audits

Date: 2026-10-19
Status: accepted

> **SBOM correction (2026-10-20):** the SPDX 3.0 JSON previously emitted by the
> shell stub was not standards-valid and included placeholder VEX claims. It is
> superseded by `tools/ci/sbom.py`, which generates an SPDX 2.3 locked-graph
> inventory only. SPDX 3.0/VEX remain pending; see ADR-0031.

## Context

CI must validate:
- ubuntu + windows tests (same 211+ tests)
- dependency + crypto policy audit (allowlist + exact-pin, PassThrough cannot ship)
- SBOM generation (S13)
- No third-party kernel component in data path (C2)

Previously, CI ran `cargo test` which stops at first failing binary, hiding relay and service failures behind earlier core failure. Also, no per-binary counts on success.

## Decision

- CI runs `cargo test --no-fail-fast` and publishes per-binary counts on success as well as failure (already implemented in `.github/workflows/ci.yml`)
- Audit: `tools/ci/audit-deps.sh` checks allowlist + exact-pin, fails on anything not on list, checks PassThrough literal, checks Cargo.lock if present
- Historical SBOM stub was handwritten and invalid; current `tools/ci/sbom.sh` delegates to the SPDX 2.3 locked-graph generator. Future work is standards-valid SPDX 3.0 and reviewed VEX.
- Each push must be green on both ubuntu and windows + audit
- Use `gh run list --limit 1 --branch <branch> --json databaseId,conclusion` + `check-runs` to verify, with 115s wait for CI to start

## Consequences

- Positive: no hidden failures, same tests on both platforms, policy enforced
- Negative: more CI time, but necessary
- Neutral: no change to product code

## References

- Blueprint §4.8 dependency policy, §9.1b tampered input refused, C2/C4
- Code: `.github/workflows/ci.yml`, `tools/ci/audit-deps.sh`, `sbom.sh`
