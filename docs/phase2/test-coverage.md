# Test coverage — 215+ tests, what is covered

Date: 2026-10-20
Status: continuous, CI green ubuntu+windows

## Counts

- core: 82 + 3 ratelimit + 3 ECN/delay already counted? Actually core has 85+? Let's list:
  - bond.rs: WRH minimal disruption, removing uplink moves only flows that were on it
  - congestion.rs: 13 tests (slow start, halving once per burst, floor, per-path independence, LIA, ECN 80% vs 50% loss, burst once, delay detection 10% reduction, ack_with_rtt, ecn_ce, delay_events_path, rtt_min/last)
  - flowtable.rs: flow affinity, eviction, etc.
  - health.rs: health hysteresis fast drop / slow return Sec.6.5
  - keepalive.rs: keepalive scheduler per-path, traffic arms timers, expire_all
  - mtu.rs: adapter MTU min(live path MTU) - overhead, Sec.6.1
  - pacing.rs: paces at configured rate, allows when idle, respects max delay, per-path, forget/reset, rate update, prevents burst 14KB at 10Mbit/s spread over ~11ms, 7 tests
  - placement.rs: WRH placement
  - policy.rs: PolicyEngine wrapper for on_flow_packet
  - ratelimit.rs: TokenBucket allow within burst, hold when over, refill over time, 3 tests
  - rules.rs: FlowClass classification, curated defaults, duplicate eligible
  - scoring.rs: CapacityEstimator on_packet_sent, on_flow_packet, rolls window, notes traffic, suppresses idle probe, ACK pacing window, sleep/resume restarts window >90 Mbit/s vs stale <80
- crypto: 36 tests
  - lib.rs: 8 tests (16-byte tag only overhead, plaintext absent from ciphertext, every bit position refused, header bound as AAD, nonce separated by path/seq/salt/key, 1024 nonces distinct, wrong-length key refused, 20-payload stream end-to-end)
  - handshake.rs: 12 tests (both sides derive same two keys, pinned relay identity authenticates, every byte of flight 1 authenticated, prologue binds context, initiator static travels encrypted, fresh ephemeral gives fresh session, flights out of order refused, truncations refused, negotiated keys drive tunnel lane, pinning enforced both sides, replay cache rejects second flight)
  - keys.rs: static keypair derives public, ephemeral zeroized on drop, pinned keys pin/unpin, OS RNG distinct
  - store.rs: memory store generates/remembers, file store roundtrips, file store rejects corrupt
  - replay.rs: 5 tests (new ephemeral not replay, same within window is replay, expired not replay, capacity evicts oldest, different ephemerals not replay)
  - lib.rs rekey: 3 tests (rekey changes key but keeps salt, rekey deterministic and one-way, transport keys rekey both directions)
- ipc: 17 tests (wire contract, message round-trips, truncation/partial-frame handling, strict decoding, stable opcodes, loopback-only binding, fragmented/pipelined transport, event push, malformed-frame rejection, and 60-second timeout for idle unsubscribed clients)
- relay: 12 tests
  - Endpoint: two_paths_with_delay_gap_reassemble_in_order, ignores_garbage, deduplicates_duplicate_mode_copy, answers_probes, drains_gap_after_hold_cap, ordered_stream_survives_a_real_udp_socket
  - SecureRelay: secure_handshake_creates_session, rejects_unpinned_client, rejects_replay, secure_tunnel_after_handshake, enforces_rate_limit_and_session_cap, rate_limits_handshakes_per_ip
