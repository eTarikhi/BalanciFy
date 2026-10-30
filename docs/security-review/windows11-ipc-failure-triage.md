# Windows 11 IPC failure triage and rerun matrix

## Failure-log availability

The user supplied a Windows 11 local full-CI summary for HEAD `e63e1439b1ac2ad763e426e7dec249b6d013ee80`: all 12 listed CI stages passed. Those checks are separate from the IPC harness; portable driver tests are not driver install/runtime acceptance.

The user then supplied the Admin and Standard harness output after pulling commit `b147226`. This confirms the PowerShell syntax fixes worked. The Admin command reached `execution-context` but failed because the caller was not elevated: `ETARIKHI\\Administrator`, session 1, `elevatedAdmin=False`, while `-Role Admin` requires `IsInRole(Administrator)` to be true. That is a **test setup/context failure**, not a product bug; no Admin mutations or further Admin cases ran.

The later debug-service startup output prints the state directory/public key, then returns OS error 5 (`Access is denied`) before printing simulated uplink sockets or the IPC listener. In `balancify-service`, the next fallible operation after the public-key print is `MachinePolicyStore::open`; on Windows it applies a protected ACL with SYSTEM as owner/group. The service was launched from a non-elevated shell, so this strongly points to the service being unable to set its intended machine-policy ACL under that token. Treat this as a **service startup privilege/setup failure** until it is retried under the intended service identity; do not weaken the ACL.

The Standard command passed `execution-context`, but `permitted-ping`, `permitted-get-status`, `machine-policy-write`, `idle-connection-timeout`, `partial-ipc-header-timeout`, and `stalled-ipc-payload-timeout` all failed on `NamedPipeClientStream.Connect()` timing out after 5 seconds. Since startup failed before the named-pipe listener was created, these are downstream setup failures, not six product IPC failures. No IPC request was sent, so authorization and frame/idle deadlines were not tested. No case is accepted from these attempts; verify the listener starts and then rerun.

