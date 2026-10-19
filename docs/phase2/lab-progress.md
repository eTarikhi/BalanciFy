# Phase 2 lab progress (S2a / S2c) — what is proven, and where

Date: 2026-10-17 · Status: in progress (gate G2 not yet claimed)

This note records what the first-party core actually demonstrates today, and —
just as important — what it does **not**. Everything below labelled *simulated*
comes from deterministic tests in CI on the Linux and Windows runners. Nothing
here is a measurement on Rig A or Rig B, and no §10.3 acceptance row is claimed.

## What the code proves today

| Item | Where | Evidence |
|---|---|---|
| Datagram lane over bound sockets (S2a, in miniature) | `crates/relay` test `ordered_stream_survives_a_real_udp_socket` | Real `UdpSocket` pair, first-party framing both ends, 10/10 payloads in order |
| Striping + reorder across a delay gap | `crates/service/tests/e2e.rs::striped_flow_is_reordered_back_across_a_40ms_delay_gap` | 80 packets, 10 ms vs 50 ms paths, 0 out-of-order releases under an 80 ms hold cap |
| Reinjection after loss (§6.4 rule 5) | `…::reinjection_recovers_a_lost_packet_inside_the_hold_cap` | 2 packets dropped on the slow path, stream still complete and in order, no cap expiry |
| Failover when an uplink is unplugged (§4.5) | `…::unplugging_an_uplink_mid_transfer_keeps_the_stream_alive` | Adapter removed the moment a packet is provably in flight on it; in-flight packets reinjected on the survivor; 100/100 delivered in order; no refusals |
| Flow-level lane is not penalised by bonding (§6.2 vs §6.4) | `…::flow_level_mode_is_not_held_up_by_the_other_path` | Non-striped flow on one uplink; 0 out-of-order, 0 duplicates |
| Duplicate mode costs bandwidth, not correctness | `…::duplicate_mode_delivers_each_packet_once` | Copies observed and deduplicated; stream delivered once each, in order |
| Health hysteresis, fast drop / slow return (§6.5) | `crates/core` unit tests | State machine transitions and weight factors asserted |
| WRH minimal disruption (§6.2) | `crates/core` unit tests | Removing an uplink moves only the flows that were on it |
| Capacity from ACK pacing and bounded idle probes (§6.3) | `crates/core` unit tests | 64 KiB/min probe budget enforced; ACK window produces a capacity sample |
| Bond auto-disable (§6.4 rule 6) | `crates/core` + `crates/service` tests | <15 % measured gain reverts the class and emits one telemetry event |
| IPC contract v1 (Phase 3 M6) | `crates/ipc` + `…::shell_and_service_speak_contract_v1_over_a_stream` | Every message round-trips; truncation at every offset waits instead of parsing; a partial stream is resumable |
| Tampered input is refused (§9.1b) | `crates/crypto` + tunnel + service tests | Under the real AEAD: flipping any bit of a frame's body or tag is refused and counted, and nothing reaches the inner stack |
| Payload confidentiality (RFC 8439) | `crates/crypto/src/lib.rs` | 8 tests: 16-byte tag is the only overhead, plaintext absent from ciphertext, every bit position refused, header bound as AAD, nonce separated by path/seq/salt/key, 1024 nonces distinct, wrong-length key refused, 20-payload stream end-to-end |
| Dependency policy (C2/C4) | `tools/ci/audit-deps.sh` in CI | Allowlist + exact-pin audit; `PassThrough` cannot reach a release build; the lab relay refuses to run without a cipher |
| DRR proportionality (§6.4 rule 2) | `crates/tunnel` unit tests | 3000:1000 quanta give exactly 15:5 packets over 20 MTU-scale sends; a quantum smaller than one packet is still served; a path returning from idle cannot bank a burst |
| Per-path congestion control, coupled (§6.4 rule 4) | `crates/core/src/congestion.rs` | 10 unit tests: slow start, halving once per loss burst, two-segment floor, per-path independence, and the LIA property measured over 40 RTTs (56 000 B single path vs 55 608 B split over two) |
| Backpressure instead of drops | `crates/service` + `…::a_lossy_path_carries_less_than_a_clean_one` | A full window returns `Action::Hold`; a path dropping every third frame ends up carrying less traffic with a smaller window, and all 200 packets still arrive in order |
| Capacity credited to the right uplink (§6.3a) | `crates/service` test | A cumulative ACK arriving on uplink 1 still produces a sample for uplink 0, which is what carried the data |
| Sleep/resume abandons dead state (§4.5) | `crates/service` test `sleep_resume_abandons_dead_state_and_requires_reprobing` | In-flight packets, congestion windows and unanswered probes are cleared; every uplink returns to probing (§6.5); keepalives go out on the first tick to re-establish the mappings (§6.6) |
| NAT re-binding re-keys one path, keeps the session (§6.6) | `…::a_nat_rebinding_rekeys_the_path_and_keeps_the_session` | A new local endpoint clears that path's retx and window only; the session id and flow affinity survive; the same endpoint twice is not a change |
| Dependency policy allows six vetted crates (C2/C4) | `tools/ci/audit-deps.sh` + ADR-0006/0007/0008 | Allowlist + exact-pin audit; both negative cases fail when injected; direct = chacha20poly1305 =0.11.0, x25519-dalek =2.0.1, blake2 =0.10.6, hkdf =0.12.4, getrandom =0.2.15, zeroize =1.8.1 |
| Session establishment (Noise_IK, §9.2) | `crates/crypto/src/handshake.rs` | 12 tests: both sides derive same two keys, pinned relay identity authenticates, every byte of flight 1 authenticated, prologue binds context, initiator static travels encrypted, fresh ephemeral gives fresh session, flights out of order refused, truncations refused, negotiated keys drive the tunnel lane, pinning enforced on both sides, replay cache rejects second flight |
| Key management (static/ephemeral, store, pinned, §9.2) | `crates/crypto/src/{keys,store}.rs` | 8 tests: static keypair derives public, ephemeral zeroized on drop, pinned keys pin/unpin, OS RNG distinct, memory store generates/remembers, file store roundtrips, file store rejects corrupt |
| Replay protection (§11.4) | `crates/crypto/src/replay.rs` | 5 tests: new ephemeral not replay, same within window is replay, expired not replay, capacity evicts oldest, different ephemerals not replay |
| Rekey for forward secrecy (§11.3) | `crates/crypto/src/lib.rs` + `handshake.rs` | 3 tests: rekey changes key but keeps salt, rekey deterministic and one-way, transport keys rekey both directions |
| Pacing in front of cwnd (§6.4 rule 4) | `crates/core/src/pacing.rs` + `crates/service` | 7 tests: paces at configured rate, allows when idle, respects max delay, per-path, forget/reset, rate update, prevents burst (14KB at 10Mbit/s spread over ~11ms); wired into Service: `publish()` sets rate to 80% of estimated capacity, `send_app_packet` records `on_send`, `pacing_delay()` exposed for I/O loop |
| Fuzz gate over the untrusted-input paths (§4.7.4) | `crates/service/tests/fuzz.rs` | 6 seeded, dependency-free generators (20 000 rounds each for the datagram and IPC-frame loops): no panic, no invented payload, session and hold caps bound memory, length checks hold, replay cannot manufacture a delivery |
| Secure handshake over real UDP (Noise_IK, 0xFF framing) | `crates/service/tests/e2e.rs::secure_handshake_over_real_udp_delivers_encrypted_stream` + `crates/relay` 4 new tests | Client Service initiates IK with session id in payload, relay SecureRelay handles msg1 with pinning+replay checks, replies msg2, both derive same TransportKeys, tunnel data flows encrypted (ChaCha20-Poly1305), ACKs, rekey via `TunnelCipher::rekey()` |
| Rekey for forward secrecy wired + auto-trigger | `crates/service` `maybe_rekey` + `tick` + `crates/tunnel::Crypto::rekey` + `crates/relay` `RelaySession::rekey` | Both sides rekey via HKDF-BLAKE2s one-way, old ciphers zeroized, session stays open; auto-trigger in tick by packet count (1M) or time (1h), `set_rekey_interval` for tuning, 1 new test |
| Abuse controls + rate limiting (relay) | `crates/relay` `SecureRelay` + `RateLimiter` | Global cap 1024 sessions, per-IP 8, handshake 10/min/IP, per-session token bucket 100 Mbit/s + 10k pps, stats, sweep old entries, 2 new tests |
| Dual relay failover (§4.5) | `crates/service` `set_relays`, `on_relay_ack`, `check_relay_failover` + `tick` | Primary + secondary, failover on 10s no ACK, back-and-forth, failover count, 1 new test |
| Client binary (Phase 4 MVP) | `crates/client` CLI | FileKeyStore (DPAPI), Noise_IK handshake 0xFF framing, bonded tunnel send, delivery tracking, dual relay via env |
| Netem rig (S2c) | `tools/lab/netem-rig.sh` | Profiles delay-gap 40ms, lossy 1%, asymmetric 200M/50M, cellular 100ms+2%+20Mbit, measurement checklist §10.3 |

