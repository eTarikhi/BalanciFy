# CI history — historical baseline and active Arena continuation

Date: 2026-10-21
Historical baseline: `arena/7abc96c8-balancify`, last green 1b7fa25 (50bf7c8 + ADR-0034), 215+ tests.
Active continuation: `arena/1e00c990-balancify`, commit `6b33e5e`; PR #13 is merged as `6856b498d00f6863007fd8a7def9805ed6a954bc`, and PR #14 is open. CI run `39669985734` passed all four checks after pinning Linux jobs to Ubuntu 24.04 and covering installer CLI parsing/journal path isolation. The merged installer transaction/journal hardening, fail-closed lab service transport, Home stale-state behavior, and loopback-only IPC client policy are now in `main`. Production named-pipe authentication, live ETW trace validation, persisted UI state, production UI, reviewed VEX, WDK/NDIS runtime, HVCI, clean-machine installation, and hardware performance remain unverified. See `continuous-development-log.md` for the verified continuation.

## Progression

- 39206081178 green (tunnel ack_with_ecn + relay ECN echo)
- 39206268659 failure bench trait (Crypto::open signature)
- 39206283573 failure bench trait+docs
- 39206485667 failure open 5 args
- 39206671029 success bench fix (44d17f7)
- 39206863607 success ADR-0011/0012
- 39207060537 failure rate limiter burst
- 39207242665 success burst fix
- 39207437764 success DRR bench
- 39207636405 success ADR-0014
- 39207832446 failure pacing-bench delay_for not found
- 39208040465 success bench imports + pacing method
- 39208241321 success ADR-0015
- 39208453826 success roadmap-next
- 39208659414 success ADR-0016
- 39208870890 success bug-hunt
- 39209091609 success lab-progress
- 39209291038 success ADR-0017
- 39209496047 success ADR-0018
- 39209696771 success ADR-0019
- 39209903149 success ADR-0020
- 39232571739 failure ETW wiring non-ASCII
- 39233028939 failure prefix binding/mapping/dropped
- 39233548995 failure arg order on_ack_with_rtt
- 39233826038 success arg order fix — ETW wiring + relay ECN stats
- 39234128363 success per-path diag — support bundle per-path ECN/delay
- 39234403709 success ADR-0021
- 39234666177 success ADR-0022
- 39234937199 success ADR-0023
- 39235214291 success continuous-development-log
- 39235545738 success driver WDF control
- 39235817239 success ADR-0024
- 39236091314 success app relay & bundle
- 39236360963 success ADR-0025
- 39236639218 success ECN bench
- 39236895693 success ADR-0026
- 39237159469 success security-hardening
- 39237431952 success test-coverage
- 39237683235 success next-steps
- 39237951269 success final-summary — 215+ tests, benches
- 39238213860 success ci-history
- 39266364959 success ETW uplink/bond (655d91d)
- 39266694119 success ADR-0027
- 39267073764 success non-ASCII+flow ack (0d8f2b1)
- 39267418909 success ADR-0028
- 39267743843 failure throughput PassThrough
- 39268156078 failure Service generic
- 4761969 success throughput real cipher (fix Service<TunnelCipher>)
- 4d05142 success per-flow retx + flow ACK + driver S2b
- f89a617 success ADR-0029/0030
- 5fc3af0 success docs update lab-progress + throughput Rig B plan + HVCI + SBOM
- e9c2c52 success final-summary updated
- 493f354 success ci-history updated
- 4feba2b success development progress report with 5 visualizations (gantt option1, burndown option2, velocity option2, strategic-alignment option1, technical-breakdown option1)
- 5491758 success Cargo.lock vetted-only version 3 + allowlist transitive closure
- 9115e4f success driver vcxproj HVCI flags /integritycheck /DYNAMICBASE /NXCOMPAT /Qspectre PoolTag BlaB
- 6c9a52f success sbom.sh v2 SPDX 3.0 + VEX 10 first-party + vetted + transitive
- 0581256 success throughput harness enhanced 9 profiles Rig B wired 1Gbps/Wi-Fi 6 300M per-flow on/off aggregate 16 flows
- cd1673f success app shell enhanced 9 modes per-path ECN/delay bond gain uplink up/down ETW per-flow sessions Rig B HVCI SBOM
- 78cd32b success docs lab-progress + hvci-readiness updated
- 36d65eb success throughput-rows updated with 9 profiles
- 816b5e2 success final-summary updated
- 8ab9501 success installer SBOM sidecar 8 steps
- a527d6d success driver INF enhanced HVCI-ready Universal
- 6384de4 success ADR-0032 driver packaging + SBOM sidecar
- b666a03 success ci-history updated with Cargo.lock, vcxproj, SBOM, throughput 9 profiles, app 9 modes, report 5 viz, INF, installer, ADR-0032
- 2f2766c success crypto bench enhanced CPU/Gbit
- 7fb32ec success driver README WDK build instructions x64/ARM64
- 5c05cb5 success netem-rig.sh enhanced 11 profiles + analyze.py
- 04a533b success ADR-0033 netem-rig enhanced 11 profiles + analyze.py
- ae34432 failure soak harness remove_uplink with 2 args + etw.event not found + sleep/resume not found — E0599
- 95dbb0e failure failover tracking etw.event not found — E0599
- 0722246 failure throughput failover etw.event + remove_uplink_at timing
- 2c81f63 failure app diagnostics still etw.event? Actually 0722246 chain
- 34386af failure fix remove_uplink_at but still etw.event + sleep
- 094bfa7 failure fix etw.event->info/warn but still sleep/resume
- 89e1f26 success fix sleep->resume_from_sleep — soak harness G5 72h + random flaps + 100 resume — CI green
- 50bf7c8 success app failover and soak modes 11 modes — G5
- 1b7fa25 success ADR-0034 WinUI3 shell 11 modes + soak harness G5 + failover convergence <=2s + netem-rig 11 profiles + analyze.py

