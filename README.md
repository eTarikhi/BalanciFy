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

## Development status (audited 2026-10-26)

BalanciFy remains an engineering prototype, not a supported release. G1 has no
formal sign-off record in the repository; G2–G8 are open. Source coverage and
proposed exit criteria are not substitutes for Windows VM, hardware, signing,
review, or field evidence. See the [completion ledger](docs/completion-ledger.md)
for the prioritized work list and evidence classes.

| Area | Implemented / evidenced | Still unproven or incomplete |
|---|---|---|
| Rust service, IPC, tunnel and relay | Hosted Ubuntu and Windows Rust workspace tests pass; latest recorded CI at `b6f955e` reports 335 Rust tests on each platform, and Linux release build plus SPDX 2.3/3.0.1 generation/validation pass. Authenticated lab transport, IPC contracts, bounded packet handling, simulated path/failover tests and support-bundle redaction have automated coverage. | Production Windows service lifecycle, relay deployment/interoperability, service-owned durable machine policy, and real network behavior. |
| Windows UI | Native WinUI 3/.NET shell source contains Home/status, policy controls, custom Rules editor, redacted Relay health, Diagnostics/self-test/support export, and bounded per-user UI preferences. | The latest completed hosted run at `b6f955e` failed while building WinUI; its original log was not retained. The next CI run is being improved to expose compiler diagnostics. No Windows runtime, tray, accessibility, or usability acceptance is recorded. |
| Driver and OS integration | Hosted WDK CI compiles/links unsigned x64 and ARM64 drivers and checks PE mitigations/INF contracts; portable C packet/OID tests pass. Driver/service source includes bounded queue and injection paths. | No test-signed installation, Driver Verifier, HVCI/HLK, VM packet path, adapter steering across three classes, or operational miniport evidence. Compile/link is not runtime validation. |
| Installer, diagnostics and release | Rust installer journal/rollback and SBOM sidecar logic have unit coverage; CI generates and validates SPDX 2.3 and 3.0.1. The service has a bounded redacted support bundle and ETW source. | No production offline bundle, clean-machine install/uninstall/rollback run, live ETW collection, reviewed VEX, code signing/attestation, independent security review, GA distribution, or field observation. |
| Performance gates | Blueprint §10.3 defines the thresholds and lab harnesses include safe netem dry-run and software simulations. | No Rig B/hardware measurements. Aggregate/single-flow throughput, ≤3 ms added wired delay, ≤0.1% delivered reordering, ≤2 s failover, 72 h soak, 100 resume cycles and compatibility matrix remain acceptance targets, not results. |

Hosted CI at `b6f955e` passed dependency/crypto audit, WSL/Linux quick profile,
lab Python tests, Ubuntu/Windows Rust tests, x64/ARM64 WDK builds and Linux
release/SBOM jobs; only the native MSVC job failed at the WinUI build, so its
following native quick profile was skipped. A Linux quick profile run against source HEAD `44c89e8` with the pending
support-bundle validation edits in its worktree passed dependency audit, 5 lab
Python tests, 62 CI Python tests, netem safety and 12 portable driver tests; its
logs are retained at `target/local-ci/logs/20261026T011948Z/`. It intentionally
skips Rust/release/SPDX validation. These results are CI evidence only, not product acceptance. The
latest workflow runs `40253719333` (pull request) and `40253714360` (push)
for head `5df7959` were queued at 2026-10-26 01:20 UTC; they are not evidence
of a pass or failure. The revised Windows diagnostic workflow and native build
remain unvalidated on hosted CI.

The client/service binaries and installer remain lab components. Their
explicitly insecure test modes are debug-only and loopback-only. Persistent
identities use account-appropriate state roots; the shell's policy snapshot is
per-user UI state, not the service's durable machine configuration. See ADR-0035
and ADR-0038 for transport fail-closed behavior.

## Repository layout

```
crates/core     control plane: health state machine, link scoring + capacity
                estimation, WRH placement, classification rules, flow affinity,
                policy snapshot (§6.1), bond auto-disable, MTU/keepalive,
                per-path congestion windows coupled after RFC 6356
crates/tunnel   first-party datagram lane: authenticated wire v2, DRR striping,
                reorder buffer, bounded FlowData repair/ACK, flow-level lane
crates/ipc      versioned, length-checked IPC contract v1 and local transports
crates/service  sans-I/O service core: both schedulers off one policy snapshot,
                tunnel lane, and the IPC contract served for real
crates/relay    in-binary cooperating endpoint (server mode), lab form
driver/         first-party NDIS virtual miniport (WDK/Windows only)
app/            Rust lab CLI
windows/BalanciFy.App/  native WinUI 3 / .NET 10 product shell preview
installer/      transactional provisioning prototype; production bundle pending
tools/lab/      safe netem harness, analyzer, and acceptance tooling
tools/ci/       local profiles, source-contract tests, audits and SBOM tooling
docs/           blueprint, ADRs, phase artifacts and completion ledger
.github/        CI: Linux/Windows Rust, WinUI/MSVC, WDK, SBOM, audits and lab tests
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

Driver, shell and installer build on Windows only (see their READMEs). For complete
machine setup, local CI, safe push-after-green behavior, WDK builds, and remote-run
monitoring, see [Local development and CI/CD](docs/local-development-and-ci.md).

For the prioritized software checklist, evidence ledger and external blockers, see
[`docs/completion-ledger.md`](docs/completion-ledger.md) and the dated phase notes
under [`docs/phase2/`](docs/phase2/).

## Licence

Blueprint and docs: CC0 (see `LICENSE`). Product code: proprietary (A5).
