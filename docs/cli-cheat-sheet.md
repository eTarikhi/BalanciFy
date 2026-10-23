# BalanciFy CLI Cheat Sheet

A task-oriented command reference for the current repository. The project is a Rust 2021 workspace with a Windows NDIS miniport, Python lab/CI utilities, and Bash/PowerShell automation. It is a development prototype: the driver artifacts are unsigned, and production GUI, installation, hardware, HVCI, and packet-path acceptance are not established.

## Architecture and toolchain at a glance

- **Rust workspace:** `crates/core` (uplinks, policy and scheduling), `crates/tunnel` (wire framing and tunnel lane), `crates/crypto` (keys/handshake), `crates/ipc` (IPC v1), `crates/service` (service core and lab binaries), `crates/relay`, `crates/client`, `app` (lab CLI), and `installer` (transactional installer prototype). Workspace edition is Rust 2021; versions are pinned in `Cargo.lock`.
- **Driver:** `driver/` contains WDF/NDIS C source and a Visual Studio WDK project. Compile locally on Windows with WDK; Linux CI exercises portable C contracts, not the WDK binary.
- **Automation:** `.github/workflows/ci.yml` runs policy, Python/lab, Rust Linux/Windows, SPDX, and x64/ARM64 WDK checks. `.github/workflows/static.yml` builds and deploys the static portal to Pages from `main` or manual dispatch.
- **No frontend package manager:** there is no `package.json` or Node build. There are also no Docker/Compose manifests or machine-learning training scripts; Docker, npm, and training commands are therefore not part of this project workflow.

## 1. Development & setup

### Initialize the environment

Run from the repository root. The standard cross-platform toolchain is Rust stable; use Ubuntu 24.04/WSL2 for the full portable CI-equivalent suite.

```sh
rustup toolchain install stable --profile minimal  # install the CI Rust toolchain
rustup component add rustfmt clippy                 # install optional formatting/lint tools
cargo fetch --locked                               # fetch exactly the committed dependency graph
cargo metadata --locked --format-version 1         # inspect workspace/package metadata
cargo tree --locked                                # inspect dependency graph
sh tools/ci/audit-deps.sh                           # enforce dependency/crypto allowlist policy
```

Regenerate lockfile/allowlist data only when intentionally changing dependencies:

```sh
sh tools/ci/gen-lock.sh
```

Review the resulting `Cargo.lock` and `tools/ci/crypto-allowlist.txt` changes before committing; the script does not make the policy decision for you.

### Environment variables used by the lab binaries

Use an absolute, private state directory for local runs. Do not share development keys or use the insecure lab mode outside an isolated test environment.

```sh
export BALANCIFY_STATE_DIR="$HOME/.local/state/balancify-dev"  # client/service/relay key state
export BALANCIFY_IPC_PORT=5000                                 # loopback IPC listener port
export BALANCIFY_RELAY_PUBLIC_KEY='<64 hex characters>'         # pinned relay public identity
export BALANCIFY_FORWARD_ADDR='127.0.0.1:8080'                 # optional relay UDP forward target
```

Other code-supported toggles include `BALANCIFY_PER_FLOW=1` for client per-flow session behavior and `BALANCIFY_SIMULATE_ECN=1` for relay ECN simulation. Check the corresponding source before using environment overrides; they are lab controls, not production configuration management.

### Inspect CLI usage

```sh
cargo run -p balancify-service -- --help       # service flags and relay arguments
cargo run -p balancify-client -- --help        # client flags/positional arguments
cargo run -p balancify-installer -- --help     # installer dry-run/journal arguments
```

The app CLI takes the loopback IPC socket and a view name; available modes include `home`, `rules`, `diagnostics`, `tray`, `relay`, `bundle`, `perflow`, `rigb`, `hvci`, `sbom`, `failover`, and `soak`:

```sh
cargo run -p balancify-app -- 127.0.0.1:5000 home
cargo run -p balancify-app -- 127.0.0.1:5000 diagnostics
```

### Start a local lab stack (three terminals)

The following is for a disposable, trusted lab only. `lab-crypto` is explicitly **no confidentiality**, and the relay binds UDP sockets on `0.0.0.0`; isolate/firewall the host and stop the processes after testing. Never use this configuration for sensitive traffic or production.

```sh
# Terminal 1 — lab-only relay on UDP 4430 and 4431
cargo run -p balancify-relay --features lab-crypto -- 4430,4431

# Terminal 2 — debug-only loopback service and synthetic app traffic
mkdir -p "$HOME/.local/state/balancify-lab"
BALANCIFY_STATE_DIR="$HOME/.local/state/balancify-lab" \
  cargo run -p balancify-service -- \
  --insecure-lab --simulate-app-traffic 127.0.0.1:4430

# Terminal 3 — query the service's loopback IPC adapter
cargo run -p balancify-app -- 127.0.0.1:5000 home
```

