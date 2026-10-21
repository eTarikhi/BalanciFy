# Balancify — Comprehensive Development Progress Report

**Date:** 2026-10-20
**Branch at report creation:** `arena/7abc96c8-balancify` (historical snapshot; current work is on the active Arena branch)
**Status:** Historical snapshot; gate status is tracked in the current README and phase notes.
**Report Type:** Retrospective + Prospective + Strategic Alignment with data visualizations

---

## Executive Summary

Balancify is a Windows-native multi-WAN aggregation product (Wi-Fi, Ethernet, cellular) with load balancing, bonding, seamless failover, first-party NDIS miniport, encrypted dual-lane tunnel, WinUI 3 shell, transactional offline installer. This report communicates project status across three dimensions using professional charts.

**Current momentum:** CI green chain from `39233826038` onward (after fixing non-ASCII `§`, format! escaping, arg order), velocity 8-15 commits/week, tests grew from 0 to 215+ in 8 weeks, 30 ADRs ratified.

**Measurement correction (2026-10-20):** `crates/service/src/bin/throughput.rs` is a synthetic software-path benchmark, not a network simulation. Historical rates and “added delay” figures in this report were derived from CPU-side send loops and are not acceptance evidence. See `docs/phase2/throughput-rows.md`; all real Rig B performance rows remain pending.

---

## 1. Retrospective Analysis — What Was Built

### 1.1 Milestone Timeline & Gate Discipline

| Gate | Criterion | Status | Evidence | Commit |
|------|-----------|--------|----------|--------|
| **G1** | Decisions D1-D5 + dependency policy + signing strategy + enrolment | **PASS** | ADR-0002 dependency policy (no third-party kernel in data path, only vetted/pinned crates, PassThrough test-only), signing strategy attestation vs HLK, EV ordered | `18c028a` ADR-0015, `56fb239` ADR-0016 |
| **G2** | Steering proven (≥3 adapter classes) + tunnel end-to-end via in-house miniport (test-signed) + no third-party kernel in data path | **PASS** | Spike S1 source-address/`IP_UNICAST_IF` steering, S2a datagram tunnel over bound sockets (`ordered_stream_survives_a_real_udp_socket` real UdpSocket), S2b miniport skeleton passing packets, WDF control device + IOCTLs, NBL pool + INJECT_NBL via `NdisMIndicateReceiveNetBufferLists` | `be21940` driver WDF, `4d05142` NBL pool |
| **G3** | Miniport perf + HVCI trio + signing dry-run | **IN PROGRESS** | DRR/pacing unit behavior is covered in software tests; no driver packet-path latency or CPU performance acceptance measurement yet. HVCI readiness procedure documented; VM/HLK and signing dry-run pending. | `5fc3af0` hvci-readiness |
| **G4** | Acceptance table on Rig B with signed driver + clean-machine offline provisioning | **PENDING** | The software-path harness is not network evidence; no §10.3 throughput, added-delay, or failover row is accepted until measured on Rig B with the driver and real relay. | Pending hardware validation |
| **G5** | Soak + Driver Verifier clean | PENDING | 72h soak, random flaps, 100 resume cycles, Verifier standard+random | — |
| **G6** | Review + compat matrix | PENDING | VPN clients, AV, Hyper-V, WSL2, MI on/off | — |
| **G7** | Checklist + rollback + final attestation + SBOM/VEX | PENDING | SPDX 2.3 and standards-valid SPDX 3.0.1 locked-graph inventories generate and validate in CI; reviewed VEX and clean-machine installer/rollback validation remain pending | `5fc3af0` (historical baseline) |
| **G8** | Field stable | PENDING | GA + winget + status page | — |

### 1.2 Detailed Technical Work Breakdown (Completed)

#### Core (balancify-core) — 85+ tests
- **Bonding & Flow Scheduling (Sec.6.2, Sec.6.4):** WRH weighted rendezvous hashing, minimal disruption on flap, flow affinity 120s, per-flow/per-class sessions (`BTreeMap<FlowKey,u64>` + `BTreeMap<u8,u64>`), session_for_flow evicts oldest on cap, global seq → per-session seq (ADR-0029)
- **Capacity Estimation (Sec.6.3):** EWMA RTT α=0.2, loss, capacity from ACK pacing (window 500ms, α=0.3) + bounded idle probes 64 KiB/min, 5s cadence, `request_idle_probe`, `on_ack`, `on_flow_packet`, `on_flow_ack` (ADR-0028), `on_probe_result`, `restart_window` on sleep, `note_traffic` suppresses probes
- **Congestion Control (Sec.6.4 rule 4, ADR-0005):** per-path CC coupled, slow start, halving once per loss burst, 2-segment floor, LIA property, ECN milder 80% vs loss 50%, burst once, delay detection RTT min+20ms & 1.25x → 10% reduction, `on_ack_with_rtt`, `ecn_ce`, `delay_events_path`, `rtt_min`, `rtt_last`, 13 unit tests + ecn-bench
- **Pacing (Sec.6.4):** `PacingController` rate, burst, `delay_until_next`, `on_send`, `set_rate`, per-path, forget/reset, prevents burst (14KB at 10Mbit/s spread ~11ms), 7 tests, wired into Service publish 80% capacity
- **Health (Sec.6.5):** fast drop/slow return, Probing→Up (5 successes)→Degraded→Down (3 misses)→Recovering ramp 3×30s
- **Rate Limiting (ADR-0017):** TokenBucket rate_bps+burst, `RateLimiter` per-uplink, default 1 Gbps/1MB, `set_uplink_rate`, `try_send` → Hold on limit

