# Security hardening — what is done, what remains

Date: 2026-10-19
Status: continuous, 215+ tests green

## Done

- **Crypto allowlist + exact-pin audit** (C2/C4): `tools/ci/audit-deps.sh` checks allowlist (chacha20poly1305=0.11.0, x25519-dalek=2.0.1, blake2=0.10.6, hkdf=0.12.4, getrandom=0.2.15, zeroize=1.8.1) + exact-pin, fails on anything not on list, checks PassThrough literal, checks Cargo.lock if present
- **PassThrough cannot ship** (Phase-4 CI audit): `PassThrough` cipher is test-only, must never ship, audit fails if literal found in release build
- **No third-party kernel component in data path** (C2): only NDIS + WDF, no Wintun unless exception file exists (ADR-0009)
- **AEAD with AAD binding**: ChaCha20-Poly1305, header bound as AAD, nonce separated by path/seq/salt/key, 16-byte tag only overhead, every bit position refused, wrong-length key refused, 20-payload stream end-to-end (8 tests in `crates/crypto/src/lib.rs`)
- **Noise_IK handshake** (Sec.9.2): both sides derive same two keys, pinned relay identity authenticates, every byte of flight 1 authenticated, prologue binds context, initiator static travels encrypted, fresh ephemeral gives fresh session, flights out of order refused, truncations refused, negotiated keys drive tunnel lane, pinning enforced both sides, replay cache rejects second flight (12 tests)
- **Key management**: static keypair derives public, ephemeral zeroized on drop, pinned keys pin/unpin, OS RNG distinct, memory store generates/remembers, file store roundtrips, file store rejects corrupt (8 tests)
- **Replay protection** (Sec.11.4): new ephemeral not replay, same within window is replay, expired not replay, capacity evicts oldest, different ephemerals not replay (5 tests)
- **Rekey for forward secrecy** (Sec.11.3): rekey changes key but keeps salt, rekey deterministic and one-way, transport keys rekey both directions, auto-trigger by packet count (1M) or time (1h), old ciphers zeroized, session stays open (3 tests + service auto)
- **Zeroize**: ephemeral zeroized on drop, old ciphers zeroized on rekey, static secret zeroized? `x25519-dalek` with `zeroize` feature, `zeroize` crate pinned
- **Abuse controls + rate limiting (relay)**: global cap 1024 sessions, per-IP 8, handshake 10/min/IP, per-session token bucket 100 Mbit/s + 10k pps, stats, sweep old entries (2 tests)
- **Rate limiting service egress**: TokenBucket rate_bps + burst, try_consume, delay_for, RateLimiter per-uplink, default 1 Gbps/1MB, set_uplink_rate, try_send wired into send_app_packet -> Hold on limit (3 core tests + 1 service test)
- **Tampered input refused** (Sec.9.1b): under real AEAD, flipping any bit of frame's body or tag is refused and counted, nothing reaches inner stack (tunnel + service tests)
- **Fuzz gate over untrusted-input paths** (Sec.4.7.4): 6 seeded, dependency-free generators (20k rounds each for datagram and IPC-frame loops): no panic, no invented payload, session and hold caps bound memory, length checks hold, replay cannot manufacture delivery
- **Wintun fallback exception** (Sec.11.2): feature wintun-fallback, exception file check, raw FFI placeholder, flow-level only, must be retired next minor release, audit allows only when exception file exists (ADR-0009)
- **ETW logging + support bundle redacted** (Sec.8.2, Sec.9.2): EtwProvider stub, event IDs 100-600, UplinkDiag cwnd/in_flight, CongestionDiag loss/ecn/delay+paths+per_path, JSON redacted (no destinations, only counters, hashed flow keys)

## Remaining for Phase 4 / G3

- **Driver packaging**: bundle self-contained .NET 10 + WinAppSDK runtimes, transactional installer with atomic rollback, clean-machine offline test per `clean-machine-test.md`, SBOM sidecar
- **HVCI readiness trio**: Driver Verifier with code-integrity, HLK HVCI readiness test, run on VBS/Memory-Integrity machine (S15)
- **SBOM/VEX SPDX 3.0**: generate via `sbom.sh` + cargo cyclonedx, publish sidecar, S13 — stub exists, real generation needs cargo cyclonedx
- **Cargo.lock**: generate with `cargo generate-lockfile` once toolchain available, commit, audit checks lock file if present — currently no toolchain in sandbox (crates.io blocked, only github/pypi/npm allowed)
- **Real netem S2c**: replace simulated rig with `tools/lab/netem-rig.sh` profiles, record 10 reps median/p95/spread per Sec.10.1 — harness exists, real Rig B pending
- **Throughput rows on Rig B**: re-run Sec.10.3 aggregate/single-flow/failover with pacing enabled and secure mode, measure added delay and CPU/Gbit/s via crypto-bench + ETW — the local synthetic benchmark is not network evidence; real Rig B results pending
- **Client binary + GUI IPC**: wire Service handshake into CLI client (done lab mode), then WinUI 3 shell (Home, Rules, Relay & account, Diagnostics, tray) via IPC v1, self-contained .NET 10 — lab CLI `balancify-app` already mimics shell with home/rules/diagnostics/tray/relay/bundle modes
- **Per-flow session demux retx keyed by (session, seq)**: currently global unique seq for retx simplicity, future per-session seq spaces for true per-session sequencing
- **Flow-level ACK**: add explicit FlowData ACK for capacity (done via on_flow_packet counting flow packets into window_bytes, suppresses idle probe, but explicit ACK could still be added)
- **Driver WDF control device + IOCTLs**: `driver/src/ioctl.c` now has SET_MTU, GET_STATS, INJECT_NBL, SET_QUANTA, length-validated, WdfDriverCreate, BalancifyCreateControlDevice in MiniportInitializeEx — compiles on WDK, needs test-signed load with Driver Verifier and S2b socket test through miniport

## References

- Blueprint Sec.4.5 failover, Sec.4.7.4 per-flow sessions, Sec.4.8 dependency policy, Sec.6.1 TAP-like adapter, Sec.6.2 WRH + flow classes, Sec.6.3 capacity, Sec.6.4 DRR + pacing + CC, Sec.6.5 health hysteresis, Sec.6.6 NAT keepalive, Sec.8.2 Diagnostics, Sec.8.3 installer, Sec.9.2 IPC + handshake, Sec.9.3 signing, Sec.9.5 HVCI, Sec.10.1 Rig A/B, Sec.10.3 acceptance rows, Sec.11.2 Wintun exception, S13 SBOM/VEX, S15 HVCI trio, C2/C4
- ADRs 0003-0026
- CI: 39236895693 success, 215+ tests, benches (drr, pacing, ecn, crypto), docs
