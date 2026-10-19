# balancify

[![CI](https://github.com/eTarikhi/balancify/actions/workflows/ci.yml/badge.svg)](https://github.com/eTarikhi/balancify/actions/workflows/ci.yml)

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
| 2 Lab prototype | **In progress** — control plane (health, scoring/capacity, WRH placement, rules, flow affinity, bond auto-disable, MTU/keepalive, coupled per-path congestion control, pacing **wired into Service (80% cap, delay exposed)**, RFC 8439 AEAD payload protection, Noise_IK session establishment with pinning enforced and replay protection, key management with **DPAPI via first-party FFI (no external crate, TPM-backed where available)**, rekey) and first-party datagram lane (wire v0, DRR striping, reorder, reinjection, flow-level lane, backpressure, sleep/resume and NAT re-binding) with unit, simulated end-to-end and deterministic fuzz tests (193 tests, 0 failures on ubuntu/windows); relay server-mode lab binary with secure endpoint (pinning, replay, auto_accept=false, `establish_secure_session`); netem rig + S1 steering script; NDIS miniport skeleton (WDK build on Windows). **Not yet proven:** S2c on the netem rig (real measurements), S2b miniport in the loop, Cargo.lock committed, server-mode binary secure wiring (handshake + forwarding), throughput rows on Rig B |
| 3 Architecture spike | **Started** — IPC contract v1 delivered and tested (milestone M6, ahead of the driver work it was sequenced behind); miniport perf/HVCI/signing dry-run not started |
| 4–9 | Planned per roadmap rev 2 |

## Repository layout

```
crates/core     control plane: health state machine, link scoring + capacity
                estimation, WRH placement, classification rules, flow affinity,
                policy snapshot (§6.1), bond auto-disable, MTU/keepalive,
                per-path congestion windows coupled after RFC 6356
crates/tunnel   first-party datagram lane: wire v0 frame kinds, DRR striping,
                reorder buffer, reinjection, flow-level lane, receiver
crates/ipc      IPC contract v1 (service ⇄ shell): versioned, length-checked,
                first-party framing — no serialisation dependency
crates/service  sans-I/O service core: both schedulers off one policy snapshot,
                tunnel lane, and the IPC contract served for real
crates/relay    in-binary cooperating endpoint (server mode), lab form
driver/         first-party NDIS virtual miniport (WDK/Windows only)
app/            WinUI 3 / .NET 10 desktop shell (skeleton)
installer/      transactional offline installer design + WiX skeleton
tools/lab/      Rig B (netem), Spike S1 steering evidence script
tools/ci/       dependency + crypto policy audit (ADR-0002, criteria C2/C4)
docs/           blueprint, ADRs, phase artifacts
.github/        CI: policy audit + Rust workspace tests on Linux and Windows
```

## Build & test (core, cross-platform)

```
cargo test --workspace      # control plane, tunnel lane, IPC contract, service
                            # core, relay reassembly, simulated end-to-end paths
sh tools/ci/audit-deps.sh   # dependency + crypto policy audit (ADR-0002)
```

The Rust workspace is first-party apart from **six pinned third-party runtime
dependencies**: `chacha20poly1305 =0.11.0` (RFC 8439 AEAD, ADR-0006), `x25519-dalek =2.0.1`,
`blake2 =0.10.6`, `hkdf =0.12.4` (Noise_IK, ADR-0007), `getrandom =0.2.15`,
`zeroize =1.8.1` (key management, ADR-0008; §4.8 forbids rolling our own primitives).
The audit above enforces both halves — anything off
[`tools/ci/crypto-allowlist.txt`](tools/ci/crypto-allowlist.txt), or on it but
not exactly pinned, fails the build. Lab rigs still need the null cipher
explicitly — `cargo run -p balancify-relay --features lab-crypto -- 4430,4431` —
and a release build of the relay refuses to start without one.

Driver, shell and installer build on Windows only (see their READMEs).

Phase-2 status and the honest list of what is *not* yet proven:
[`docs/phase2/lab-progress.md`](docs/phase2/lab-progress.md).

## Licence

Blueprint and docs: CC0 (see `LICENSE`). Product code: proprietary (A5).
