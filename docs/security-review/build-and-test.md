# Build, test and evidence instructions

This page is for a reviewer reproducing source/hosted software checks. It does not authorize a production deployment or driver load. Never use production credentials/keys in CI or a disposable test VM. Do not install the driver or enable test-signing in hosted CI.

## Reproducible source baseline

Before review, record `git rev-parse HEAD`, `git status --short`, the full release artifact hashes, compiler/toolchain versions, `Cargo.lock` hash, WDK version, and any local patch. The package was prepared against base commit `528739405ef08404e48574808d5fcc513feef2bb`; at the time the CI workflow and IPC script had uncommitted edits, so this draft is not bound to a final immutable release SHA.

## Rust/source checks

From repository root with the pinned workspace lockfile:

```sh
cargo test --workspace --all-targets --no-fail-fast
cargo build --release --workspace
sh tools/ci/audit-deps.sh
sh tools/ci/sbom.sh 0.1.0
python3 tools/ci/sbom_v3.py 0.1.0
python3 tools/ci/validate_spdx3.py sbom.spdx3.json
```

The SPDX 2.3 document is generated and validated by the existing CI steps. Confirm generated SBOMs identify the exact candidate and are retained with release hashes; never edit the inventory to force a clean VEX result.

## Windows PowerShell and read-only IPC job

The new `windows-latest` job is designed to:

1. Enumerate every repository `.ps1` outside `.git`, generated `target`, and tool caches; parse each using `[System.Management.Automation.Language.Parser]::ParseFile` and fail on parse errors.
2. Run PSScriptAnalyzer 1.24.0 on the same script set. Error-severity findings fail the job; warnings are reported for triage.
3. Build the debug `balancify-service` executable, set a unique temporary `BALANCIFY_STATE_DIR`, and start it in explicit `--insecure-lab` mode without `--driver-device` or relay credentials.
4. Run `scripts/windows/Test-IpcAcceptance.ps1 -Role Auto -ReadOnlyOnly` against the local named pipe. It exercises a permitted Ping/GetStatus, idle disconnect, partial-header and stalled-body timeout. A temporary SYSTEM session-0 probe is included only if the runner token is elevated and Task Scheduler is running. The script never changes policy in this mode.
5. Upload parser/analyzer/IPC evidence and service logs. CI artifact status must be labelled **hosted-runner evidence, not isolated-VM acceptance**.

### Hosted-runner cases not covered

- **Standard-user context and denial of machine mutations:** runner account/token is environment-controlled and normally elevated; the job does not create/use a separate standard account. No standard-user authorization result may be inferred.
- **Elevated-admin machine-policy write success:** deliberately skipped with `-ReadOnlyOnly` to avoid modifying durable settings/rules on a shared runner. A recorded `IsInRole` value is context only, not authorization acceptance.
- **Windows 11 isolated-VM differences:** a GitHub-hosted runner is not the customer's disposable Windows 11 VM; ACLs, UAC, session model, service account, reboot, driver/device, HVCI, Verifier and packet-path behavior are not equivalent.
- **Driver/hardware cases:** no driver package install, service/driver registration, test-signing, HVCI toggle, WDK runtime, device IOCTL, real uplink flap, relay packet path or hardware measurement is run.

If a CI failure comes from environment prerequisites (session 0, task scheduler, service startup or pipe ACL), retain raw logs and classify it as environment/setup until reproduced; do not silently convert it to a product PASS. A failure in the IPC script itself must be triaged against the actual Windows 11 VM logs before assigning script/product root cause.

## Driver build boundary

WDK builds and PE/INF checks are compile/static-artifact evidence. Existing unsigned artifacts must not be installed on a host. Driver acceptance requires a separately prepared disposable Windows 11 Hyper-V VM, approved test-signed package, checkpoints, and the procedures in `docs/runtime-test-plan.md`. The current CI job performs none of those operations.

## Current results status

The user supplied a Windows 11 local full-CI summary for HEAD `e63e1439b1ac2ad763e426e7dec249b6d013ee80`; all 12 listed stages passed. The Admin harness parse defects (`$session:` and `$exit:`) were fixed in `b147226`. In the next VM invocation, Admin failed the context check because the PowerShell process was not elevated; Standard context passed but all pipe connections timed out. No IPC request reached the service, and service/pipe readiness logs were not supplied, so product root cause is undetermined. Hosted run [41038076670](https://github.com/eTarikhi/BalanciFy/actions/runs/41038076670) passed PowerShell parsing but failed PSScriptAnalyzer; service build and hosted IPC were skipped. Analyzer findings are not available. Standard/admin IPC authorization, Windows 11 named-pipe behavior, VM driver runtime and hardware measurements remain unverified.
