# CI history — arena/7abc96c8-balancify

Date: 2026-10-19
Status: continuous, last green e9c2c52 (5fc3af0 + docs), 215+ tests

## Progression

- 39206081178 green (tunnel ack_with_ecn + relay ECN echo)
- 39206268659 failure bench trait (Crypto::open signature)
- 39206283573 failure bench trait+docs
- 39206485667 failure open 5 args
- 39206671029 success bench fix (44d17f7 fix(crypto): bench open signature)
- 39206863607 success ADR-0011/0012 (f27c945, 5622ff1)
- 39207060537 failure rate limiter burst (4af4638 test rate limiter holds)
- 39207242665 success burst fix (e56254d burst 1000->2000)
- 39207437764 success DRR bench (437d153)
- 39207636405 success ADR-0014 (3382d32)
- 39207832446 failure pacing-bench delay_for not found (06481d5)
- 39208040465 success bench imports + pacing method (d1f9365)
- 39208241321 success ADR-0015 (18c028a)
- 39208453826 success roadmap-next (9567422)
- 39208659414 success ADR-0016 (56fb239)
- 39208870890 success bug-hunt (a2ea61c)
- 39209091609 success lab-progress (796fb8f)
- 39209291038 success ADR-0017 (45b97d7)
- 39209496047 success ADR-0018 (5fd5abe)
- 39209696771 success ADR-0019 (f1e07f7)
- 39209903149 success ADR-0020 (0f5baaf)
- 39232571739 failure ETW wiring non-ASCII (44942dc) — Sec. markers §, —, etc.
- 39233028939 failure prefix binding/mapping/dropped (8c2bc1e) — replaced Sec. but still prefix errors + arg order
- 39233548995 failure arg order on_ack_with_rtt (4fdded0) — format! escaping fixed but arg order still wrong
- 39233826038 success arg order fix (eb3b55a) — ubuntu+windows+audits green, ETW wiring + relay ECN stats
- 39234128363 success per-path diag (acdb6a3) — support bundle per-path ECN/delay
- 39234403709 success ADR-0021 (88a4db9)
- 39234666177 success ADR-0022 (054481e)
- 39234937199 success ADR-0023 (9084d95)
- 39235214291 success continuous-development-log (a9c1f60)
- 39235545738 success driver WDF control (be21940)
- 39235817239 success ADR-0024 (61f6c74)
- 39236091314 success app relay & bundle (166bfcf)
- 39236360963 success ADR-0025 (74a1b71)
- 39236639218 success ECN bench (f09bf71)
- 39236895693 success ADR-0026 (7b96fae)
- 39237159469 success security-hardening (7f8172c)
- 39237431952 success test-coverage (4e919bf)
- 39237683235 success next-steps (cc5960e)
- 39237951269 success final-summary (7ecb339) — 215+ tests, benches (drr, pacing, ecn, crypto), docs
- 39238213860 success ci-history (fa1d7da)
- 39266364959 success ETW uplink/bond (655d91d) — publish() prev_uplinks clone, EVT_UPLINK_UP 200 / DOWN 201, bond reverted 300
- 39266694119 success ADR-0027 (c6b5f74)
- 39267073764 success non-ASCII+flow ack (0d8f2b1) — all crates non-ASCII cleaned, scoring on_flow_ack
- 39267418909 success ADR-0028 (5f767b1)
- 39267743843 failure throughput PassThrough (45d0d5b) — policy audit: PassThrough string in file, plus PassThrough usage in bin
- 39268156078 failure Service generic (85f3ed6) — audit passed but ubuntu/windows E0107 missing generics Service
- 39268156078? Actually 4761969 success throughput real cipher (fix Service<TunnelCipher>, remove PassThrough string)
- 4d05142 success per-flow retx + flow ACK + driver S2b (per-flow retx keyed by (session,seq), flow-level ACK integration in ingest_path, driver S2b INJECT_NBL via NdisMIndicateReceiveNetBufferLists with NBL pool)
- f89a617 success ADR-0029/0030 (per-flow retx, flow ACK and driver S2b docs)
- 5fc3af0 success docs update (lab-progress with per-flow retx, flow ACK, driver S2b NBL pool, throughput Rig B plan, HVCI readiness trio, SBOM/VEX SPDX3)
- e9c2c52 success final-summary updated

## Current

- Last green e9c2c52 (5fc3af0 + docs), ubuntu+windows+audits success, 215+ tests, benches compile, docs
- Branch `arena/7abc96c8-balancify` has:
  - ETW wiring (ECN CE 401, delay 402, auth_failed 600, rekey 400, failover 500, uplink up/down 200/201, bond reverted 300)
  - Relay ECN stats (ecn_echoed, total_ecn_echoed, sweep, stats 6-tuple)
  - Support bundle per-path ECN/delay diag (cc methods ecn_ce, delay_events_path, rtt_min, rtt_last, per_path Vec<PathCongestionDiag>)
  - Driver WDF control (SET_MTU 0x800, GET_STATS 0x801, INJECT_NBL 0x802 via NdisMIndicateReceiveNetBufferLists with NBL pool, SET_QUANTA 0x803, length-validated, WdfDriverCreate, BalancifyCreateControlDevice, NBL pool, MDL freeing)
  - App shell relay & bundle modes (home/rules/diagnostics/tray/relay/bundle, ECN/delay counters)
  - ECN bench (cwnd 80% ECN vs 50% loss, delay 10% reduction, per-path)
  - Throughput harness (real cipher, 5 profiles bonded/single/lossy/ECN/delay, 10 reps mean/median/p95/spread, gain, penalty)
  - Per-flow retx keyed by (session,seq) + per-session seq spaces (Sent includes session, retx BTreeMap<(u64,u64), RetxEntry>, session_seqs authoritative, on_ack_for_session, duplicate_with_session, reinjections_due preserves session)
  - Explicit flow-level ACK integration (PolicyEngine on_flow_ack wrapper, Service ingest_path detects FlowData and calls on_flow_ack + on_ack)
  - HVCI readiness trio doc (Verifier 0x209BB + code integrity, HLK HVCIMemoryIntegrity, VBS/Memory-Integrity VM)
  - SBOM/VEX SPDX 3.0 (sbom.sh SPDX 3.0 JSON with 6 vetted crates + first-party, relationships, annotations C2, VEX section, installer sidecar)
  - Throughput Rig B plan (wired 1Gbps/Wi-Fi 6 300M, secure Noise_IK, pacing 80%, per-flow on/off, iperf3, added delay <=3ms, CPU/Gbit via crypto-bench+ETW, 10 reps median/p95/spread)
- Next: real netem S2c (netem-rig.sh 10 reps), real Rig B throughput, Cargo.lock (gen-lock.sh), HVCI real VM, SBOM real cyclonedx, WinUI3 shell, soak+Verifier+compat matrix, RC+attestation+SBOM/VEX+rollback, GA

## References

- Blueprint Sec.10.3 acceptance rows, Sec.10.1 10 reps, Sec.6.4 DRR+pacing+CC, S13 SBOM/VEX, S15 HVCI trio, C2/C4
- ADRs 0002-0031
- CI: e9c2c52 success (latest)