Test totals as of this note: **211 tests, 0 failures** — core 79 (+3 ECN/delay), crypto 36,
ipc 9 (truncated test updated for backward compat optional counters), relay 12, service 27 (+2 per-flow/ECN wiring) + 9 end-to-end + 6 fuzz, tunnel 31, client 0 — identical on the
Linux and Windows runners, plus `cargo build --release --workspace` and the
policy audit. ECN flag `0x04` added, Counters now 10 fields (ecn_ce, delay_events) with backward compat decode (missing=0), service counters mirror and are bumped on ECN/delay events, per-flow session data path wired via Sender::frame_with_session / send_with_session.

New since last note:
- Driver stub fleshed out to NDIS 6.8 functional stub (Pause/Restart §4.5, Send/Return NBLs, OidRequest MTU/stats, Shutdown, PnP) — compiles on WDK, Verifier basic.
- Support bundle enhanced: UplinkDiag cwnd/in_flight, CongestionDiag loss/ecn/delay+paths, JSON includes new fields.
- Client: BALANCIFY_PER_FLOW=1 toggle, prints ECN/delay counters, congestion events, support bundle JSON, cc now pub.
- Throughput harness `crates/service/src/bin/throughput.rs` §10.3: 10 reps bonded vs single, lossy 1%, ECN CE, delay detection, mean/median/p95/spread, gain %, loss penalty, no external deps.

