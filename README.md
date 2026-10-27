# balancify

[![CI](https://github.com/eTarikhi/balancify/actions/workflows/ci.yml/badge.svg)](https://github.com/eTarikhi/balancify/actions/workflows/ci.yml)

Windows-native multi-WAN **bonding + load balancing**: combine Wi-Fi, Ethernet
and cellular behind one virtual adapter, with every capability embedded in the
product — no external apps, services or solutions at runtime; the installer
provisions the whole working environment (dependency policy rev 2.1).

## Quick start — Windows 11 x64

### Required development tools

- Windows 11 x64 and **Developer PowerShell for Visual Studio 2022**.
- Visual Studio 2022 (17.x) with **Desktop development with C++**, MSVC v143 x64/x86 tools, and MSBuild.
- Windows SDK **10.0.26100.0** and WDK **10.0.26100.6584** for the driver build (x64 and ARM64 support).
- Stable Rust for `x86_64-pc-windows-msvc`, including Cargo.
- **.NET 10 SDK** and WinUI 3/Windows App SDK tooling; the app restores the pinned `Microsoft.WindowsAppSDK` 2.5.1 package.
- Python **3.10+** with the `py -3` launcher (CI uses Python 3.12).
- Git for Windows, including Git Bash (`bash.exe`/`sh`). PowerShell is included with Windows.

WSL, Docker, and Node.js are not required for Windows development. GitHub CLI (`gh`) is optional for local work and is only needed for push/PR automation.

### Build and test

Open Developer PowerShell for VS 2022 at the repository root, then run:

```powershell
# Fast local checks (does not build Rust, WinUI, or the WDK driver)
powershell -NoProfile -ExecutionPolicy Bypass -File tools\ci\local-ci-windows.ps1 -Quick

# Full native Windows validation: WinUI, Rust, SBOM, and x64/ARM64 WDK builds
powershell -NoProfile -ExecutionPolicy Bypass -File tools\ci\local-ci-windows.ps1
```

To build and run the WinUI app directly:

```powershell
cd windows\BalanciFy.App
dotnet restore
dotnet build -c Debug -r win-x64
dotnet run -c Debug -r win-x64
```

### Local deployment limitation

The local WDK build produces an **unsigned** driver. The installer’s driver/service installation steps are still prototypes, so this repository does not yet provide a supported production installation path. Do not install or load the unsigned driver on your everyday PC; driver deployment/runtime testing requires an isolated Windows VM and separate test-signing setup. See [native Windows development and CI](docs/local-development-and-ci.md) for the detailed checklist and validation limits.

## Technical blueprint

[`docs/technical-blueprint.md`](docs/technical-blueprint.md) — engineering
decision document (rev 2.1): methodology comparison, fully self-contained
architecture (§4.8), embedded cooperating endpoint (§4.7), installer-provisioned
environment (§4.8 rev 2.1 / §8.3), security & signing plan, validation plan,
**roadmap rev 2** with criteria-coverage verification (§11.5), skills matrix,
risk register, sourced assumptions. Diagrams in [`docs/assets/`](docs/assets/).
Decisions: [`docs/decisions/`](docs/decisions/). Phase 1: [`docs/phase1/`](docs/phase1/).

## Development status (audited 2026-10-27)

BalanciFy remains an engineering prototype, not a supported release. PR #20 is
merged; the active continuation branch is `arena/fb5e8c41-balancify`. The last
fully green hosted revision is [`e3e406b`](https://github.com/eTarikhi/BalanciFy/commit/e3e406b0c4e5b5625b9cf614a96389dcb3f9cd6f),
whose [CI run 40433764565](https://github.com/eTarikhi/BalanciFy/actions/runs/40433764565)
passed all eight jobs. The newer pushed revision [`13c58ff`](https://github.com/eTarikhi/BalanciFy/commit/13c58ff17a3fdd827ac7bf2b5bd406d1f57577ac)
adds a global authenticated tunnel reorder-memory ceiling. In its CI run
`40444227523`, Linux/Windows Rust tests, release build/SPDX checks, WSL, lab, and
dependency audit have passed; native MSVC and WDK x64/ARM64 were still running at
the latest query. Source coverage and proposed exit criteria
are not substitutes for Windows VM, hardware, signing, review, or field
evidence. See the [completion ledger](docs/completion-ledger.md) for prioritized
work and evidence classes.

| Area | Implemented / evidenced | Still unproven or incomplete |
|---|---|---|
| Rust service, IPC, tunnel and relay | Hosted Ubuntu and Windows workspace tests, Linux release build, SPDX 2.3/3.0.1 validation, native Windows quick checks, WSL parity, lab/dependency audits, and WDK x64/ARM64 builds passed at `e3e406b` in run `40433764565`. The service retains and retries transient inbound miniport-not-ready packets; the current `13c58ff` patch bounds authenticated reorder retention. In run `40444227523`, Rust tests passed on Ubuntu and Windows, as did release/SPDX, WSL, lab and dependency checks. | At the latest query native MSVC and WDK x64/ARM64 were still running for `13c58ff`. Production Windows service lifecycle, relay deployment/interoperability, and real network behavior remain unvalidated. |
| Windows UI | Hosted WinUI 3/.NET Release build and native Windows quick validation passed at `e3e406b`. Source includes Home/status, policy controls, custom Rules editor, redacted Relay health, Diagnostics/self-test/support export, and explicit one-time legacy policy migration. | No interactive Windows UI, tray, accessibility, usability, or OS-integration acceptance is recorded. |
| Driver and OS integration | Hosted WDK x64/ARM64 builds and PE architecture/mitigation checks passed at `e3e406b`; portable C packet/OID tests pass. Source includes initialization gating, bounded queues, injection ownership, and receive-filter checks. | Driver artifacts are unsigned. No test-signed installation, Driver Verifier, HVCI/HLK, VM packet path, adapter steering across three classes, or operational miniport evidence. Compile/link is not runtime validation. |
| Installer, diagnostics and release | Rust installer journal/rollback and SBOM sidecar logic have unit coverage; CI generates and validates SPDX 2.3 and 3.0.1. The service has a bounded redacted support bundle and ETW source. | No production offline bundle, clean-machine install/uninstall/rollback run, live ETW collection, reviewed VEX, code signing/attestation, independent security review, GA distribution, or field observation. |
| Performance gates | Blueprint §10.3 defines the thresholds; lab tooling includes safe netem dry-run and software simulations. | No Rig B/hardware measurements. Aggregate/single-flow throughput, ≤3 ms added wired delay, ≤0.1% delivered reordering, ≤2 s failover, 72-hour soak, 100 resume cycles, and compatibility targets are not results. |

The former native-Windows Python path failures are resolved in hosted CI run
`40433764565`. The Pages workflow's site build passed in run `40377552901`, but
GitHub's `Setup Pages` step reported that Pages is not enabled/configured for
Actions (HTTP 404); deployment is not claimed. Latest local quick CI on the
`13c58ff` worktree passed 5 lab tests, 77 CI Python tests, netem safety, and 12
portable C tests with strict warnings, ASan, and UBSan. The quick profile skips
Rust tests, release build, and SPDX validation; local Cargo/Rust tools are not
available here. For `13c58ff`, the hosted Linux/Windows Rust and release/SPDX
checks passed, while native MSVC and WDK x64/ARM64 were still in progress at the
latest query.

The client/service binaries and installer remain lab components. Their
explicitly insecure test modes are debug-only and loopback-only. Persistent
identities use account-appropriate state roots; ADR-0046 makes the service the
intended durable machine-policy authority and limits the old per-user file to
explicit migration. Rust and Windows hosted checks passed at `e3e406b`, but
policy authorization/ACL runtime behavior still requires an isolated Windows
VM. See ADR-0035, ADR-0038, and ADR-0046 for transport and policy behavior.

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