The syntax fixes are in pushed commit `b147226`. Hosted run [41038076670](https://github.com/eTarikhi/BalanciFy/actions/runs/41038076670) passed the repository-wide PowerShell parser, but PSScriptAnalyzer failed and the service-build/IPC steps were skipped. Analyzer findings are not available at this inspection.

The additive `-Role Auto -ReadOnlyOnly` mode is solely for hosted CI. It does not change standard-user/admin authorization logic.

## Minimum evidence needed for root-cause classification

Supply the original files without editing/redacting the diagnostic lines needed for triage:

1. `ipc-*.json` from each invocation (`context`, `command`, case `expected`, `observed`, `result`, `transcriptLog`, summary).
2. Full PowerShell console transcript/stdout/stderr and script exit code; exact command line and PowerShell version.
3. Windows edition/version/build, session ID, account type (standard, elevated admin, SYSTEM), service binary SHA/build, service start/stop logs, and named-pipe name.
4. For timeout cases, observed elapsed time, the service log around accept/request/close, and any Windows event-log entries.
5. For admin mutation cases, before/after status and flow-rule snapshots, and whether policy values/rules were restored.

Do not send signing keys, relay secrets, user credentials, raw packet payloads or private IP/identity data. Redact identifiers while keeping the relevant status codes and timing intact.

## Exact IPC reruns if an earlier symptom remains

The 12 local CI PASS checks do not need rerunning. The supplied Standard transcript names the timed-out IPC cases, but there is no case JSON or service log. Because each failed at `Connect()` before sending a request, first start the intended service build and confirm the process and local pipe `BalanciFy.Service.v1` are present. Follow RUNBOOK setup: use the approved test service, or in a disposable VM run the debug test host in a separate window with a unique temporary state directory and `--insecure-lab` (never add `--driver-device`):

```powershell
$env:BALANCIFY_STATE_DIR = 'C:\LabEvidence\balancify-ipc-state'
cargo build -p balancify-service --bin balancify-service
.\target\debug\balancify-service.exe --insecure-lab
```

Leave that process running while testing. Do not start a second listener if the approved service already owns the pipe. If the process is running but the named pipe is absent, stop and capture service stdout/stderr rather than treating downstream timeouts as IPC case results. Then rerun both role groups below. For Admin, launch a new **Run as administrator** PowerShell and verify `WindowsPrincipal.IsInRole(Administrator)` is `True`; the account name alone is insufficient. Do not mark a case accepted until its evidence JSON and transcript are reviewed.

### A. Standard-user authorization + transport group

On the Windows 11 VM, use a non-elevated standard local user and native Windows PowerShell 5.1:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows\Test-IpcAcceptance.ps1 `
  -Role Standard -OutputDirectory C:\LabEvidence\ipc-standard-rerun
```

Cases to capture exactly:

- `execution-context`
- `permitted-ping`
- `permitted-get-status`
- `standard-denied-SetFlowRule`
- `standard-denied-RemoveFlowRule`
- `standard-denied-SetBondMode`
- `standard-denied-SetDuplicateMode`
- `standard-denied-SetAllowMetered`
- `standard-denied-ApplyMachinePolicy`
- `standard-policy-unchanged`
- `idle-connection-timeout`
- `partial-ipc-header-timeout`
- `stalled-ipc-payload-timeout`

Expected authorization response is IPC Error status 5 (`PermissionDenied`) for all six mutations; expected transport close is near the service's 60-second timeout (45-second lower tolerance through the configured observation deadline). These expectations are not results.

### B. Elevated-admin mutation + session-0 group

In a disposable VM checkpoint, run an elevated native PowerShell 5.1 session:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows\Test-IpcAcceptance.ps1 `
  -Role Admin -AllowPolicyMutation -SkipTransportCases -IncludeNonInteractiveProbe `
  -OutputDirectory C:\LabEvidence\ipc-admin-rerun
```

Capture:

- `execution-context`, `permitted-ping`, `permitted-get-status`
- `admin-Bond-write-allowed`, `admin-Bond-write-applied`, `admin-Bond-restored`
- equivalent `admin-Duplicate-*` and `admin-AllowMetered-*` cases
- `admin-set-flow-rule-allowed`, `admin-remove-flow-rule-allowed`, any `admin-temporary-rule-cleanup`
- `admin-apply-machine-policy-allowed`
- `non-interactive-client-rejected`

The script's `finally` paths restore flags and remove the temporary rule; verify the final JSON and machine status/rules before leaving the VM. If interrupted, revert the snapshot or restore the recorded settings manually and include that recovery in the evidence.

### C. Admin read-only transport-only rerun (if an idle/framing case failed)

This isolates transport from policy mutation and scheduled-task setup:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows\Test-IpcAcceptance.ps1 `
  -Role Admin -ReadOnlyOnly -OutputDirectory C:\LabEvidence\ipc-admin-transport-rerun
```

Capture `execution-context`, Ping/GetStatus and all three transport cases listed in group A. `-ReadOnlyOnly` intentionally omits every machine-policy mutation; it cannot replace group A or B.

## How to classify after logs are supplied

- **Script bug candidate:** parser/runtime exception in the harness before a well-formed request is sent; malformed locally generated frame; evidence says wrong command/role/pipe; no corresponding service request reached the pipe; or timeout elapsed-time logic incorrectly maps an observed close to PASS/FAIL. Reproduce with the same PowerShell version and a wire-level trace where practical.
- **Product bug candidate:** harness sent the documented well-formed request under a verified role and the service returned the wrong status, changed policy despite denial, failed to restore state, accepted session 0, or violated the documented timeout/frame handling. Confirm service logs, token/session identity and request bytes before assigning root cause.
- **Environment/setup issue:** service missing/not ready, wrong named pipe/build, UAC token/session mismatch, VM policy, scheduled-task service unavailable, test package mismatch, or a host timeout that does not match the service's configured clock. Do not relabel setup failures as product acceptance.

Current status for every listed IPC case remains **NOT RUN / NOT ACCEPTED** until the raw evidence is supplied and reviewed.