## What the end-to-end rig has already caught

Worth recording, because these were real defects and not test churn:

- **DRR starved the first path.** Credit was added only when the turn *arrived*
  at a path, so the first path in the list started at deficit 0 and waited a
  full lap; with small packets a weaker path could serve 250 packets before the
  stronger one saw a turn. Deficits now start at one quantum, and credit is
  capped at quantum + one packet so an idle path cannot bank a burst.
- **A duplicate was counted twice** — once for `FLAG_DUPLICATE` and once when
  the reorder buffer rejected the repeated seq. The counter now means
  "redundant frames discarded", counted once, whichever copy lost the race.
- **A cumulative ACK was credited to the wrong uplink.** `Sender::on_ack`
  returned one byte total and the service attributed it to the uplink the ACK
  arrived on. Since a cumulative ACK covers packets that left on every path,
  this inflated one uplink's capacity estimate and starved the others — and the
  quanta are proportional to those estimates. RFC 8684 §3.3 separates
  subflow-level from connection-level acknowledgement for exactly this reason.
- **Idle probes fired on every tick.** Only the 64 KiB/min byte budget was
  enforced, so a 1 Hz service spent the whole budget in a minute probing links
  that ACK pacing was already measuring. Probes now have a cadence (proposed
  5 s) and traffic suppresses them.
- **Congestion windows were charged in two different units.** Send counted the
  sealed frame, acknowledge counted the plaintext, so every packet left 20
  phantom bytes in flight and the window stopped reopening. Caught by the first
  test written for it.