The service refuses to start without a configured/pinned relay identity unless `--insecure-lab` is explicitly used in a debug build, and that mode only permits loopback relay addresses. The service's default IPC adapter binds loopback; it is a lab TCP adapter, not the planned production ACL-protected named pipe. For secure relay identity provisioning and persistent state, consult the service/client/relay source and `docs/phase2/security-hardening.md` rather than reusing the lab command above.

### Local CI runner

```sh
bash tools/ci/local-ci.sh --quick  # policy + Python + netem safety + portable driver tests
bash tools/ci/local-ci.sh          # full local software suite, Rust build/tests and SPDX validators
bash tools/ci/local-ci.sh --push   # full checks, then push clean committed arena/* HEAD and watch remote CI
```

The full profile requires Rust/Cargo and installs the exact SPDX validator versions into ignored `target/local-ci/venv/`. `--push` never stages or commits; it requires a clean worktree and checks that the remote branch tip is an ancestor of the tested commit. If the later GitHub run fails, the commit remains pushed. See [`local-development-and-ci.md`](local-development-and-ci.md) for OS prerequisites, logs, timeout, and security details.

## 2. Build processes

### Compile and build the Rust workspace

```sh
cargo check --workspace                              # fast type-check without producing release artifacts
cargo build --workspace                              # debug build of all workspace members
cargo build --release --workspace                    # optimized workspace build (Linux CI build step)
cargo build -p balancify-service --bin balancify-service
cargo build -p balancify-relay --features lab-crypto # lab-only relay binary; no confidentiality
```

Run a selected binary without manually locating its output:

```sh
cargo run -p balancify-service -- --help
cargo run -p balancify-client -- --help
cargo run -p balancify-installer -- --dry-run --journal /tmp/balancify-install-journal.json
```

The installer command exercises prototype journal/dry-run logic; it is **not** a production installer and does not install the kernel driver or GUI.

### Run software harnesses (not hardware measurements)

```sh
cargo run -p balancify-service --bin throughput --release  # synthetic service-path workload, not network throughput
cargo run -p balancify-service --bin soak --release        # fast simulated stability/flap harness, not a real 72-hour soak
cargo run -p balancify-service --bin compat --release      # software scenario harness, not an installed Windows compatibility matrix
```

Interpret their output as local software processing evidence only. It does not establish adapter-added latency, bonded throughput, driver behavior, Windows compatibility, or hardware acceptance.

### Build and preview the documentation portal

```sh
sh tools/ci/build-docs-site.sh /tmp/balancify-site
python3 -m http.server 8000 --directory /tmp/balancify-site
```

Open `http://127.0.0.1:8000` in a local browser. The site builder assembles the reviewed static HTML/SVG artifact; it does not deploy it.

### Build the Windows driver

Prerequisites: Windows, Visual Studio 2022/MSBuild 17.x, Windows SDK/WDK 10.0.26100.0 and the CI-pinned WDK 10.0.26100.6584. From Developer PowerShell:

```powershell
pwsh -File tools/ci/local-wdk-build.ps1                       # x64 + ARM64; writes under target/local-wdk/
pwsh -File tools/ci/local-wdk-build.ps1 -Architecture x64
pwsh -File tools/ci/local-wdk-build.ps1 -Architecture ARM64
```

The script builds with signing disabled and verifies PE architecture, required mitigation flags, and an empty certificate table. It does **not** sign, install, or load the driver. The supported CI-equivalent x64/ARM64 builds also run on hosted Windows Actions runners.

## 3. Testing & debugging

### Rust tests, formatting, and linting

```sh
cargo test --workspace --all-targets --no-fail-fast              # match Linux CI test command
cargo test -p balancify-service                                  # focus on one crate
cargo test -p balancify-installer installer_cli_dry_run          # filter by test name
cargo test -p balancify-service -- --nocapture                   # show test stdout/stderr
cargo fmt --all -- --check                                       # optional format check
cargo clippy --workspace --all-targets -- -D warnings            # optional lint; not a separate required CI job
```

`cargo fmt`/`clippy` are useful local quality gates but the required workflow is the checked-in CI workflow; do not treat an optional lint command as a substitute for CI.

### Python, shell, and portable C contract tests