- service: 30+ tests
  - bonded_flow_is_striped_across_both_uplinks, flow_level_mode_keeps_a_flow_on_one_uplink, duplicate_mode_sends_critical_packets_twice, no_healthy_uplink_refuses_instead_of_black_holing, an_idle_peer_session_releases_its_slot, an_unanswered_probe_is_forgotten_after_the_probe_timeout, a_capacity_sample_lands_on_the_uplink_that_carried_the_data, sleep_resume_abandons_dead_state_and_requires_reprobing, a_nat_rebinding_rekeys_the_path_and_keeps_the_session, a_full_window_holds_the_packet_instead_of_dropping_it, acknowledgements_reopen_the_window, tick_emits_keepalives_and_a_bounded_probe, probe_reply_becomes_rtt_and_capacity, ipc_status_reflects_the_live_system, ipc_rule_edits_reach_the_classifier, ipc_rejects_an_absurd_event_rate, ipc_telemetry_snapshot_contains_warnings_and_counter_values, self_test_reports_no_capacity_before_probes, bond_revert_event_carries_the_measured_gain, adapter_mtu_follows_the_narrowest_live_path, metered_uplink_stays_out_until_the_user_allows_it, uplink_loss_fails_over_and_flows_keep_their_affinity, secure_handshake_establishes_transport_keys, pinning_enforced_rejects_unpinned_relay, relay_failover_switches_on_timeout, rekey_auto_trigger_by_packet_count, per_flow_sessions_assign_distinct_ids, per_class_sessions_assign_distinct_ids, rate_limiter_holds_when_over_limit, ecn_and_delay_wire_via_ingest, bundle_includes_per_path_ecn_delay
- tunnel: 31 tests
  - DRR proportionality 3000:1000 quanta give exactly 15:5 packets over 20 MTU-scale sends, quantum smaller than one packet is still served, path returning from idle cannot bank burst, sender_numbers_and_spreads, sender_without_paths_refuses_rather_than_panicking, ack_frees_the_retx_window_and_reports_bytes, retx_window_is_bounded, etc.
  - ECN: ack_with_ecn FLAG_ECN_CE=0x04
- e2e: 9 tests (striped_flow_is_reordered_back_across_a_40ms_delay_gap, reinjection_recovers_a_lost_packet_inside_the_hold_cap, unplugging_an_uplink_mid_transfer_keeps_the_stream_alive, flow_level_mode_is_not_held_up_by_the_other_path, duplicate_mode_delivers_each_packet_once, shell_and_service_speak_contract_v1_over_a_stream, secure_handshake_over_real_udp_delivers_encrypted_stream, etc.)
- fuzz: 6 tests (20k rounds each for datagram and IPC-frame loops, no panic, no invented payload, session and hold caps bound memory, length checks hold, replay cannot manufacture delivery)
- app: 0 tests (lab CLI)
- installer: 2 tests (journaled transactional, dry-run)
- crypto bench, drr-bench, pacing-bench, ecn-bench, throughput.rs, etc. (binaries, not tests)

Total: 215+ tests, 0 failures, identical on Linux and Windows runners (windows had transient failures fixed: E0503 borrow checker in delay detection, E0063 Counters missing ecn_ce/delay_events, truncated test with optional tail, FlowClass vs BondClass, WindowsEtwHandle case, bench imports, rate limiter burst 1000<1036, non-ASCII Sec. markers, format! escaping, on_ack_with_rtt arg order)

## Gaps (deliberate, in scope for rest of Phase 2/3)

- No Cargo.lock yet (no toolchain in sandbox, crates.io blocked)
- Simulated paths are not netem (delay and loss applied to frames in discrete-time rig, S2c real measurements pending, harness exists netem-rig.sh + throughput.rs)
- Miniport in the loop is functional stub + WDF control device, not yet S2b socket test through it (needs test-signed load with Driver Verifier)
- Flow-level capacity now counts flow packets + probes, but no explicit FlowData ACK
- Retx still keyed by seq only, not (session, seq) — per-flow/per-class sessions share global unique seq for retx simplicity
- Wintun fallback is stub, not real Wintun.dll bundling (feature-gated, exception file check, ADR-0009)
- Throughput rows simulated, not real Rig B (wired 1 Gbps, Wi-Fi 6 300 Mbit/s, secure mode, pacing 80% capacity, per-flow off/on, 10 reps, iperf3 pending)

## References

- Blueprint Sec.10.3 acceptance rows, Sec.10.1 10 reps median/p95/spread, Sec.6.4 DRR+pacing+CC, S13 SBOM/VEX, C2/C4
- CI: 39237159469 success