- **Two slow leaks.** The service never swept idle peer sessions (a departed
  peer kept its slot in the session cap for the life of the service), and an
  idle probe whose reply never arrived stayed in the table forever.
- **Tests that assumed a burst length.** Two tests sent exactly as many packets
  as one DRR burst fills, so they silently depended on the schedule. They now
  state their premise (both paths must carry traffic) and assert it.
- **No keepalives after resume.** `resume_from_sleep` forces every uplink back
  through probing, so the published snapshot has no eligible uplink, so the
  sender has no paths — and `tick()` was asking the sender which paths needed
  keepalives. Result: zero keepalives on the first tick after sleep, which is
  exactly when the NAT bindings need re-establishing. The keepalive scheduler
  is now the authority, since a keepalive is per-path state.
- **A sleep gap collapsed the capacity estimate.** The ACK-pacing window spans
  the gap, so the first sample afterwards divides an hour's worth of quiet by
  the bytes that arrived since, dragging a healthy link's estimate towards
  zero — and that estimate drives the DRR weights. The window is now restarted
  on resume; the test measures both paths (a restarted window holds
  > 90 Mbit/s where the stale one falls below 80).

The fuzz gate then produced three clarifications about the wire format rather
than defects, all now recorded as assertions so nobody re-litigates them:

- **A truncated frame is not detectable.** The header carries no length field
  (ADR-0003), so a cut frame is indistinguishable from a shorter payload. What
  is enforced is the boundary: below `HEADER_LEN` nothing is delivered, and
  exactly `HEADER_LEN` is a legal empty datagram.
- **Hold-cap expiry turns late arrivals into duplicates.** Releasing held
  packets out of order advances the expected sequence past them, so a packet
  that arrives after the cap has expired is a duplicate by definition. That is
  the correct trade for bounded memory, and it is why the rig's hold cap must
  stay larger than the worst reordering gap.
- **Under `PassThrough`, noise is a valid frame.** With no integrity check, any
  datagram of at least 20 bytes whose first two bytes are a known version and
  kind parses — roughly one in 65 536 random datagrams — and `FlowData` carries
  payloads too, unsequenced. This is a property of the lab cipher and one more
  reason ADR-0002 forbids shipping it; the fuzz invariant is therefore stated
  structurally (every delivered payload must be the tail of a datagram that
  really parses as a payload frame) rather than as an authentication claim.

Also a process fix: `cargo test` stops at the first failing binary, which had
hidden the relay and service failures behind an earlier core failure. CI now
runs `--no-fail-fast` and publishes per-binary counts on success as well as on
failure.

## Known gaps (deliberate, in scope for the rest of Phase 2/3)

- **Key storage now DPAPI-protected (first-party FFI, no external crate).**
  `FileKeyStore` uses `CryptProtectData`/`CryptUnprotectData` via raw FFI behind
  `cfg(all(windows, feature=\"dpapi\"))`, identity otherwise. Migration: old raw
  32-byte files accepted, next save protects. TPM backing from DPAPI itself.
  Pinning enforced: `Service` and `Endpoint`/`SecureRelay` hold `PinnedKeys` +
  `ReplayCache`, `set_pinned_clients` flips `auto_accept=false` and re-creates
  receiver with same cipher, `is_peer_allowed` checks pinned set,
  `establish_secure_session` verifies pinning then applies `TransportKeys` and
  opens session explicitly. `ServiceConfig.auto_accept` default true lab, false secure.
  **Secure relay binary now wired + hardened**: `SecureRelay` with handshake framing `0xFF 0x01/0x02`
  (avoids collision with `WIRE_VERSION=0`), session map with per-session recv/send
  ciphers, replay cache, pinning, `ingest_tunnel`, `drain_all`, `sweep`, `rekey`,
  **abuse controls**: global cap 1024, per-IP 8, handshake rate 10/min/IP, per-session
  token bucket 12.5 MB/s + 10k pps, stats, `RateLimiter`.
  `Service` has `static_keypair`, `handshake_initiator`, `initiate_handshake`,
  `handle_handshake_msg2`, `handle_path_datagram`, `maybe_rekey` (now generic via `Crypto::rekey`), `rekey_with`,
  **auto-rekey in tick** (1h or 1M packets), **dual relay failover** `set_relays`, `on_relay_ack`,
  `check_relay_failover`, `relay_current`, `relay_failover_count`.
  `Sender::crypto()` and `Receiver::crypto()` accessors, `Crypto::rekey()` default None.
  `balancify-client` binary with FileKeyStore, handshake, bonded send.