## Historical snapshot (pre-Arena continuation)

- Last green 1b7fa25 (50bf7c8 + ADR-0034), ubuntu+windows+audit success, 215+ tests, benches compile, docs, Cargo.lock vetted-only, vcxproj HVCI, INF Universal, SBOM/VEX SPDX3 v2, throughput 10 profiles Rig B+failover, app 11 modes failover/soak, netem-rig 11 profiles + analyze.py, soak harness G5, failover convergence tracking <=2s, installer 8 steps SBOM sidecar, report 5 viz
- Branch `arena/7abc96c8-balancify` has:
  - ETW wiring (ECN CE 401, delay 402, auth_failed 600, rekey 400, failover 500, uplink up/down 200/201, bond reverted 300) with info/warn methods
  - Relay ECN stats (ecn_echoed, total_ecn_echoed, sweep, stats 6-tuple) + rate limiting global 1024 per-IP 8 handshake 10/min/IP token bucket 100 Mbit/s 10k pps
  - Support bundle per-path ECN/delay diag (cc methods ecn_ce, delay_events_path, rtt_min, rtt_last, per_path Vec<PathCongestionDiag>) + failover counts+convergence ms
  - Driver WDF control + NBL pool + packaging vcxproj HVCI flags INF Universal README WDK build instructions
  - App shell 11 modes (home/rules/diagnostics/tray/relay/bundle/perflow/rigb/hvci/sbom/failover/soak)
  - Throughput harness 10 profiles (bonded/single/lossy/ECN/delay/RigB/perflow_off/perflow_on/aggregate_16flows/failover) + soak harness G5
  - Per-flow retx (session,seq) + flow ACK + failover convergence <=2s
  - Netem rig 11 profiles + analyze.py mean/median/p95/spread
  - Cargo.lock vetted-only, SBOM/VEX SPDX3 v2, installer 8 steps SBOM sidecar, report 5 viz
- Next: real netem S2c 10 reps median/p95/spread on Linux gateway, real Rig B throughput iperf3 wired 1Gbps/Wi-Fi 6 300M secure pacing 80% per-flow on/off failover <=2s added delay <=3ms CPU/Gbit ETW WPA, HVCI real VM Verifier 0x209BB + HLK + VBS/MI, SBOM real cyclonedx, WinUI3 shell real .NET 10 + WinAppSDK, soak 72h + Verifier + compat matrix, RC+attestation+SBOM/VEX+rollback drill G7, GA+winget+status page G8

## References

- Blueprint Sec.10.3 acceptance rows, Sec.10.1 10 reps, Sec.6.4 DRR+pacing+CC, S13 SBOM/VEX, S15 HVCI trio, C2/C4, Sec.4.5 failover, Sec.8.2 Diagnostics, G5 soak+Verifier+compat
- ADRs 0002-0034
- Historical CI: 1b7fa25 success, 89e1f26 success soak fix, 50bf7c8 success app 11 modes; current active CI is recorded above
