# Acceptance evidence template

Copy one or more rows per criterion into `docs/completion-ledger.md` after evidence review. Do not replace `NOT RUN` / `BLOCKED` with a pass based on source inspection, compilation, or an incomplete run.

## Ledger-compatible criterion row

| Criterion | Source/location | Command/evidence | Evidence class | Status / discrepancy |
|---|---|---|---|---|
| `[criterion ID and short name]` | `[paths / artifact links]` | `[commit SHA; exact command; run ID or evidence-file path; measured result]` | `[Source / Unit-CI / VM-lab / Hardware / External / Blocked]` | `[NOT RUN / BLOCKED / PASS / FAIL; reviewer; caveats; next step]` |

## Per-case evidence record

| Case ID | Command | Expected | Observed | Result | Evidence file / log hash | Owner / reviewer |
|---|---|---|---|---|---|---|
| `[ID]` | `[exact command]` | `[measurable criterion]` | `[actual output / metric, not target]` | `NOT RUN` | `[path or URI + SHA-256]` | `[name/role]` |

## Environment and measurement record

| Field | Value |
|---|---|
| Commit / source SHA | `[full SHA]` |
| Artifact SHA-256 / signer | `[hash, signer thumbprint or N/A]` |
| Evidence class | `[Unit/CI / VM-lab / Hardware / External]` |
| Windows edition/version/build | `[value]` |
| Architecture / hardware | `[x64/ARM64; VM generation or Rig ID; NIC model/driver]` |
| Account privilege/session | `[standard/admin; interactive/session ID]` |
| HVCI/VBS/Secure Boot/test-signing | `[raw state; do not infer]` |
| Uplink / relay / route configuration | `[named values; redact secrets and identifiers]` |
| Repetitions / duration | `[N, run duration, soak/resume cycles]` |
| Tool versions / commands | `[exact versions and exact commands]` |
| Raw evidence | `[JSON/CSV/log/capture paths; hashes]` |
| Target threshold | `[separately state the ledger target]` |
| Measured result | `[median/p95/spread/variance or exact outcome]` |
| Reviewer decision | `[PASS / FAIL / INCOMPLETE / NOT RUN, reviewer and date]` |
| Follow-up / rollback | `[action and owner]` |

## JSON + transcript envelope example

Use this structure for a single evidence file when scripts produce both structured JSON and a transcript log. Values below are placeholders, not test results.

```json
{
  "evidenceType": "[criterion / evidence class]",
  "status": "NOT RUN",
  "commit": "[full source SHA]",
  "environment": {
    "windowsCaption": "[value]",
    "windowsVersion": "[value]",
    "windowsBuild": "[value]",
    "architecture": "[value]",
    "account": "[role/session; redact user name if needed]"
  },
  "cases": [
    {
      "id": "[case ID]",
      "command": "[exact command]",
      "expected": "[measurable expected result]",
      "observed": "NOT RUN",
      "result": "NOT RUN",
      "evidenceFiles": []
    }
  ],
  "measurements": {
    "observed": "[actual value or NOT RUN]",
    "unit": "[unit]",
    "repetitions": 0,
    "median": null,
    "p95": null,
    "sampleVariance": null,
    "standardDeviation": null,
    "coefficientOfVariationPct": null
  },
  "targets": {
    "threshold": "[ledger target; not measured evidence]"
  },
  "transcriptLog": ["[timestamped output lines]"]
}
```

## Handling and review

- Remove credentials, private keys, user names, public IPs, relay keys, packet payloads, and other sensitive data before sharing. Preserve the unredacted evidence in the approved restricted store if necessary.
- Keep measurements separate from target thresholds and summarize variance. Store raw CSV/log/capture files with hashes; do not substitute screenshots for raw data.
- Label WDK/CI artifacts exactly **“build-only evidence, not runtime acceptance.”**
- Hardware/VM evidence must name the actual machine, OS, driver, package, relay and run configuration. Failed and partial tests are evidence too; retain them rather than overwriting.
