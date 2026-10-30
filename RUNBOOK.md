# Windows VM and Rig B acceptance runbook

This runbook is for an owner-operated disposable Windows 11 x64 VM / isolated Rig B only. It uses native PowerShell; WSL is not needed. Scripts do not install a driver, enable test-signing, toggle HVCI, or flap an adapter unless the operator invokes the explicit guarded stage/switch described below. No test has been run from this checkout.

## 1. IPC authorization and framing tests

### Setup

1. Use a disposable Windows 11 VM and take a snapshot before running. Build/install the service and shell using your approved test package; start the service and confirm its local pipe is `BalanciFy.Service.v1`. If using the debug IPC test host instead of an installed service, in a separate PowerShell window set a unique temporary state directory, build and run the service with `--insecure-lab`, and leave it running while tests execute:

   ```powershell
   $env:BALANCIFY_STATE_DIR = 'C:\LabEvidence\balancify-ipc-state'
   cargo build -p balancify-service --bin balancify-service
   .\target\debug\balancify-service.exe --insecure-lab
   ```

   Use this only in the disposable VM, without relay credentials or `--driver-device`; do not start a second listener if the approved service already owns the pipe.
2. The IPC harness emits **one JSON evidence file per invocation**. It contains the structured evidence records and a `transcriptLog` array. Default output is ignored `target\acceptance-evidence\ipc\`. Hosted CI uses `-Role Auto -ReadOnlyOnly` to exercise permitted requests and frame/idle deadlines without policy mutation; it is hosted-runner evidence, not isolated-VM acceptance, and cannot establish standard-user authorization or admin write behavior.
3. The standard-user invocation takes roughly 3 minutes because idle, partial-header, and stalled-payload tests exercise the service's 60-second timeouts. `-TimeoutSeconds` is a client observation ceiling (65–300s), not a server setting.

### Standard-user run

Sign in as a standard local user (not an elevated split-token admin) and run an ordinary, non-elevated Windows PowerShell 5.1 console:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows\Test-IpcAcceptance.ps1 -Role Standard
```

Expected per-case results: execution context, Ping, and GetStatus PASS; each of SetFlowRule, RemoveFlowRule, SetBondMode, SetDuplicateMode, SetAllowMetered, and ApplyMachinePolicy returns IPC `PermissionDenied` status 5; the three exposed policy flags remain unchanged; idle connection closes; partial header and stalled payload close near the 60-second server deadline. The script never attempts a successful policy write in standard mode.

### Elevated-admin and session-0 runs

From **Run as administrator** PowerShell inside the disposable VM:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows\Test-IpcAcceptance.ps1 `
  -Role Admin -AllowPolicyMutation -SkipTransportCases -IncludeNonInteractiveProbe
```

The admin test toggles bond, duplicate, and metered flags and restores each original value; it also adds/removes a uniquely identified temporary custom rule and submits the exact current settings/rules as a no-op policy import. `-AllowPolicyMutation` is mandatory. Review each restoration case and the final policy/rule state. The non-interactive test creates a temporary SYSTEM scheduled task, attempts one Ping from session 0, records the observation, then unregisters the task and removes its temporary file. The policy is not changed by that task. A remaining task or temporary directory is a test failure: remove it manually and include that fact in evidence.

If a run is interrupted during the admin probes, restore the originally recorded bond, duplicate, and metered flags through the approved admin UI/service, remove any `IPC-acceptance-*` rule created by that run, verify the final status/rule snapshot, and include recovery in the evidence. Revert the VM snapshot if state cannot be confidently restored. Do not use production machine policy.

### Evidence review

For each JSON file, inspect `context`, every case's `command` / `expected` / `observed`, `result`, and `transcriptLog`. The test can fail because the service is absent or the wrong account/session is used; distinguish setup failures from authorization failures. Attach the service build SHA and service logs. These tests are VM evidence only; they do not establish driver or hardware acceptance.

## 2. Isolated VM driver tests

Follow [`docs/runtime-test-plan.md`](docs/runtime-test-plan.md). On the Hyper-V host, checkpoint first:

```powershell
.\scripts\windows\Manage-DriverVmCheckpoint.ps1 -Action Before -VMName BalanciFy-DriverTest -ConfirmDisposableVM
```

Inside the guest, begin with read-only preflight. Test-signing, install, Verifier, unload-under-load, and uninstall are separate explicit stages. Do not run these against the host or a daily-use VM. The scripts require the Hyper-V guest identity plus `-ConfirmDisposableVM`; test-signing additionally requires `-EnableTestSigning`; Verifier additionally requires `-ApplyVerifier`; interface/device removal under traffic requires `-ConfirmUnloadUnderLoad`.