```sh
python3 -m unittest discover -s tools/lab -p 'test_*.py' -v
python3 -m unittest discover -s tools/ci -p 'test_*.py' -v
python3 -m unittest discover -s driver/tests -p 'test_*.py' -v
sh -n tools/lab/netem-rig.sh
sh tools/lab/test_netem_rig.sh
sh tools/ci/test-driver-packets.sh  # strict C warnings + ASan/UBSan + OID/INF contracts
```

The portable packet suite covers raw IPv4/IPv6 lengths and TCP/UDP boundaries, queue behavior, MTU and pause state. It does not run kernel fuzzing, WDK runtime checks, or Driver Verifier.

### Debugging failures and driver artifacts

```sh
RUST_BACKTRACE=1 cargo test --workspace --all-targets --no-fail-fast
RUST_BACKTRACE=1 cargo run -p balancify-service -- --help
```

PowerShell equivalent:

```powershell
$env:RUST_BACKTRACE = '1'
cargo test --workspace --all-targets --no-fail-fast
```

The local CI runner streams each step and saves logs/summary in `target/local-ci/logs/<UTC timestamp>/`. For a prebuilt CI artifact, download it from the relevant GitHub Actions run; verify an unsigned driver without installing it:

```powershell
py -3 tools/ci/verify_driver_pe.py driver/x64/Release/balancify.sys x64
py -3 tools/ci/verify_driver_pe.py driver/ARM64/Release/balancify.sys ARM64
```

For WDK build diagnostics, inspect `target/local-wdk/wdk-x64.log` or `target/local-wdk/wdk-ARM64.log`; for Actions, inspect the job summary and uploaded WDK logs. No repository-wide `RUST_LOG` logger configuration is defined, so do not assume `RUST_LOG=debug` enables additional output.

## 4. Deployment & maintenance

### Dependency and SBOM maintenance

```sh
sh tools/ci/audit-deps.sh                         # policy / exact-pin audit
sh tools/ci/sbom.sh 0.1.0                        # generate SPDX 2.3 inventory
python3 tools/ci/sbom_v3.py 0.1.0                 # generate SPDX 3.0.1 JSON-LD inventory
```

The full local CI runner installs pinned validators and checks both files. Generated SBOM files are ignored build artifacts. The repository does not fabricate VEX claims; reviewed vulnerability-status evidence remains separate.

### Trigger and monitor GitHub Actions

Pushes to `main` and `arena/*` and pull requests trigger `.github/workflows/ci.yml`. The CI workflow also supports manual dispatch (after the workflow version is present on the repository's default branch). Static Pages deployment is separately configured for pushes to `main` or manual dispatch.

```sh
gh workflow list
gh workflow run ci.yml --ref "$(git branch --show-current)"      # CI for an already-pushed branch
gh run list --workflow ci.yml --branch "$(git branch --show-current)"
gh run watch RUN_ID --exit-status --interval 20
gh run view RUN_ID
gh pr checks PR_NUMBER --watch

# Publish the static documentation site, from main or an allowed ref:
gh workflow run static.yml --ref main
gh run list --workflow static.yml --branch main
```

Authenticate once with `gh auth login`; keep credentials in the GitHub CLI credential store, not the repository. A green CI run proves the configured source/build/test checks only—not driver signing, installation, runtime packet flow, HVCI, or product readiness.

### Lab network measurement (explicitly opt-in)

Dry-run is safe and produces no result files:

```sh
sh tools/lab/netem-rig.sh --dry-run rigb 1
```

Real `--apply` traffic requires a dedicated lab host, a running iperf3 server, root, `tc`/`ip`, and explicit environment values. Shaped profiles **replace root qdiscs** on the selected interfaces; cleanup removes the test qdisc but does not restore an existing custom qdisc. Never run this against a production/shared network. Example only for dedicated disposable interfaces:

```sh
sudo env IPERF3_HOST=192.0.2.10 IFACE=eth0 IFACE_SLOW=eth1 \
  CONFIRM_TC_ROOT_REPLACE=YES sh tools/lab/netem-rig.sh --apply rigb 10
python3 tools/lab/analyze.py /tmp/balancify-netem-results
```

`192.0.2.10` is a documentation-only address; replace it with the isolated lab iperf3 endpoint. Raw iperf3 link-path output is not proof of BalanciFy bonded throughput or driver acceptance.

### What is not configured here

There is no container build/deployment flow, no production GUI build, no training pipeline, and no safe driver install/deploy command in this cheat sheet. Driver products from CI are unsigned. Signing/attestation, test-signed installation, Driver Verifier, HVCI/VBS, real NDIS packet-path execution, clean-machine installer validation, and hardware performance testing must follow their dedicated Windows lab procedures and be reported as pending until performed.
