# CI history — arena/7abc96c8-balancify

Date: 2026-10-19
Status: continuous, last green 39237951269

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

## Current

- Last green 39237951269, ubuntu+windows+audits success, 215+ tests, benches compile, docs
- Branch `arena/7abc96c8-balancify` has ETW wiring, relay ECN stats, per-path diag, driver WDF control, app relay/bundle, ECN bench, security hardening, test coverage, next steps, final summary
- Next: driver S2b, Cargo.lock, netem S2c, Rig B throughput, HVCI, SBOM, GUI, per-flow retx keyed by (session, seq), flow-level ACK, more ETW events

## References

- Blueprint Sec.10.3 acceptance rows, Sec.10.1 10 reps, Sec.6.4 DRR+pacing+CC, S13 SBOM/VEX, C2/C4
- ADRs 0003-0026