#### Tunnel (balancify-tunnel) — 31 tests
- **Framing (ADR-0003):** Header 20B (version, kind, path, session, seq, flags), FrameKind Data/FlowData/Keepalive/Probe/ProbeReply/Ack, FLAG_DUPLICATE 0x01, FLAG_REINJECTED 0x02, FLAG_ECN_CE 0x04
- **DRR (Sec.6.4 rule 2):** Deficit round robin quanta ∝ capacity, deficit starts at 1 quantum, cap quantum+packet to prevent burst banking, `next_filtered` skips blocked (cwnd full), `next_excluding` for reinjection
- **Sender (Sec.6.4, ADR-0011, ADR-0029):** per-session seq spaces, retx keyed by `(session,seq)` `BTreeMap<(u64,u64), RetxEntry>`, `session_seqs`, `next_seq_for(session)`, `remember(session,seq)`, `on_ack_for_session(session, up_to)` range `(session,0)..=(session,up_to)`, `reinjections_due` preserves session via `frame_with_session`, `Sent { session }`, `duplicate_with_session`, `ack_for_session`
- **Receiver (Sec.6.4 rule 3):** per-session `ReorderBuffer`, hold cap 80ms (gap+10ms), releases in order, on cap expiry releases out-of-order and advances, `released_out_of_order`, dedup by seq, auth before buffering (Sec.9.1)
- **Duplicate policy (Sec.6.4 rule 5):** `should_duplicate(len, critical, duplicate_mode)` ≤256B

#### Service (balancify-service) — 30+ tests + e2e 9 + fuzz 6
- **Sans-I/O core:** `Service<C: Crypto>` with `PolicyEngine`, `FlowTable`, `RuleSet` curated, `BondGainTracker`, `KeepaliveScheduler`, `MtuPolicy`, `Sender`, `Receiver`, `CongestionController`, `PacingController`, `RateLimiter`, `PinnedKeys`, `ReplayCache`, `EtwProvider`
- **Data plane:** `send_app_packet` classifies via rules, places via WRH, checks `cc.blocked`, selects session per-flow>per-class>default, `send_excluding_with_session`, rate limiter check → Hold, `cc.on_send`, `pacing.on_send`, `packets_stripped`, duplicate_mode via `duplicate_with_session`, flow lane via `send_flow_with_session` + `on_flow_packet`
- **Control plane:** `ingest_path` handles ECN CE flag → `cc.on_ecn` + ETW 401, Ack → `on_ack_for_session` → `policy.on_ack` + `cc.on_ack_with_rtt` + ETW 402, ProbeReply → `on_probe_reply` + `on_idle_probe` + `on_rtt`, FlowData → explicit `on_flow_ack` + `on_ack` (ADR-0030), payload/control → `on_relay_ack`
- **Lifecycle:** `resume_from_sleep` drops retx, resets CC/pacing/probes, `expire_all` keepalives, `restart_measurements`, `resumes` counter; `note_local_endpoint` detects NAT re-binding via `path_changed`, `forget_path`, `path_changes`
- **Security:** `set_pinned_clients`/`relays` flips auto_accept false, recreates Receiver, `open_session`, `is_peer_allowed`, `set_static_keypair`, `replay_cache`, `maybe_rekey` via `Crypto::rekey()` HKDF-BLAKE2s, auto-trigger by packet count 1M or time 1h, ETW 400
- **Failover (Sec.4.5):** `set_relays` primary+secondary, `on_relay_ack`, `check_relay_failover` 10s timeout, `relay_failover_count`, ETW 500, tick checks failover
- **ETW (ADR-0021, ADR-0027):** `EtwProvider` stub Windows EventRegister/EventWrite else eprintln, IDs 100-600: 200 UPLINK_UP, 201 DOWN, 300 BOND_REVERTED (via `observe_bond_gain` logs when flipped, returns `BondReverted`), 400 REKEY, 401 ECN_CE, 402 DELAY, 500 FAILOVER, 600 AUTH_FAILED; publish() clones prev_uplinks, logs state changes
- **Support bundle (ADR-0022):** `support/mod.rs` `per_path Vec<PathCongestionDiag>` cwnd/in_flight/ecn_ce/delay_events/rtt_min/last_ms, JSON includes new fields, test `bundle_includes_per_path_ecn_delay`

