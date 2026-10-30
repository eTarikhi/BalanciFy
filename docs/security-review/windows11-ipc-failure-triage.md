# Windows 11 IPC failure triage and rerun matrix

## Failure-log availability

The user message contained the literal placeholder **`<paste logs>`**, not raw Windows 11 PowerShell output or an evidence JSON file. No failure case IDs, expected/observed values, timestamps, service logs, Windows build, account privilege, or script exit code were provided. Therefore it is not possible to classify any reported failure as a **script bug** or a **product bug** from the supplied material. No failure-specific code fix or acceptance claim is made.

The additive `-Role Auto -ReadOnlyOnly` mode was added only for hosted CI. It does not change standard-user/admin authorization logic and is not a claimed fix for the missing VM failures.

## Minimum evidence needed for root-cause classification

Supply the original files without editing/redacting the diagnostic lines needed for triage:

1. `ipc-*.json` from each invocation (`context`, `command`, case `expected`, `observed`, `result`, `transcriptLog`, summary).
2. Full PowerShell console transcript/stdout/stderr and script exit code; exact command line and PowerShell version.
3. Windows edition/version/build, session ID, account type (standard, elevated admin, SYSTEM), service binary SHA/build, service start/stop logs, and named-pipe name.
4. For timeout cases, observed elapsed time, the service log around accept/request/close, and any Windows event-log entries.
5. For admin mutation cases, before/after status and flow-rule snapshots, and whether policy values/rules were restored.

Do not send signing keys, relay secrets, user credentials, raw packet payloads or private IP/identity data. Redact identifiers while keeping the relevant status codes and timing intact.

## Exact reruns after the logs are available

Because no failure can be mapped to a specific case, rerun the complete case group that corresponds to the execution role. Do not mark a case accepted until its JSON and transcript are reviewed.

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
