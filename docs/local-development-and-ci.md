# Local development, CI execution, and monitoring

This guide maps repository setup to `.github/workflows/ci.yml` and provides a local runner that can optionally push a previously committed branch after the local checks pass. It keeps local validation separate from hardware/driver acceptance and never signs, installs, or loads the driver.

## 1. What the CI pipeline runs

| Job | GitHub runner | Local equivalent |
|---|---|---|
| Dependency + crypto policy audit | Ubuntu 24.04 | `sh tools/ci/audit-deps.sh` |
| Lab analysis and CI Python tests; netem safety; packet/OID contracts | Ubuntu 24.04 | `tools/ci/local-ci.sh` runs the Python unittest suites, `sh -n` plus `tools/lab/test_netem_rig.sh`, and `tools/ci/test-driver-packets.sh` |
| Rust workspace tests | Ubuntu 24.04 and Windows | Ubuntu/WSL local runner runs `cargo test --workspace --all-targets --no-fail-fast`; Windows parity is confirmed by remote CI |
| Rust release build and SPDX 2.3/3.0.1 inventories | Ubuntu 24.04 | Local full profile builds release and generates/validates both SPDX files with the pinned validators |
| WDK x64 and ARM64 compile/link plus PE verification | Windows Server 2022 | `tools/ci/local-wdk-build.ps1` on Windows with the WDK; also run remotely on every CI push |
| Pages deployment | Ubuntu 24.04, main only | Not part of the CI workflow runner; build/test the static portal locally with `python3 -m unittest discover -s tools/ci -p 'test_*.py'` |

GitHub-hosted Windows jobs are not faithfully emulated by `act` or by Linux. The local Linux/WSL script intentionally reports that it does not run the WDK jobs; the `--push` option then waits for actual GitHub Actions, including both WDK architectures and the Windows Rust job.

## 2. Requirements

### Windows 11 x64 setup (recommended host workflow)

Use Ubuntu 24.04 under WSL2 for Git, the complete CI-equivalent software run, and the end-to-end PR workflow. Use native Windows PowerShell/MSBuild for the optional WDK compile. The repository’s full local runner intentionally targets Linux; native PowerShell does not replace the ASan/UBSan C checks or pinned SPDX validators.

From an elevated Windows Terminal, install WSL2 if needed, then reboot if Windows requests it:

```powershell
wsl --install --distribution Ubuntu-24.04
```

In the new Ubuntu terminal, install the portable-suite prerequisites and clone into the Linux filesystem (not `/mnt/c`, for faster Cargo builds and reliable permissions):

```sh
sudo apt update
sudo apt install --yes build-essential python3 python3-venv python3-pip git curl gh
mkdir -p ~/src && cd ~/src
git clone https://github.com/eTarikhi/BalanciFy.git
cd BalanciFy
```

Install Rust stable inside WSL as documented below, then authenticate `gh` and configure Git there. GitHub CLI credentials are not automatically shared between Windows and WSL:

```sh
gh auth login
gh auth setup-git
git config --global user.name "Your Name"
git config --global user.email "you@example.com"
```

For native Rust tools in Windows PowerShell, `winget install --id Rustlang.Rustup --exact` may install the `rustup-init.exe` bootstrapper without placing the `rustup` toolchain command on PATH. Run the bootstrapper, then reopen PowerShell:

```powershell
$rustupInit = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet" -Filter rustup-init.exe -File -Recurse -ErrorAction SilentlyContinue |
    Select-Object -First 1 -ExpandProperty FullName
if (-not $rustupInit) { throw 'rustup-init.exe was not found under the WinGet package directory.' }
& $rustupInit -y --default-toolchain stable --profile minimal
```

After reopening PowerShell:

```powershell
rustup toolchain install stable --profile minimal
rustup component add rustfmt clippy --toolchain stable
cargo --version
```

If `rustup` is still not found in the current PowerShell session, prepend `$env:USERPROFILE\.cargo\bin` to `$env:Path` or reopen the terminal. The WDK build additionally needs Visual Studio 2022 C++ Build Tools/MSBuild 17.x and the pinned WDK; see the native Windows requirements below.