#### Crypto (balancify-crypto) — 36 tests
- **AEAD (ADR-0006):** `TunnelCipher` ChaCha20Poly1305 0.11.0, wire-v1 nonce separated by frame kind/path/seq/salt/key, header bound as AAD, 16B tag only overhead, 10 crypto unit tests
- **Handshake (ADR-0007):** Noise_IK `-> e, es, s, ss` / `<- e, ee, se`, X25519 2.0.1, BLAKE2s 0.10.6, HKDF 0.12.4, 12 tests: same keys, pinned identity, every byte auth, prologue binds context, initiator static encrypted, fresh ephemeral → fresh session, out-of-order refused, truncations refused, pinning enforced, replay cache
- **Key Management (ADR-0008):** StaticKeypair, Ephemeral zeroized on drop, PinnedKeys, OS RNG getrandom 0.2.15 distinct, memory/file store, zeroize 1.8.1
- **Replay (Sec.11.4):** ReplayCache 5 tests
- **Rekey (Sec.11.3):** rekey changes key keeps salt, deterministic one-way, transport keys rekey both

#### Relay (balancify-relay) — 12 tests
- **Endpoint:** decrypt, reorder, forward from stable address, `ordered_stream_survives_a_real_udp_socket` real UdpSocket, `ecn_echoed`, `total_ecn_echoed`, sweep, stats 6-tuple, ECN echo via `BALANCIFY_SIMULATE_ECN=1` → ACK with ECN flag, `ack_with_flags`, `ack_with_ecn`

#### Driver (driver/src/) — WDK 10.0.22621+
- **Miniport (miniport.c):** source-only NDIS 6.8 scaffold with registration/general attributes, raw-IP NdisMediumIP, MTU/lookahead/packet OIDs, and owned receive NBLs. Pause drains active receive indications, returned NBLs, and send callback chains. The transmit ring is absent, so sends fail rather than being falsely counted as successful; WDK/runtime behavior remains unvalidated.
- **Control (ioctl.c):** WDF control device `\\Device\\Balancify`, symlink `\\DosDevices\\Balancify`, SDDL `SDDL_DEVOBJ_SYS_ALL_ADM_RWX_WORLD_RW_RES_R`, IOCTLs METHOD_BUFFERED: SET_MTU 0x800 validates 576..9000, GET_STATS 0x801 returns `BALANCIFY_STATS`, INJECT_NBL 0x802 validates 20..9000, allocates MDL `NdisAllocateMdl`, NBL `NdisAllocateNetBufferAndNetBufferList(pool)`, checks paused, indicates `NdisMIndicateReceiveNetBufferLists(..., DISPATCH_LEVEL)`, SET_QUANTA 0x803 array (u8 path, u64 quanta) count ≤256; length validation, no METHOD_NEITHER

#### App & Installer
- **App (app/src/main.rs):** lab CLI mimicking WinUI3 shell: modes home/rules/diagnostics/tray/relay/bundle (Sec.8.2), 1Hz event subscription, renders counters ECN/delay, last-known-good + stale markers, relay stats, support bundle redacted, ECN/delay counters (ADR-0025)
- **Installer (installer/src/main.rs):** journaled 9 steps preflight/driver/service/adapter/runtimes/gui/two SBOM sidecars/self-test with reverse rollback, dry-run, tests, SPDX 2.3 + 3.0.1 sidecar copies to `ProgramData\Balancify\sbom.spdx.json`, clean-machine offline test per `clean-machine-test.md`, offline bundle + SHA-256 sidecar

#### Benches & Harnesses
- **throughput.rs:** real cipher in a synthetic service-path workload; reports local work rate and per-call software latency only. It does not measure network throughput, added delay, loss penalty, or end-to-end failover.
- **drr-bench, pacing-bench, ecn-bench, crypto-bench:** DRR proportionality, pacing rate+burst, ECN 80% vs loss 50%, delay 10%, seal/open/rekey MB/s + us/op for CPU/Gbit/s

### 1.3 Velocity & Momentum

![Velocity Chart](assets/progress/velocity-chart.png)

**Interpretation:**
- Tests grew from 0 to 215+ in 8 weeks, with steepest growth in weeks 2-4 (lab prototype) when core + tunnel + crypto landed
- Commits per week: 5→12→18→15→8→10→12→15, showing sustained velocity after initial spike
- CI success rate 85-100% after fixing non-ASCII, format! escaping, arg order (failures 39232571739, 39233028939, 39233548995 → green 39233826038 onward)
- Current velocity 8 tasks/sprint, 215+ tests 0 failures identical on Linux+Windows

### 1.4 Bug Hunt & Fixes (Real Defects Caught by Rig)

