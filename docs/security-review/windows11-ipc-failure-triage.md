# Windows 11 IPC failure triage and rerun matrix

## Failure-log availability

The user supplied a Windows 11 local full-CI summary for HEAD `e63e1439b1ac2ad763e426e7dec249b6d013ee80`: all 12 listed CI stages passed. Those checks are separate from the IPC harness; portable driver tests are not driver install/runtime acceptance.

The user then supplied the Admin and Standard harness output after pulling commit `b147226`. This confirms the PowerShell syntax fixes worked. The Admin command reached `execution-context` but failed because the caller was not elevated: `ETARIKHI\\Administrator`, session 1, `elevatedAdmin=False`, while `-Role Admin` requires `IsInRole(Administrator)` to be true. That is a **test setup/context failure**, not a product bug; no Admin mutations or further Admin cases ran.

The latest user-supplied reruns show the Admin context passed with `elevatedAdmin=True`, but Ping, GetStatus, machine-policy-write, and the session-0 probe all timed out before a request/response. The service then failed with Windows OS error 1307 (`This security ID may not be assigned as the owner of this object.`) while opening the policy store. `Get-Acl` showed the half-initialized folder owned by `ETARIKHI\Administrator` with inherited Authenticated Users Modify access; this is evidence of ACL initialization failing, not an IPC protocol result. The subsequent `-Role Standard` invocation was run from that same elevated Administrator token and failed its context gate before connecting. All IPC protocol cases remain **NOT RUN / NOT ACCEPTED** until the service starts as LocalSystem, logs both policy-store and listener startup success, and the harness preflight receives Pong. Do not weaken the ACL.

In the earlier separate Standard attempt, `execution-context` passed, but Ping/GetStatus, policy, idle, partial-header, and stalled-payload cases failed on `NamedPipeClientStream.Connect()` timing out after 5 seconds. The service had not reached listener startup, so no IPC request was sent; these were downstream setup failures, not protocol results. The latest Standard invocation instead failed `execution-context` because it was launched from the elevated Administrator token. No case is accepted from either run.

The syntax fixes are in pushed commit `b147226`. Hosted run [41038076670](https://github.com/eTarikhi/BalanciFy/actions/runs/41038076670) passed the repository-wide PowerShell parser, but PSScriptAnalyzer failed and the service-build/IPC steps were skipped. The follow-up in [41042199740](https://github.com/eTarikhi/BalanciFy/actions/runs/41042199740) on commit `ef6f142` passed PowerShell parsing, ran PSScriptAnalyzer and the service build, then reached the hosted IPC step; IPC failed and the final analyzer-enforcement step also failed. The findings/IPC evidence artifact was uploaded, but this environment could not download it (the artifact host returned EOF), so exact analyzer rules and case-level IPC results remain unavailable. The quoted `IsInRole` and `Get-Acl` errors came from `cmd.exe`, so they are command-shell errors, not PSScriptAnalyzer findings. Run those commands in Windows PowerShell (for example, start `powershell.exe` first); use the exact policy path printed by the service diagnostic when inspecting its ACL.

The workflow now places rule/severity/file/line/message details in the Actions job summary and JSON artifact, records analyzer failure without skipping IPC, and enforces the analyzer result after evidence upload. Follow-up run [41042559115](https://github.com/eTarikhi/BalanciFy/actions/runs/41042559115) on `f5ada7f` also passed parsing and service build, ran the hosted IPC step (failed), and failed analyzer enforcement. Artifact download returned EOF; check annotations contained only generic step failures. `PSScriptAnalyzerSettings.psd1` enables default rules and has no exclusions. Analyzer failure is confirmed, but no rule-level finding can yet be claimed fixed. The Rust workspace tests passed on `ef6f142`, including the diagnostic unit tests; this is hosted CI evidence only.

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

The 12 local CI PASS checks do not need rerunning. The supplied Admin transcript confirms that the service did not start and no IPC request reached it. Build the debug binary, then follow the exact [`RUNBOOK.md` LocalSystem/PsExec `-i -s` procedure](../../RUNBOOK.md); **do not launch the debug host directly from an elevated Administrator shell**. Keep the SYSTEM-hosted service running and confirm `[startup] open machine-policy store: OK` and `[startup] start local IPC v1 listener: OK`. The updated harness preflights the pipe with Ping; if the service is unavailable it records `service-reachable` as failed, marks requested protocol cases `NOT RUN`, and exits without repeated connect timeouts. If the process is running but the pipe is absent, retain its startup output and resolve that setup issue before testing. For Standard, use the separate non-admin local account; for Admin, use an elevated interactive PowerShell and verify the token. Do not mark a case accepted until the listener and case-level evidence JSON/transcript are reviewed.

### A. Standard-user authorization + transport group

On the Windows 11 VM, use a non-elevated standard local user and native Windows PowerShell 5.1:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\Users\Public\BalanciFy-Ipc\Test-IpcAcceptance.ps1 `
  -Role Standard -OutputDirectory C:\LabEvidence\ipc-standard-rerun
```

Cases to capture exactly:

- `execution-context`
- `service-reachable`
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
powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\Users\Public\BalanciFy-Ipc\Test-IpcAcceptance.ps1 `
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
powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\Users\Public\BalanciFy-Ipc\Test-IpcAcceptance.ps1 `
  -Role Admin -ReadOnlyOnly -OutputDirectory C:\LabEvidence\ipc-admin-transport-rerun
```

Capture `execution-context`, `service-reachable`, Ping/GetStatus and all three transport cases listed in group A. `-ReadOnlyOnly` intentionally omits every machine-policy mutation; it cannot replace group A or B.

## How to classify after logs are supplied

- **Script bug candidate:** parser/runtime exception in the harness before a well-formed request is sent; malformed locally generated frame; evidence says wrong command/role/pipe; no corresponding service request reached the pipe; or timeout elapsed-time logic incorrectly maps an observed close to PASS/FAIL. Reproduce with the same PowerShell version and a wire-level trace where practical.
- **Product bug candidate:** harness sent the documented well-formed request under a verified role and the service returned the wrong status, changed policy despite denial, failed to restore state, accepted session 0, or violated the documented timeout/frame handling. Confirm service logs, token/session identity and request bytes before assigning root cause.
- **Environment/setup issue:** service missing/not ready, wrong named pipe/build, UAC token/session mismatch, VM policy, scheduled-task service unavailable, test package mismatch, or a host timeout that does not match the service's configured clock. Do not relabel setup failures as product acceptance.

Current status for every listed IPC case remains **NOT RUN / NOT ACCEPTED** until the raw evidence is supplied and reviewed.
