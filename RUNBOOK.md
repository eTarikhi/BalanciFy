# Windows VM and Rig B acceptance runbook

This runbook is for an owner-operated disposable Windows 11 x64 VM / isolated Rig B only. It uses native PowerShell; WSL is not needed. Scripts do not install a driver, enable test-signing, toggle HVCI, or flap an adapter unless the operator invokes the explicit guarded stage/switch described below. No driver or hardware acceptance is claimed. The user's reported machine-policy ACL result is tracked separately from IPC acceptance; the latest service run did not remain available for case-level IPC evidence.

## Insecure lab relay and IPC availability

`balancify-service --insecure-lab` is a debug-only zero-key lab mode. It opens a local test session without an authenticated handshake, then sends tunnel probes/data as UDP to the configured relay. With no explicit address, the destination is `127.0.0.1:4430`; an optional secondary relay uses its configured loopback address (the lab relay defaults to ports 4430 and 4431). Windows may report ICMP Port Unreachable for an absent UDP listener as `WSAECONNRESET` / OS error 10054. In this mode the service now logs the unreachable loopback relay and keeps the local IPC event loop alive; tunnel traffic is unavailable until a relay is listening. Other send errors remain fatal, and secure mode remains fail-closed.

For IPC-only acceptance, no relay is required. If testing data-plane traffic, an optional disposable lab relay can be started separately:

```powershell
cargo run -p balancify-relay --features lab-crypto -- 4430,4431
```

The relay's lab-crypto mode has **no confidentiality** and binds UDP on `0.0.0.0`; run it only inside the disposable VM and isolate/firewall it. Never send sensitive or production traffic through it.

## LocalSystem machine-policy ACL test

The ignored Windows test calls the production `MachinePolicyStore::open()` against a unique temporary directory, then reads its owner and DACL using Windows security APIs. It asserts owner SID `S-1-5-18`, a protected DACL containing exactly SYSTEM and built-in Administrators with full access, and no Authenticated Users (`S-1-5-11`) or Users (`S-1-5-32-545`) ACE. It does not start the service or load a driver. It is ignored by default because assigning SYSTEM as owner requires LocalSystem / `SeRestorePrivilege`; elevated Administrator is intentionally insufficient.

From an elevated PowerShell in the disposable Windows VM, launch an interactive LocalSystem PowerShell with PsExec:

```powershell
& 'C:\Tools\PsExec64.exe' -accepteula -i -s powershell.exe -NoProfile -NoExit
```

In the new SYSTEM PowerShell window, verify the token, change to the repository clone, and run the ignored test:

```powershell
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if ($identity.User.Value -ne 'S-1-5-18') {
    throw "Expected LocalSystem (S-1-5-18); got $($identity.Name) / $($identity.User.Value)."
}
Set-Location -LiteralPath 'C:\path\to\BalanciFy'
cargo test -p balancify-service --lib `
  machine_policy::tests::windows_real_open_sets_system_owner_and_restricted_dacl `
  -- --ignored --exact --nocapture
```

A pass verifies the ACL on the fresh temporary test directory only. It does not establish IPC acceptance; those cases remain **NOT RUN / NOT ACCEPTED** until the separate service-listener and case-level evidence steps below are completed.

## 1. IPC authorization and framing tests

### Setup

