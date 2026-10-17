# Server-mode infrastructure cost model (template — PROPOSED, unmeasured)

Fill during Phase 1 for two candidate regions (gate G1). All figures are cost
*drivers* and formulas, not quotes; replace `<...>` with vendor quotes.

## Cost drivers per region

| Driver | Formula | Notes |
|---|---|---|
| Egress bandwidth | `GB_month × $<egress>/GB` | Dominant term; bonding multiplies egress by ~1.0 (single exit) but raises volume per user |
| Instance compute | `instances × $<vcpu-ram>/h × 730` | Size from G4 CPU/Gbit/s measurement (§10.3) |
| IPv4 addresses | `static_ips × $<ip>/mo` | One stable exit address per relay (§4.1) |
| Inter-region backhaul | 0 for single-exit design | Avoided by design |
| Ops/monitoring/abuse | flat ` $<ops>/mo` | Status page, alerting, abuse handling (§9) |

## Per-user monthly cost

```
cost_user = (avg_GB_user × egress_price) + (share_of_instance + share_of_ip + share_of_ops)
```

## Acceptance rule (G1)

Proceed only if `price_plan > cost_user × 3` (gross-margin floor) **and**
median relay RTT added ≤ proposed 3 ms wired / ≤ 15 ms regional mobile
(§10.3 added-delay row, measured in Spike S2). Otherwise: narrow to flow-level
balancing with user-self-hosted server mode only (T1-self-host).

## Regions to quote (template)

| Region | Egress $/GB | Instance $/h | RTT to target users (ms) |
|---|---|---|---|
| `<region-1>` | <...> | <...> | <...> |
| `<region-2>` | <...> | <...> | <...> |
