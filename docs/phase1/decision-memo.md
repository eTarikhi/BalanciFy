# Phase 1 decision memo (G1 input)

Date: 2026-10-17 · Author: architecture · Status: proposed defaults pending owner sign-off

## Open decisions and proposed defaults (blueprint §13.2)

| ID | Decision | Proposed default | Rationale |
|---|---|---|---|
| D1 | Single-flow bonding hard requirement? | Yes, opt-in per flow class | Directive names bonding as core feature; fallback class policy protects heterogeneous links |
| D2 | Operate relays? | Yes — as **in-binary server mode** (T1) in our cloud + user self-host | Directive: nothing external; server mode is product code (§4.7) |
| D3 | Fail-open vs fail-closed when all paths/endpoint down | Fail-closed for tunneled, fail-open for flow-balanced | §9.2 safe defaults |
| D4 | Windows 10 support | Best-effort only | Driver/HVCI matrix cost |
| D5 | Closed source | Closed source, open docs | A5 |
| D6 | Tunnel wire | First-party datagram lane + optional vendored MPTCP module | §4.7/§4.8 rev 2 |

## Signing strategy (G1 artifact, roadmap rev 2 Phase 1)

- **Attestation signing first** (desktop Win10/11): EV certificate ordered week 1,
  Partner Center enrolment started week 1; dry-run submission at G3; final at G7.
- **HLK/WHCP only if** Windows Server support enters scope (not in v1).
- Code-signing for app binaries: Azure Artifact Signing (short-lived certs +
  timestamping) with OV certificate as documented fallback (§7).
- SBOM (SPDX 3.0) + VEX generated in CI from the first commit (03/2027 rule, §9.4).

## Relay/infra cost model pointer
See `relay-cost-model.md` (template; numbers to fill from two candidate regions
during Phase 1 — labelled proposed until measured).

## Licence review status
- mptcp-rs vendored module: Apache-2.0/MIT — compatible with D5 (counsel confirm Phase 1).
- Wintun/WinDivert: excluded by ADR-0002; analysed only for the §11.2 exception.