1. Use a disposable Windows 11 x64 VM, take a snapshot, and use **native PowerShell** (not `cmd.exe` for PowerShell cmdlets). Start the service once under its intended identity and confirm the listener before any IPC cases. Do not install a driver, enable test-signing, or use production policy/credentials for these tests.
2. **The debug service must run as LocalSystem (or the configured service identity with equivalent restore authority).** `MachinePolicyStore::open` applies a protected ACL whose owner/group are SYSTEM. Assigning SYSTEM as owner requires `SeRestorePrivilege`; an elevated Administrator token is not sufficient and this service deliberately does not enable that privilege in an admin token. Windows reports error 1307 when the owner SID cannot be assigned, or error 5 for access denied. Startup now names the exact policy-store path and OS error and tells you to run as SYSTEM. Do not grant extra access to the policy directory, change its owner/DACL manually, or weaken the ACL.
3. If a previous non-SYSTEM launch left the policy directory behind, first ensure the test service process has exited. Inspect it without changing its ACL:

   ```powershell
   $stateDir = 'C:\LabEvidence\balancify-ipc-state'
   $policyPath = Join-Path $stateDir 'machine-policy'
   if (Test-Path -LiteralPath $policyPath) {
       $contents = @(Get-ChildItem -LiteralPath $policyPath -Force)
       if ($contents.Count -eq 0) {
           Remove-Item -LiteralPath $policyPath -Force
           'Removed the empty directory created by the failed startup.'
       } else {
           $contents | Select-Object Name,Length,LastWriteTime
           throw 'Policy data exists. Do not delete it; preserve it and restart the service as LocalSystem so it can apply the protected ACL.'
       }
   }
   ```

   Only remove the directory if it is empty and belongs to this disposable test state. If it contains `machine-policy.bin`, `machine-policy.bak`, or any other data, leave it intact; run the service as SYSTEM to secure and load it, or revert the VM snapshot if its provenance is uncertain. Do not use `icacls`, `Set-Acl`, or recursive deletion on the `machine-policy` child to bypass its protected ACL. Step 4 restricts only the fresh test state-directory parent to SYSTEM and Administrators before the child is created.