Do not enable test-signing until a valid test-signed package is available and the before checkpoint has been verified. Never create/import certificates on the host or copy production signing keys into the guest; any trust setup uses only the verified public test certificate inside the disposable guest and is reverted with its checkpoint. Keep every driver/runtime table row `NOT RUN` until its logs are supplied and reviewed.

## 3. Rig B measurements

Start a UDP echo endpoint on a dedicated lab IP and firewall its port to the Rig B source address:

```powershell
.\scripts\windows\Start-UdpEchoEndpoint.ps1 -BindAddress 10.20.0.10 -Port 47321
```

The endpoint is intentionally small, rate-limited, and echoes only up to 256 bytes. Do not expose it to the public Internet. Install `iperf3.exe` from your approved source and run it against a controlled test server. Keep the actual BalanciFy driver/tunnel/relay in the path; the script cannot prove routing by itself.

Run each network scenario separately after configuring the corresponding links/policy on Rig B. `-Repetitions` cannot be set below 10:

```powershell
$evidence = 'C:\LabEvidence\RigB-Run-001'
.\scripts\windows\Invoke-RigBMeasurements.ps1 -Scenario Aggregate16 -Repetitions 10 `
  -IperfServer 10.20.0.10 -UplinkBaselineMbps 1000,300 -OutputDirectory $evidence

.\scripts\windows\Invoke-RigBMeasurements.ps1 -Scenario MatchedSingle -Repetitions 10 `
  -IperfServer 10.20.0.10 -UplinkBaselineMbps 1000,300 -OutputDirectory $evidence

.\scripts\windows\Invoke-RigBMeasurements.ps1 -Scenario MismatchedSingle -Repetitions 10 `
  -IperfServer 10.20.0.10 -BestUplinkMbps 1000 -UplinkBaselineMbps 1000,300 -OutputDirectory $evidence

# Measure the no-app median RTT first under the same route and endpoint, then provide it:
.\scripts\windows\Invoke-RigBMeasurements.ps1 -Scenario UdpEcho -Repetitions 10 `
  -UdpEchoServer 10.20.0.10 -NoAppMedianRttMs 8.5 -OutputDirectory $evidence

# Disruptive: requires elevated shell and exact adapter name. Script restores it in finally.
.\scripts\windows\Invoke-RigBMeasurements.ps1 -Scenario Failover -Repetitions 10 `
  -UdpEchoServer 10.20.0.10 -AdapterName 'Wi-Fi' -ConfirmUplinkFlap -OutputDirectory $evidence
```

The numbers above are examples only—replace them with named baselines measured on Rig B. Do not type illustrative values as results. `Aggregate16` uses iperf3 `-P 16`; the single-flow scenarios use `-P 1`. For matched/mismatched runs, configure the product/uplinks before each invocation and record that configuration. The mismatch scenario does **not** create a 40 ms path gap; configure and verify the path gap separately using the approved lab impairment setup and include its logs.

Outputs: `measurements.csv` contains per-repetition observations and separate threshold columns; `summary.json` reports median, p95, min/max, sample variance, standard deviation and coefficient of variation (CV), plus target comparisons. No baseline means no threshold comparison; provide the independent no-app/best-link/summed-link baselines rather than treating the target as a measurement. Retain server logs and packet/ETW captures alongside these outputs.

## 4. Soak, resume, compatibility and clean-machine runs

- Start a 72-hour monitor with `-Scenario SoakMonitor -SoakHours 72 -RequiredResumeCycles 100`. It does not suspend the computer; an operator must safely perform and record each sleep/resume cycle and separately schedule/log the ledger's controlled random link flaps. The script samples service/adapter state and counts Windows Power-Troubleshooter resume events; it does not generate flaps. Preserve the CSV and event-log export. Short trials are useful for setup only and do not satisfy the 72-hour gate.
- Generate `compatibility-checklist.csv` with `-Scenario CompatibilityTemplate`. Rows begin `NOT RUN`; run each named VPN, endpoint-security, Hyper-V, WSL2, HVCI-on/off and sleep/resume configuration, attach logs, and update each result only after review.
- For the clean-machine installer run, take a clean offline VM snapshot, record `-Scenario CleanBefore`, then manually run the approved installer/self-test/uninstall procedure from `docs/phase2/clean-machine-test.md`. After uninstall, use `-Scenario CleanAfter -BaselineSnapshot <clean-machine-cleanbefore.json>`. The snapshot helper only checks named BalanciFy service/driver/device/adapter/route artifacts; also manually validate files, registry, firewall, package, and installer journal. Do not use the synthetic `throughput.rs` benchmark as network acceptance.

## 5. Release gates

Use [`docs/release-gates-checklist.md`](docs/release-gates-checklist.md) to assign owners, gather evidence, and keep each item BLOCKED until its artifact is reviewed. Use [`docs/templates/acceptance-evidence-template.md`](docs/templates/acceptance-evidence-template.md) for ledger-compatible evidence records.