### Canonical Linux / WSL2 environment

Use Ubuntu 24.04 natively or in WSL2 for the local CI-equivalent software suite. CI currently uses Ubuntu 24.04 and Python 3.12.

- Git, Bash, POSIX `sh`, `coreutils` (`timeout`, `tee`, `seq`), and `cc`/GCC with AddressSanitizer and UndefinedBehaviorSanitizer support.
- Python 3.12 (3.10 minimum for the local runner), `venv`, and pip.
- Rust stable via rustup, including `cargo` and `rustc`.
- GitHub CLI (`gh`) authenticated for optional remote monitoring/push. Local-only testing does not need GitHub authentication.
- Network access when first fetching Cargo crates or pinned Python validators. The validator virtual environment and logs are stored under ignored `target/local-ci/`.

Typical Ubuntu bootstrap (install `gh` using the current GitHub CLI installation instructions for your distribution):

```sh
sudo apt update
sudo apt install --yes build-essential python3 python3-venv python3-pip git curl gh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
rustup toolchain install stable --profile minimal
# Authenticate GitHub CLI if you want push/PR automation:
gh auth login
gh auth setup-git
```

In WSL2, clone the repository inside the Linux filesystem (for example `~/src/BalanciFy`) rather than `/mnt/c/...` for faster Cargo builds and more reliable file permissions. Configure Git identity and SSH or HTTPS remote access using your normal secure credential manager. Never put a GitHub token in a script, `.env` file, or command history.

### Native Windows WDK environment (optional local driver compile)

- Windows 11 x64 or Windows Server 2022; Visual Studio 2022 / Build Tools with MSBuild 17.x and the C++ build tools.
- Windows Driver Kit 10.0.26100.6584 (the CI pin), plus Windows SDK/WDK headers and libraries for target platform 10.0.26100.0.
- Rust stable, Python launcher `py -3`, Git, and PowerShell. Run from Developer PowerShell so `msbuild` is on `PATH`.
- Build both `x64` and `ARM64`; the WDK host toolchain is x64. The script writes outputs/logs below `target/local-wdk/` and validates PE architecture, required mitigation flags, and the empty certificate table.

The local WDK script uses `SignMode=Off`. It does not require a signing certificate and does not install/load the `.sys` file. Use WSL2 for the full portable suite; use native Windows for the additional WDK compile check. The ASan/UBSan C fixtures are designed for the Linux CI compiler environment.

## 3. First-time repository setup

```sh
git clone https://github.com/eTarikhi/BalanciFy.git
cd BalanciFy
rustup toolchain install stable --profile minimal
cargo fetch --locked
python3 -m venv target/local-ci/venv
```

The local CI runner creates/reuses that venv and installs the same pinned SPDX validators used by Actions: `spdx-tools==0.8.5` and `spdx3-validate==0.0.7`. It does not change the user's system Python packages. Cargo outputs, SBOM files, local logs, and local WDK products live in ignored `target/` or the repository's existing ignored SBOM paths.

For optional GitHub push/watch mode:

```sh
gh auth login
gh auth status
git remote -v
```

Use the configured `origin`; do not paste credentials into chat or repository files.

## 4. Local commands and daily workflow

### Quick checks (no Rust build or SBOM validation)

```sh
bash tools/ci/local-ci.sh --quick
```

This runs the dependency audit, lab Python tests, `tools/ci` Python tests, netem script syntax/safety checks, and portable driver tests. It cannot push.

### Full local software CI

```sh
bash tools/ci/local-ci.sh
```

This runs the quick checks plus Rust workspace tests, release build, SPDX 2.3 generation/validation, and SPDX 3.0.1 generation/validation. It does not run WDK or native Windows jobs on Linux/WSL.

For separate local WDK build and PE checks, open Developer PowerShell in the repository:

```powershell
pwsh -File tools/ci/local-wdk-build.ps1
# or just one architecture
pwsh -File tools/ci/local-wdk-build.ps1 -Architecture x64
pwsh -File tools/ci/local-wdk-build.ps1 -Architecture ARM64
```

### Push only after local success, then watch remote CI