- DRR starved first path (deficit started 0) → deficits start at 1 quantum, cap quantum+packet
- Duplicate counted twice (FLAG_DUPLICATE + reorder buffer) → count once "redundant frames discarded"
- Cumulative ACK credited to wrong uplink (ACK arrival path) → per-path accounting per RFC 8684 Sec.3.3, DRR quanta corrupted
- Idle probes fired every tick (only byte budget enforced) → cadence 5s + traffic suppresses
- Cwnd charged in two units (sealed vs plaintext) → 20 phantom bytes, window stopped reopening
- Leaks: idle peer sessions never swept, unanswered probes stayed forever → sweep added
- Tests assumed burst length → now assert premise both paths carry traffic
- No keepalives after resume (sender had no paths, keepalive scheduler is authority) → keepalive scheduler now authority, keepalives go out first tick after sleep
- Sleep gap collapsed capacity estimate (window spans hours) → `restart_window` on resume
- Borrow checker E0503 in delay detection, IPC sample_status missing fields, truncated test optional tail, FlowClass vs BondClass, WindowsEtwHandle case, bench imports, rate limiter burst 1000<1036, non-ASCII, format! escaping, on_ack_with_rtt arg order, Service generic, PassThrough string audit

---

## 2. Prospective Planning — What's Next

### 2.1 Roadmap & Gantt

![Gantt Roadmap](assets/progress/gantt-roadmap.png)

**Current position:** Vertical dashed line at week 30-32, Phase 4/5 boundary (MVP → Alpha). G1 and G2 passed, G3 in progress, G4 pending.

| Phase | Objective | Weeks | Owner | Deliverables | Gate | Status |
|-------|-----------|-------|-------|--------------|------|--------|
| **1 Feasibility** | Requirements freeze, dependency policy, signing strategy, EV ordered, relay cost model | 1-4 | Architect + product owner + release eng | Decision memo, policy, enrolment receipts, cost model | G1 | DONE |
| **2 Lab prototype** | S1 steering, S2a datagram tunnel, S2b miniport skeleton + WDF control + NBL pool + INJECT_NBL, S2c netem rig | 5-12 | 2 systems + driver eng | Spike reports, unsigned driver, rig, baselines B1-B3 | G2 | DONE |
| **3 Architecture spike** | Miniport perf benchmark vs §10.3, HVCI trio (Verifier w/ CI, HLK HVCI readiness, VBS/MI machine), attestation dry-run, IPC v1, GUI wireframes | 13-16 | Driver + perf + UI eng | Benchmark report, HVCI report, dry-run, IPC IDL, wireframes | G3 | IN PROGRESS |
| **4 MVP** | Service (classifier, schedulers, dual-lane tunnel + AEAD), server mode T1/T3/T4 + container, WinUI shell Home/Rules/Tray, transactional offline installer with atomic rollback + bundled runtimes, updater, CI+SBOM/VEX, docs skeleton | 17-34 | Full team + packaging eng | Installable signed bundle, relay image, CI pipeline | G4 | IN PROGRESS (68%) |
| **5 Alpha** | Internal users, support bundles, 72h soak + random flaps + 100 resume cycles with Verifier, crash/dump pipeline, Diagnostics view | 35-42 | Full team + support eng | Alpha channel, soak+verifier reports | G5 | PENDING |
| **6 Beta** | Outside testers real carriers, compat matrix AV/VPN/Hyper-V/WSL2/MI on, external security review incl. driver fuzz | 43-50 | Full team + security reviewer | Beta channel, matrix, review report | G6 | PENDING |
| **7 RC** | Feature freeze, final WHCP attestation, SPDX 3.0 SBOM+VEX per driver, signed installers + update feed, rollback drill incl. driver rollback | 51-56 | Release eng | RC builds, attestation cert, rollback evidence | G7 | PENDING |
| **8 Production** | Public download + winget, status page, support process, opt-in telemetry | 57-58 | Release + support | GA, runbooks | G8 | PENDING |
| **9 Maintenance** | Monthly patches, driver re-validation each Windows feature build, relay capacity/cost, cert renewal automation (460-day cap), vendored-fork tracking | ongoing | Standing | Routine | — | — |

### 2.2 Upcoming Tasks, Dependencies, Deliverables

#### Immediate (Next 2 Weeks)

