# ADR-0009: Wintun fallback exception (§11.2)

Date: 2026-10-19
Status: accepted (exception policy)
Deciders: product owner, driver lead

## Context

The product policy (§4.8) forbids third-party kernel components in the data path — only NDIS + WDF. The driver must be attestation-signed and pass HVCI readiness trio (Driver Verifier + HLK HVCI readiness + VBS/Memory-Integrity machine, S15).

If signing/HVCI blocks G3/G7 >4 weeks, the product cannot ship bonding, but flow-level load balancing could still ship with a bundled Wintun signed DLL (single allowed third-party runtime artifact, disclosed, per §11.2 exception).

## Decision

- Allow **one** exception: bundled Wintun (Wintun.dll, signed by WireGuard) as flow-level only fallback.
- Conditions:
  - G3 or G7 blocked >4 weeks by signing/HVCI, documented in `docs/phase2/lab-progress.md` with dates and blocker.
  - Product owner approves in writing, exception recorded in `docs/exceptions/`.
  - Bundled DLL must be pinned exact version, SBOM/VEX entry, disclosed in installer and About screen.
  - Must be retired next minor release — bonding + first-party driver continues in parallel.
  - Flow-level only: no striping, no reorder, no DRR, no congestion coupling across paths — WRH placement only.
  - Must still enforce dependency policy: Wintun is the only third-party kernel-adjacent runtime allowed, and only in this exception.
- Implementation:
  - Feature flag `wintun-fallback` (off by default), compile-time gated.
  - `crates/service/src/wintun.rs` stub (this ADR) — loads Wintun.dll via raw FFI, creates adapter, reads/writes packets, no third-party Rust crate.
  - Installer: if exception active, bundle Wintun.dll, install with same journaled rollback.
  - IPC: `StatusSnapshot` includes `wintun_fallback_active` bool, surfaced as warning `WARN_VPN_COEXISTENCE` or new `WARN_WINTUN_FALLBACK`.
  - Audit: `tools/ci/audit-deps.sh` allows Wintun.dll only when exception file exists and feature enabled.

## Consequences

- Positive: product can ship flow-level value even if driver signing stalls.
- Negative: third-party kernel-adjacent code, even if signed and isolated, increases attack surface and must be retired quickly.
- Neutral: no change to bonding path — first-party NDIS miniport remains primary.

## References

- Blueprint §11.2 exception policy, §4.8 dependency policy, §9.5 HVCI readiness trio, S15.
- Wintun: https://www.wintun.net/ — MIT license, signed DLL, used by WireGuard.
