# Windows 11 IPC failure triage and rerun matrix

## Failure-log availability

The user supplied a Windows 11 local full-CI summary for HEAD `e63e1439b1ac2ad763e426e7dec249b6d013ee80`: all 12 listed CI stages passed. Those checks are separate from the IPC harness; portable driver tests are not driver install/runtime acceptance.

The user then supplied the actual elevated-admin IPC command and PowerShell parser output. It reports two **script bugs** in `Test-IpcAcceptance.ps1`: interpolating `$session:` in strings at lines 253 and 256 is parsed as a scoped variable reference and fails with `InvalidVariableReferenceWithDrive`. I fixed both as `${session}:`. A repository-wide scan found the same parse defect in `Invoke-RigBMeasurements.ps1` (`$exit:` in an interpolated error message); I fixed it as `${exit}:` because CI parses every `.ps1`. These are harness syntax bugs, not product/IPC bugs. The parser stops before the script body runs, so no admin test case or policy mutation occurred; no case is accepted.

The separate hosted Actions runs [41019982666](https://github.com/eTarikhi/BalanciFy/actions/runs/41019982666), [41020076222](https://github.com/eTarikhi/BalanciFy/actions/runs/41020076222), and [41028177906](https://github.com/eTarikhi/BalanciFy/actions/runs/41028177906) failed at `Parse-check every repository PowerShell script`, before PSScriptAnalyzer, service build, or IPC. The user-supplied parse errors explain the acceptance-script failure; the `$exit:` defect was another repository-wide parse failure candidate. Both syntax fixes are in this branch update; rerun the parser job to confirm. PSScriptAnalyzer and hosted IPC results remain unavailable.

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

The 12 local CI PASS checks do not need rerunning based on the supplied report. They are not IPC acceptance cases. Since the earlier IPC failure was not accompanied by a case ID or case log, if that symptom still needs investigation, rerun the full role-specific group below; do not rerun an unrelated group, and do not mark any IPC case accepted until its JSON and transcript are reviewed.

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