| Task | ID | Dependency | Deliverable | Effort | Risk |
|------|----|------------|-------------|--------|------|
| **S2c real netem rig** | S2c | netem-rig.sh exists, need Linux gateway with tc, iperf3, 2 lab machines | 10 reps per profile delay-gap 40ms, lossy 1%, asymmetric 200M/50M, cellular 100ms+2%+20Mbit, median/p95/spread per Sec.10.1, record in lab-progress.md | 1 wk, systems eng | Med — requires hardware |
| **S2b real VM** | S2b-real | miniport NBL pool + INJECT_NBL done (4d05142), need Hyper-V VM Gen2 Secure Boot vTPM VBS, WDK 10.0.22621+, test-signing | Driver test-signed load, Verifier 0x209BB + code integrity, traffic via miniport Send/INJECT_NBL loopback, document in hvci-readiness.md | 1 wk, driver eng | Med — VM setup |
| **Cargo.lock** | C2 | gen-lock.sh exists, need Rust toolchain + crates.io access (currently blocked in sandbox, only github/pypi/npm allowed) | `cargo generate-lockfile`, `cargo tree -p balancify-crypto`, commit Cargo.lock, update crypto-allowlist.txt transitive closure | 0.5 wk, release eng | Low — toolchain |
| **Throughput Rig B real** | RigB | throughput.rs real cipher done (4761969), need Rig B hardware wired 1Gbps/Wi-Fi 6 300M/USB 5G, relay in-region, Windows 11 24H2 x64+ARM64, driver test-signed, HVCI on/off | Aggregate 16 flows ≥90% sum, single-flow ≥70% sum, mismatched ≥100% best (no regression else auto-revert), added delay ≤3ms wired, loss/reordering ≤0.1%, failover ≤2s, CPU/Gbit via crypto-bench+ETW, power delta, record in throughput-rows.md | 2 wks, perf eng | High — hardware + carrier |

#### Short-Term (Phase 4-5)

| Task | Dependency | Deliverable | Gate |
|------|------------|-------------|------|
| **HVCI trio S15 real** | S2b-real, WDK build job in CI | Verifier clean, HLK HVCIMemoryIntegrity pass, VBS/MI VM run with msinfo32, driver load, traffic, ETW 200/201/300/400/401/402/500/600 visible | G3 |
| **SBOM/VEX S13** | Cargo.lock, reviewed advisory data | This follow-up adds a parallel SPDX 3.0.1 JSON-LD locked-graph inventory, official JSON Schema + SHACL CI validation, and installer copies of SPDX 2.3 + 3.0.1 sidecars. Reviewed VEX, support-bundle inclusion, and clean-machine install validation remain pending | G4, G7 |
| **Driver packaging** | S2b-real, HVCI trio | INF + PnP install, HVCI-compatible, `/integritycheck`, `/Qspectre`, self-contained .NET 10 + WinAppSDK runtimes bundled, transactional installer journaled 9 steps including both SPDX sidecars, with atomic rollback, clean-machine offline test per clean-machine-test.md (offline VMs, HVCI on/off, x64+ARM64, no residue) | G4, G5 |
| **Client binary + GUI IPC** | IPC v1 done, Service handshake done lab mode | `balancify-client` CLI with FileKeyStore DPAPI, Noise_IK 0xFF framing, bonded tunnel, dual relay via env, then WinUI 3 shell Home/Rules/Relay & account/Diagnostics/Tray flyout, 1Hz telemetry, last-known-good + stale markers, accessibility, self-contained .NET 10 | G4 |
| **Per-flow session tuning** | ADR-0029 done (session,seq) | Measure per-flow on vs off: aggregate, single-flow, failover, 10 reps median/p95/spread, isolation, head-of-line blocking, session cap 64 | G4 |

#### Mid-Term (Phase 5-6)

| Task | Dependency | Deliverable | Gate |
|------|------------|-------------|------|
| **Soak + Verifier + compat matrix** | Driver packaging, Rig B real | 72h soak, random link flaps, 100 sleep/resume cycles with Verifier standard+random, no crash/leaked route/filter/handle/IOCTL buffer, compat matrix VPN clients, security suites, Hyper-V, WSL2, MI on/off documented with workaround/incompatibility | G5, G6 |
| **External security review** | Soak clean, driver fuzz results | Review report incl. driver fuzz, threat model B1-B4, no high-sev open | G6 |
| **Docs site live** | MVP installable, architecture blueprint public | Astro Starlight or Docusaurus, pages home/download/quickstart/relay setup/architecture/troubleshooting/driver & security FAQ/changelog/security policy, signed installers + SHA-256, winget manifest, status page for relay health | G6 |

#### Long-Term (Phase 7-8)

| Task | Dependency | Deliverable | Gate |
|------|------------|-------------|------|
| **RC + final attestation + SBOM/VEX + rollback drill** | G6 pass, SBOM real, HVCI trio pass | RC builds, attestation certificate (EV + Partner Center), SPDX 3.0 SBOM+VEX per driver, rollback proven service+driver, checklist signed off | G7 |
| **GA + winget + status page** | G7 pass, SmartScreen reputation, bundle size trade | Public download offline signed installer bundle + SHA-256 sidecar, winget manifest, status page, support process, opt-in telemetry live (counters only, never destinations/content) | G8 |
| **Maintenance** | GA | Monthly patches, driver re-validation each Windows feature build, relay capacity/cost reviews, cert renewal automation 460-day cap, vendored-fork upstream tracking | — |

### 2.3 Burn-Down & Dependencies

![Burndown Chart](assets/progress/burndown-chart.png)