4. In elevated PowerShell inside the VM, build the debug host and copy only the executable and IPC script to a path readable, but not writable, by standard users. Use an official Sysinternals PsExec copy at `C:\Tools\PsExec64.exe` (obtain it through the approved lab software process):

   ```powershell
   New-Item -ItemType Directory -Force -Path C:\LabEvidence | Out-Null
   $stateDir = 'C:\LabEvidence\balancify-ipc-state'
   New-Item -ItemType Directory -Force -Path $stateDir | Out-Null
   icacls $stateDir /inheritance:r /grant:r `
     '*S-1-5-18:(OI)(CI)(F)' `
     '*S-1-5-32-544:(OI)(CI)(F)'
   cargo build -p balancify-service --bin balancify-service
   $publicRoot = 'C:\Users\Public\BalanciFy-Ipc'
   New-Item -ItemType Directory -Force -Path $publicRoot | Out-Null
   Copy-Item .\target\debug\balancify-service.exe (Join-Path $publicRoot 'balancify-service.exe') -Force
   Copy-Item .\scripts\windows\Test-IpcAcceptance.ps1 (Join-Path $publicRoot 'Test-IpcAcceptance.ps1') -Force
   icacls $publicRoot /inheritance:r /grant:r `
     '*S-1-5-18:(OI)(CI)(F)' `
     '*S-1-5-32-544:(OI)(CI)(F)' `
     '*S-1-5-32-545:(OI)(CI)(RX)' /T /C
   ```

   Create a launcher and start it interactively as SYSTEM with **PsExec `-s -i`**. The launcher verifies the token SID before starting the service; it uses a unique temporary state directory and never opens the driver:

   ```powershell
   $env:BALANCIFY_STATE_DIR = 'C:\LabEvidence\balancify-ipc-state'
   $serviceExe = Join-Path $publicRoot 'balancify-service.exe'
   $launcher = 'C:\LabEvidence\Start-BalanciFyIpcService.ps1'
   @"
   `$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
   if (`$identity.User.Value -ne 'S-1-5-18') { throw "Expected LocalSystem, got `$(`$identity.Name) (`$(`$identity.User.Value))." }
   Write-Host "Service identity: `$(`$identity.Name), SID `$(`$identity.User.Value)"
   `$env:BALANCIFY_STATE_DIR = 'C:\LabEvidence\balancify-ipc-state'
   & '$serviceExe' --insecure-lab
   "@ | Set-Content -LiteralPath $launcher -Encoding UTF8
   & 'C:\Tools\PsExec64.exe' -accepteula -i -s powershell.exe `
     -NoProfile -ExecutionPolicy Bypass -NoExit -File $launcher
   ```

   In the PsExec window, verify the identity is `NT AUTHORITY\SYSTEM` / SID `S-1-5-18`, then retain the full console output. Do not start another listener if the pipe is already owned. Continue only after `[startup] open machine-policy store: OK` and `[startup] start local IPC v1 listener: OK`. If startup prints error 1307 or 5, the service did not become available; correct the identity and restart rather than weakening the ACL.
5. Create a dedicated **standard, non-Administrator local account** for the Standard-role run. Enter a test-only password interactively; do not add this account to Administrators:

   ```powershell
   $standardName = 'BalanciFyIpcStandard'
   if (-not (Get-LocalUser -Name $standardName -ErrorAction SilentlyContinue)) {
       $password = Read-Host 'Temporary password for the disposable-VM test account' -AsSecureString
       New-LocalUser -Name $standardName -Password $password -Description 'Temporary BalanciFy IPC test account'
   }
   $standardSid = (Get-LocalUser -Name $standardName).SID.Value
   $adminSid = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
   if (@(Get-LocalGroupMember -SID $adminSid | Where-Object { $_.SID.Value -eq $standardSid }).Count -gt 0) {
       throw 'The IPC test account is an Administrator; use a separate standard account.'
   }
   $standardOutput = 'C:\LabEvidence\ipc-standard-rerun'
   New-Item -ItemType Directory -Force -Path $standardOutput | Out-Null
   icacls $standardOutput /inheritance:r /grant:r `
     '*S-1-5-18:(OI)(CI)(F)' `
     '*S-1-5-32-544:(OI)(CI)(F)' `
     "*${standardSid}:(OI)(CI)(M)"
   ```

   Open a separate shell as that account with `runas.exe /user:"$env:COMPUTERNAME\BalanciFyIpcStandard" 'powershell.exe -NoProfile'`, enter the password when prompted, and confirm `IsInRole(Administrator)` is `False`. Then run the public, read-only script copy; its output directory is the one explicitly granted to this test account:

   ```powershell
   $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
   $principal = [Security.Principal.WindowsPrincipal]::new($identity)
   if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
       throw 'This is an elevated Administrator token, not a Standard test context.'
   }
   powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\Users\Public\BalanciFy-Ipc\Test-IpcAcceptance.ps1 `
     -Role Standard -OutputDirectory C:\LabEvidence\ipc-standard-rerun
   ```

   The script first connects to the named pipe and requires a Pong. If the service is absent, it emits one `service-reachable` failure with **“Service not reachable / pipe missing”**, marks requested IPC cases **NOT RUN**, and exits without repeating five-second connection attempts. A Standard run from an elevated/admin-group token fails `execution-context` before attempting any pipe connection.
6. In a separate **Run as administrator** PowerShell, only after the Standard run, run the admin probes in the disposable VM:

   ```powershell
   powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\Users\Public\BalanciFy-Ipc\Test-IpcAcceptance.ps1 `
     -Role Admin -AllowPolicyMutation -SkipTransportCases -IncludeNonInteractiveProbe `
     -OutputDirectory C:\LabEvidence\ipc-admin-rerun
   ```

   The IPC harness writes one JSON evidence file per invocation, including each case's `result`, `observed`, and `transcriptLog`. Keep connection/setup failures separate from protocol results. Do not accept any IPC case unless the service listener was confirmed and the case-level JSON/transcript was reviewed. Hosted CI is **hosted-runner evidence, not isolated-VM acceptance**; it cannot establish standard-user authorization or admin writes.
7. After preserving evidence, stop the LocalSystem debug service with Ctrl+C in the PsExec window. Remove the temporary account and copied public test files only after confirming evidence is retained; do not delete machine-policy data manually. Revert the disposable VM snapshot if the policy directory or machine state is uncertain. The Standard invocation takes roughly 3 minutes for its 60-second idle/framing deadlines; `-TimeoutSeconds` is a client observation ceiling (65–300s), not a server setting.

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
