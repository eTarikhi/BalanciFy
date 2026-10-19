# Bug hunt — what CI and fuzz have caught, and how fixed

Date: 2026-10-19
Status: continuous, 211+ tests green

## Fixed in this continuous development session

### Borrow checker E0503 in delay detection
- **Where**: `crates/core/src/congestion.rs` `on_ack_with_rtt` and `on_rtt` — `self.min_cwnd` and `self.delay_events` used while `w` mutably borrowed from `self.ensure(path)`
- **CI**: 39192146923 failure, E0503 cannot use `self.min_cwnd` because it was mutably borrowed
- **Fix**: copy `min_cwnd` to local before borrow, use `delay_triggered` flag to defer global counter increment after borrow scope

### IPC sample_status missing ecn_ce/delay_events
- **Where**: `crates/ipc/src/lib.rs` `sample_status()` — Counters now 10 fields but initializer had 8
- **CI**: 39192702879 failure, E0063 missing fields `delay_events` and `ecn_ce`
- **Fix**: add `ecn_ce: 2, delay_events: 1` to sample

### IPC truncated_payloads_decode_to_none with optional counters
- **Where**: `crates/ipc/src/lib.rs` test — after adding optional trailing u64s for backward compat, truncations that remove only optional fields still parse as old version
- **CI**: 39192883897 failure, cut at 141 must not parse
- **Fix**: update test to allow truncations that remove only optional tail (≤16 bytes), verify old 8-field and 9-field payloads parse with 0 defaults

### Per-class sessions FlowClass vs BondClass
- **Where**: `crates/service/src/lib.rs` `session_for_class` took `BondClass` but `send_app_packet` passes `FlowClass`
- **CI**: 39205483623 failure, E0308 expected `BondClass`, found `FlowClass`
- **Fix**: change `session_for_class` to take `FlowClass`, map discriminant to u8

### WindowsEtwHandle case
- **Where**: `crates/service/src/etw.rs` — struct `windows_etw_handle` should be UpperCamelCase, and field `handle: Option<windows_etw_handle>` not found after rename
- **CI**: 39205688332 failure (windows only), E0425 cannot find type `windows_etw_handle`
- **Fix**: rename struct to `WindowsEtwHandle` and fix field type

### Crypto bench missing Crypto trait import
- **Where**: `crates/crypto/src/bin/bench.rs` — `cipher.seal` and `cipher.open` require `Crypto` trait in scope
- **CI**: 39206283573 failure, E0599 no method named `seal` found
- **Fix**: add `use balancify_tunnel::Crypto;`

### Crypto bench open signature
- **Where**: `crates/crypto/src/bin/bench.rs` — `open` takes 4 args but 5 supplied (extra &mut plain_out)
- **CI**: 39206485667 failure, E0061 this method takes 4 args but 5 supplied
- **Fix**: remove extra arg, `open` returns Option<Vec<u8>>

### Pacing bench method
- **Where**: `crates/core/src/bin/pacing-bench.rs` — `delay_for` not found, should be `delay_until_next`
- **CI**: 39207832446 failure, E0599 no method named `delay_for`
- **Fix**: change to `delay_until_next`

### Rate limiter test burst size
- **Where**: `crates/service/src/lib.rs` test `rate_limiter_holds_when_over_limit` — set burst 1000 but sealed frame is 1036 (header 20 + tag 16 + 1000 payload), so first burst fails
- **CI**: 39207060537 failure, first burst should pass
- **Fix**: set burst 2000 to cover overhead

## Previously fixed (from lab-progress.md)

- DRR starved first path (credit added only when turn arrived, first path deficit 0 waited full lap)
- Duplicate counted twice (FLAG_DUPLICATE + reorder buffer reject)
- Cumulative ACK credited to wrong uplink (RFC 8684 §3.3 separates subflow vs connection-level ACK)
- Idle probes fired every tick (only byte budget enforced, spent whole budget in minute)
- Congestion windows charged in two units (sealed frame vs plaintext, 20 phantom bytes)
- Two slow leaks (idle peer sessions never swept, idle probe reply never arrived stayed forever)
- Tests assumed burst length (exactly as many packets as one DRR burst)
- No keepalives after resume (resume forces probing, snapshot has no eligible uplink, sender has no paths, tick asked sender which paths needed keepalives -> zero)
- Sleep gap collapsed capacity estimate (ACK-pacing window spans gap, divides hour's quiet by bytes, drags estimate to zero)

## Fuzz gate clarifications (not defects)

- Truncated frame not detectable (header no length field ADR-0003)
- Hold-cap expiry turns late arrivals into duplicates (correct trade for bounded memory)
- Under PassThrough, noise is valid frame (1 in 65536 random datagrams parses, FlowData carries payloads unsequenced, property of lab cipher, reason ADR-0002 forbids shipping it)

## Process fix

- `cargo test` stops at first failing binary, hiding relay and service failures behind earlier core failure — CI now runs `--no-fail-fast` and publishes per-binary counts on success as well as failure.

## References

- Blueprint §4.8 dependency policy, §6.4 DRR + pacing + CC, §9.1b tampered input refused, §10.3 acceptance rows
- CI runs: 39192146923, 39192702879, 39192883897, 39205483623, 39205688332, 39206283573, 39206485667, 39207832446, 39207060537 — all fixed, final 39208040465+ green