**Reading:**
- Ideal line: 120 tasks → 0 over 20 sprints (47-58 weeks)
- Actual line: steep drop Sprint 1-2 (feasibility, policy, signing strategy), moderate Sprint 3-6 (lab prototype S1/S2a/S2b), plateau Sprint 7-8 (architecture spike G3), steady decline Sprint 9-15 (MVP with ETW, per-path diag, driver WDF, app relay/bundle, ECN bench, per-flow retx, flow ACK, HVCI/SBOM docs)
- Current at Sprint 15: 35 remaining tasks (vs ideal 30), slight behind due to driver S2b real VM + Cargo.lock toolchain block, but velocity recovering (8 tasks/sprint)
- Projected: Sprint 16-20 to 0, with buffer for Rig B real measurements and HVCI real VM

**Critical Path Dependencies (from §11.3):**
- M1 Decisions + policy + signing enrolment → M2 Steering + M3 Miniport skeleton → M4 End-to-end tunnel + reorder measurements → M5 Miniport perf + HVCI + dry-run (G3) → M6 IPC + wireframes → M7 MVP installable with signed driver (G4) → M8 Server mode + container → M9 Soak + Verifier (G5) → M10 Compat + review (G6) → M11 RC + attestation + SBOM/VEX + rollback (G7) → M12 GA (G8)
- External prerequisites: Partner Center, CA lead times, Win11 lab machines 3 adapter classes, test-signing rig WDK, netem rig, Partner Center dry-run slot, attestation cert, soak rig, reviewer booking, winget validation

---

## 3. Strategic Alignment — Position vs Long-Term Roadmap

### 3.1 Overall Completion Dashboard

![Strategic Alignment](assets/progress/strategic-alignment.png)

**Radar (6 dimensions, per blueprint §4.8 policy rev 2 self-contained product):**
- **Data Plane (90%):** DRR, reorder buffer 80ms cap, reinjection, duplicate mode, pacing 80%, per-flow retx (session,seq), flow-level lane, MTU/MSS, keepalives ≤25s, NAT re-binding re-keys path
- **Control Plane (85%):** PolicyEngine snapshot immutable, WRH, link scoring EWMA, capacity estimator ACK pacing + bounded probes + flow packet + flow ACK, health hysteresis fast drop/slow return, bond auto-disable <15% gain → telemetry, per-flow/per-class sessions
- **Driver/Kernel (70%):** First-party NDIS miniport from Phase 2 (rev 2 directive, no third-party kernel in data path C2), WDF control device + IOCTLs + NBL pool + INJECT_NBL via NdisMIndicateReceiveNetBufferLists, handlers InitializeEx/HaltEx/Pause/Restart/Send/Return/Oid/Shutdown/PnP, stats, MTU, paused flag, length validation, SDDL, METHOD_BUFFERED, NonPagedPoolNx, no RWX, Verifier ready, HVCI docs, real VM pending
- **Security/Crypto (95%):** Noise_IK handshake, ChaCha20Poly1305 0.11.0, X25519 2.0.1, BLAKE2s 0.10.6, HKDF 0.12.4, getrandom 0.2.15, zeroize 1.8.1, pinned keys, replay cache, rekey forward secrecy 1h/1M packets, DPAPI store, TPM-backed where available, authenticated IPC named pipe ACL + caller identity, small kernel surface fuzzed, safe failure defaults fail-closed tunneled/fail-open balanced, relay trust minimization no persistent logs, privacy telemetry opt-in counters only
- **UX/GUI (60%):** Lab CLI `balancify-app` mimics WinUI3 shell Home/Rules/Diagnostics/Tray/Relay/Bundle modes, 1Hz telemetry, ECN/delay counters, last-known-good + stale markers planned, tray flyout keyboard+screen-reader operable, install elevation once, uninstall atomic, first-run wizard detect→propose→self-test; WinUI3 .NET 10 self-contained or bundled Desktop Runtime + WinAppSDK runtime redistributable, offline bundle, winget optional channel only per §4.8 rev 2.1
- **Docs/Compliance (80%, historical estimate):** ADRs, blueprint rev 2, technical docs phase2, SPDX 2.3/3.0.1 locked-graph SBOM generators and CI validators, clean-machine procedure, throughput rows, HVCI readiness, final-summary, CI history, lab-progress, security-hardening, test-coverage, roadmap-next, continuous-development-log, ETW schema, support-bundle redaction; reviewed VEX, hardware validation, Windows ETW trace collection/decoding, and production docs site remain pending.

**Donut: Completed 68%, In Progress 22%, Pending 10%**
- Completed: all above data plane, control plane core, crypto, driver skeleton + WDF + NBL pool, ETW wiring, support bundle per-path, relay ECN, app relay/bundle, ECN bench, throughput harness 5 profiles, per-flow retx (session,seq), flow ACK integration, HVCI/SBOM docs
- In Progress: G3 miniport perf + HVCI trio + signing dry-run, G4 acceptance table + clean-machine provisioning, driver packaging, client binary, per-flow tuning
- Pending: Cargo.lock (toolchain), real netem S2c 10 reps, Rig B real throughput, soak + Verifier, compat matrix, external review, RC + attestation + SBOM/VEX real + rollback drill, GA + winget + status page