Commit the intended source changes first, then run the full local suite in WSL/Linux:

```sh
bash tools/ci/local-ci.sh --push
```

`--push` is deliberately strict and safe:

1. Requires a clean worktree and a named non-default topic branch (no detached HEAD, direct default-branch push, or uncommitted changes).
2. Confirms `origin`, GitHub CLI authentication, and that local history contains the remote branch tip.
3. Runs every full-profile local check; on any failure, stops without pushing.
4. Confirms the checked-out commit/worktree did not change during validation, then performs a normal non-force `git push` of that exact HEAD.
5. Watches the GitHub Actions `CI` run for that commit and returns nonzero if the remote run fails or times out.

The script does **not** stage or create commits. Remote CI is triggered by the push, so the commit is already pushed when remote checks begin. If remote CI fails, the script reports failure and leaves the pushed commit in place; it never rewrites or force-pushes history. Configure the timeout with `LOCAL_CI_REMOTE_TIMEOUT_SECONDS` (default 5400 seconds).

To manually dispatch the GitHub workflow for an already-pushed branch without pushing again:

```sh
gh workflow run ci.yml --ref "$(git branch --show-current)"
gh run list --workflow ci.yml --branch "$(git branch --show-current)"
# Then monitor the returned run ID:
gh run watch RUN_ID --exit-status --interval 20
```

The workflow includes `workflow_dispatch` on the default branch. Pushes to all branches and pull requests trigger CI automatically.

### Validate, push, create a PR, and optionally merge

The end-to-end orchestrator requires a clean, committed non-default topic branch. It runs the full local software suite, pushes only after success, waits for push-triggered GitHub CI, creates or reuses an open PR targeting `main`, and waits for the PR-triggered CI run and required PR checks:

```sh
bash tools/ci/pr-workflow.sh             # validate, push, create/reuse PR, wait for checks; leave PR open
bash tools/ci/pr-workflow.sh --merge     # same gates, then merge with a merge commit
```

The default intentionally does **not** merge. `--merge` is explicit opt-in; the script will issue `gh pr merge <number> --merge` only after local checks, push CI, and PR CI pass and the PR head still matches the validated commit. It does not use GitHub auto-merge, force-push, or create commits. It can be launched from Windows PowerShell in the WSL repository:

```powershell
wsl.exe -d Ubuntu-24.04 -- bash -lc 'cd ~/src/BalanciFy && bash tools/ci/pr-workflow.sh'
```

To allow it to merge automatically after all those gates, append `--merge`. Review the PR and ensure repository branch protections are configured before opting in. Since the script first waits for push CI and then PR CI, a PR submission runs two remote CI workflows for the same commit. If local validation or either remote workflow fails, no merge is attempted. If a new commit appears while checks run, the script refuses to merge it.

## 5. Logs, monitoring, and result interpretation

- Local output is streamed to the terminal and preserved per step at `target/local-ci/logs/<UTC timestamp>/`; the summary is `summary.txt`. A failed step stops the runner and points to its log.
- Local driver outputs and logs are under `target/local-wdk/`.
- During a remote run, use `gh run list --branch <branch>`, `gh run view <run-id>`, and `gh run watch <run-id> --exit-status`. For PR status, use `gh pr checks <number> --watch`.
- On a local pass followed by remote failure, compare local logs with the failed GitHub job logs/artifacts. The WDK build logs and unsigned driver artifacts are published by CI; they are not release-signed products.
- The local Linux runner is a development gate, not evidence of Windows runtime acceptance. The netem checks are deliberately dry-run/safety tests and do not run live traffic or fabricate measurements.

## 6. What remains outside local CI

Neither this runner nor a green Actions check proves driver installation, signing/attestation, live NDIS packet delivery, scheduler/steering behavior, Driver Verifier, HVCI/VBS, real netem/Rig B throughput, clean-machine install/rollback, or hardware performance. Keep those acceptance items pending until executed on the designated Windows VM and hardware setup and recorded with evidence. See `docs/phase2/next-steps.md`, `docs/phase2/hvci-readiness.md`, and `docs/phase2/throughput-rows.md`.