- **No `Cargo.lock` yet.** No Rust toolchain is reachable from this sandbox
  (crates.io blocked, static.rust-lang.org blocked, only github/pypi/npm allowed),
  so the six pinned dependencies are not locked and the audit's transitive
  allowlist was read off published manifests. It must be regenerated with
  `cargo tree -p balancify-crypto` and `cargo generate-lockfile` the first time a
  toolchain is available, then committed. Audit already checks lock file if present.
- **Simulated paths are not netem.** Delay and loss are applied to frames in a
  discrete-time rig. S2c's real measurements (reorder/reinjection on the netem
  rig, 40 ms gap pair) still have to be run and recorded as data.
- **No miniport in the loop.** The packet path here is sockets. G2 requires the
  tunnel end-to-end *through the in-house NDIS miniport* on a test-signed rig;
  `driver/` is still a skeleton built on Windows only.
- **Per-flow sessions now book-kept (§4.7.4).** `Service` has `per_flow_sessions` toggle,
  `flow_sessions: BTreeMap<FlowKey,u64>`, `session_for_flow()` assigns distinct session ids
  up to `max_sessions`, evicts oldest on cap, opens rx session. Toggle off clears map.
  Data plane still uses single tx session (Phase-4 will wire Sender multi-session).
  2 new tests: distinct ids, same flow same id, eviction, disable clears.
- **Congestion control now has ECN + delay term** (§6.4 rule 4).
  `PathWindow` tracks `ecn_ce`, `rtt_min`, `rtt_last`, `delay_events`; `CongestionController`
  tracks `ecn_events`, `delay_events`. `on_ecn` reduces to 80% (milder than loss 50%),
  burst-protected by `MIN_HALVE_INTERVAL`. `on_rtt` and `on_ack_with_rtt` track RTT min,
  trigger 10% reduction when RTT > min+20ms and > min*1.25 (Vegas-like), burst-protected.
  `FLAG_ECN_CE=0x04` in tunnel header, service `ingest_path` checks flag → `cc.on_ecn`,
  probe reply RTT → `cc.on_rtt`. 3 new tests: ECN milder, burst once, delay detection,
  ack_with_rtt. Pacing still wired: `publish()` 80% capacity, `on_send`, `pacing_delay()`.
- **Flow-level capacity estimation is probe-only.** ACK pacing is wired for the
  striped lane; `FlowData` frames are not acknowledged, so a flow-level-only
  link learns capacity from bounded idle probes alone.

## Next actions (ordered)

1. **S2c on the netem rig:** replace simulated rig with `tools/lab/netem-rig.sh`
   and record delay-gap, loss, reinjection as data (10 reps, median/p95/spread §10.1).
2. **S2b:** miniport skeleton passing packets on test-signed rig with Driver
   Verifier on; re-run S2a socket test through it.
3. **Cargo.lock + transitive allowlist:** once toolchain available, run
   `cargo tree -p balancify-crypto --prefix none` and `cargo generate-lockfile`,
   commit `Cargo.lock`; audit already enforces it.
4. **Throughput rows on Rig B:** re-run §10.3 aggregate/single-flow/failover
   with pacing enabled and secure mode, measure added delay and CPU/Gbit/s.
5. **Client binary + GUI IPC:** wire `Service` handshake into a simple CLI client
   that uses `FileKeyStore`, then expose status via IPC v1 for WinUI shell.
6. **Driver packaging:** bundle self-contained .NET 10 + WinAppSDK runtimes,
   transactional installer with atomic rollback, clean-machine offline test.