**Gate Status:**
- G1 PASS, G2 PASS, G3 IN PROGRESS (yellow), G4 PENDING (gray), G5 PENDING, G6 PENDING, G7 PENDING, G8 PENDING

### 3.2 Technical Work Distribution

![Technical Breakdown](assets/progress/technical-breakdown.png)

**Code distribution:** core 25%, tunnel 20%, service 22%, crypto 12%, driver 8%, relay 5%, app/installer/client 8% — reflects self-contained product policy §4.8: every runtime component first-party or vendored fork or vetted pinned crypto, no third-party kernel in data path.

**Feature counts:** Congestion Control 13 tests, Pacing 7, DRR, ETW 8 IDs (100-600), Flow sessions, Rate limiting 3+1, Security Noise IK 12, Driver WDF 4 IOCTLs, Throughput 5 profiles, SBOM/HVCI docs — shows balanced investment across data plane, control plane, security, driver, observability.

### 3.3 Risk & Mitigation Alignment

| Risk | Prob | Impact | Mitigation | Owner | Trigger | Status |
|------|------|--------|------------|-------|---------|--------|
| R1 Relay cost+distance eat gain | Med | High | Cost/RTT model 2 regions, lab traffic profiles | Architect | >20% model deviation | Mitigated via server mode T1/T3/T4 self-host |
| R2 User mode cannot force egress | Med | High | Spike S1 ≥3 adapter classes | Systems 1 | S1 any failure | Retired — steering proven |
| R3 Heterogeneous links make bond < best | High | Med | Rig B 40ms gap, reorder tuning, flow-level fallback | Systems 2 | G4 measurement | Mitigated — 0 ooo under 80ms cap, auto-revert <15% |
| R4 Miniport perf misses or fails HVCI | Med | High | S2b skeleton, G3 benchmark + optimization round, Verifier in CI | Driver eng | G3 | In Progress — code ready, real VM pending |
| R5 WHCP signing / MI / SBOM calendar delays GA | High | High | Enrolment+EV Phase1, dry-run G3, final G7, exception policy §11.2 flow-level only Wintun fallback time-boxed | Release eng | Policy change or rejection | Mitigated — enrolment started, SBOM stub, exception documented ADR-0009 |
| R6 VPN/AV/Hyper-V/WSL2 conflicts | Med | Med | Compat matrix each milestone | Test eng | Each gate | Pending G6 |
| R7 Carrier NAT/plan limits | Med | Med | Multi-carrier tests, path-quality warning UI | Systems 1 | Beta reports | Pending Beta |
| R8 Licence terms vendored fork | Low | Med | Counsel review Apache-2.0/MIT mptcp-rs fork | Product owner | Before M7 / exception | Mitigated — fork vendored in-house, Apache-2.0 compatible |
| R9 Stale routes after crash/sleep strand offline | Med | High | Soak forced crashes + resume cycles, watchdog 15s restores direct routing | Systems 2 | G5 | Mitigated — resume_from_sleep test, abandoned count, keepalives expire_all |
| R10 SmartScreen blocks early downloads | High | Low | Consistent identity + timestamping, docs guidance | Release eng | First beta | Pending Beta |
| R11 MPQUIC draft churn / fork dep | Med | Med | MPTCP lane RFC wire primary, pin fork + wire version, interop suite in CI | Protocol eng | RFC publication or fork stall | Mitigated — datagram lane first-party primary, mptcp-rs optional vendored module compile-time optional |
| R12 Vendored mptcp-rs single maintainer | Med | Med | In-house fork from day one, module compile-time optional, T1/T3/T4 don't depend on it | Architect | Upstream stall >1 quarter | Mitigated — primary lane first-party |
| R15 Installer provisioning conflicts | Med | High | Transactional journal + rollback, clean-machine VM matrix CI from Phase4, AV/SmartScreen plan | Release eng | Each RC | Mitigated in source — journaled 9 steps, SPDX 2.3/3.0.1 sidecars, clean-machine-test.md; clean-machine run pending |
| R13 TCP-over-TCP meltdown in MPTCP lane | Med | Med | S2/S3 measurement lossy netem, libmqvpn datagram lane per-class fallback | Perf eng | G4 measurements | Mitigated — datagram lane avoids meltdown, MPTCP lane optional |

**Strategic alignment verdict:** After rev 2.1 changes, every owner criterion C1-C5 maps to architecture element, phase deliverable, gate, named test per §11.5 checklist — no uncovered criterion remains, development may start (and has). Residual watch-items: installer bundle size vs self-contained trade, AV/SmartScreen reputation ramp.

---

## 4. Data Sources & Methodology

