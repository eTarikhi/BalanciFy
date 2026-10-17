# balancify

Windows-native multi-WAN **bonding + load balancing**: combine Wi-Fi, Ethernet
and cellular behind one virtual adapter, with every capability embedded in the
product — no external apps, services or solutions at runtime; the installer
provisions the whole working environment (dependency policy rev 2.1).

## Technical blueprint

[`docs/technical-blueprint.md`](docs/technical-blueprint.md) — engineering
decision document (rev 2.1): methodology comparison, fully self-contained
architecture (§4.8), embedded cooperating endpoint (§4.7), installer-provisioned
environment (§4.8 rev 2.1 / §8.3), security & signing plan, validation plan,
**roadmap rev 2** with criteria-coverage verification (§11.5), skills matrix,
risk register, sourced assumptions. Diagrams in [`docs/assets/`](docs/assets/).
Decisions: [`docs/decisions/`](docs/decisions/). Phase 1: [`docs/phase1/`](docs/phase1/).

## Development status (started 2026-10-17)

| Phase | Status |
|---|---|
| 1 Feasibility | **In progress** — decision memo, ADRs, cost-model template delivered; owner sign-off pending (G1) |
| 2 Lab prototype | **Started** — first-party core crates with passing unit tests (health state machine, WRH placement, reorder buffer, DRR); relay server-mode lab binary; netem rig + S1 steering script; NDIS miniport skeleton (WDK build on Windows) |
| 3–9 | Planned per roadmap rev 2 |

## Repository layout

```
crates/core     control plane: health state machine, link scoring, flow placement (WRH)
crates/tunnel   first-party datagram lane: framing, sequencing, reorder buffer, DRR
crates/relay    in-binary cooperating endpoint (server mode), lab form
driver/         first-party NDIS virtual miniport (WDK/Windows only)
app/            WinUI 3 / .NET 10 desktop shell (skeleton)
installer/      transactional offline installer design + WiX skeleton
tools/lab/      Rig B (netem), Spike S1 steering evidence script
docs/           blueprint, ADRs, phase artifacts
.github/        CI: Rust workspace tests on Linux + Windows runners
```

## Build & test (core, cross-platform)

```
cargo test --workspace      # control plane + tunnel lane + relay reassembly
```

Driver, shell and installer build on Windows only (see their READMEs).

## Licence

Blueprint and docs: CC0 (see `LICENSE`). Product code: proprietary (A5).