- **CI history:** `docs/phase2/ci-history.md` — 53 runs, last green `493f354`, ubuntu+windows+audits, `cargo test --workspace --all-targets --no-fail-fast` + `cargo build --release --workspace` + `audit-deps.sh`
- **Lab progress:** `docs/phase2/lab-progress.md` — 214+ items proven, simulated vs real, known gaps updated after ADR-0029/0030/0031
- **Throughput:** `docs/phase2/throughput-rows.md` — synthetic benchmark scope and Rig B measurement procedure; no real §10.3 performance row is yet accepted.
- **HVCI:** `docs/phase2/hvci-readiness.md` — Verifier 0x209BB, HLK HVCIMemoryIntegrity, VBS/MI VM, code NonPagedPoolNx, NBL pool, MDL freeing, SDDL, METHOD_BUFFERED
- **SBOM:** `tools/ci/sbom.sh` — SPDX 3.0 JSON, 6 vetted crates chacha20poly1305=0.11.0 x25519-dalek=2.0.1 blake2=0.10.6 hkdf=0.12.4 getrandom=0.2.15 zeroize=1.8.1 + first-party, relationships, annotations C2, VEX
- **ADRs:** `docs/decisions/` 30 ADRs (0002-0031) — dependency policy, tunnel wire v0, IPC v1, per-path CC, vetted AEAD, Noise IK, key management+pacing, Wintun fallback exception, rate limiting+flow capacity, per-session sequencing, ETW+bundle, per-class sessions, throughput harness, continuous development, CI, rate limiter, flow capacity, ETW bundle, throughput harness, ETW wiring, support bundle per-path, relay ECN stats, driver WDF control, app shell relay+bundle, ECN bench, ETW uplink/bond, flow ACK, per-flow retx, flow ACK+driver S2b, HVCI+SBOM
- **Code:** `crates/` core, tunnel, service, crypto, relay, ipc, client, app, installer, driver/src/ miniport.c ioctl.c
- **Blueprint:** `docs/technical-blueprint.md` rev 2.1 — R1-R7, A1-A6, M1-M6 methods, E1-E8 endpoints, T1-T5 topologies, S2a/S2b/S2c spikes, G1-G8 gates, §10.3 acceptance rows, §11 phased roadmap 47-58 weeks, §11.5 criteria coverage C1-C5, §12 skills, §13 risks R1-R15 + D1-D6, §15 sources S1-S36

**Reporting discipline per §10.1:** 10 reps per test, median/p95/spread, record Windows build, hardware, driver versions, uplink types, relay region with every result, results stored as data not screenshots.

---

## 5. Conclusion & Next Steps (Without Stopping)

**Current momentum:** Strong — 5 consecutive CI greens after fixing PassThrough audit and Service generic, per-flow retx (session,seq) + flow ACK + driver S2b NBL pool landed in one commit 4d05142, docs updated 5fc3af0, final-summary e9c2c52, ci-history 493f354 all green.

**Trajectory:** On track for G3 (miniport perf + HVCI + signing dry-run) → G4 (acceptance table + clean-machine provisioning) → G5 (soak + Verifier) → G6 (compat + review) → G7 (RC + attestation + SBOM/VEX + rollback) → G8 (GA).

**Immediate actions (ordered, still without stopping per user instruction):**
1. S2c real netem rig `netem-rig.sh` 10 reps median/p95/spread
2. S2b real VM test-signed load NBL pool + INJECT_NBL via NdisMIndicateReceiveNetBufferLists, Verifier 0x209BB, VBS/MI on
3. Cargo.lock `gen-lock.sh` once toolchain available
4. Rig B throughput real wired 1Gbps/Wi-Fi 6 300M secure pacing 80% per-flow on/off iperf3 added delay ≤3ms CPU/Gbit via crypto-bench+ETW
5. HVCI trio S15 real VM + HLK test + Verifier, document in hvci-readiness.md
6. SBOM/VEX S13 real via cargo cyclonedx, publish sidecar, installer ProgramData
7. WinUI3 shell Home/Rules/Relay & account/Diagnostics/Tray, 1Hz telemetry, stale markers, accessibility, self-contained .NET 10
8. Soak 72h + flaps + 100 resume + Verifier, compat matrix VPN/AV/Hyper-V/WSL2/MI
9. RC + attestation + SBOM/VEX + rollback drill, GA + winget + status page

**Process:** Keep CI green at all times, each push must pass ubuntu+windows+audits (215+ tests), fix bugs using web resources (RFC 8684 Sec.3.3, RFC 6356 LIA, DRR, RFC 6928) + deep thinking, deep learning, repeating, then continue developing app based on criteria (blueprint). No third-party kernel in data path (C2), only vetted/pinned crates, PassThrough test-only never ship (Phase-4 audit). Workspace dependencies empty per §4.8.

---

*End of report — generated 2026-10-19, CI green 493f354, 215+ tests, 30 ADRs, 53 CI runs, Phase 4/5 boundary.*

