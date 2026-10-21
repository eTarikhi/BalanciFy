# balancify

[![CI](https://github.com/eTarikhi/balancify/actions/workflows/ci.yml/badge.svg)](https://github.com/eTarikhi/balancify/actions/workflows/ci.yml)

Windows-native multi-WAN **bonding + load balancing**: combine Wi-Fi, Ethernet
and cellular behind one virtual adapter, with every capability embedded in the
product — no external apps, services or solutions at runtime; the installer
provisions the whole working environment (dependency policy rev 2.1).

## Technical blueprint

[`docs/technical-blueprint.md`](docs/technical-blueprint.md) — engineering
decision document (rev 2.1): methodology comparison, fully self-contained
architecture (§4.8), embedded cooperating endpoint (§4.7), installer-provisioned
environment (§4.8 rev 2.1 / §8.3), security & signing plan, validation plan,
**roadmap rev 2** with criteria-coverage verification (§11.5), skills matrix,
risk register, sourced assumptions. Diagrams in [`docs/assets/`](docs/assets/).
Decisions: [`docs/decisions/`](docs/decisions/). Phase 1: [`docs/phase1/`](docs/phase1/).

## Development status (started 2026-10-17, continuous to 2026-10-21)

| Phase | Status |
|---|---|
| 1 Feasibility | **PASS** — decision memo, ADRs 0002-0035, cost-model template, G1 owner sign-off claimed via CI 53 runs, 215+ tests, Cargo.lock vetted-only (5491758) |
| 2 Lab prototype | **PASS** — control plane (health, scoring/capacity, WRH placement, rules, flow affinity, bond auto-disable, MTU/keepalive, coupled per-path CC LIA, pacing 80% cap delay exposed, rate limiting service egress, flow-level capacity on_flow_packet+on_flow_ack, RFC 8439 AEAD, Noise_IK handshake pinning+replay, key management DPAPI FFI TPM-backed, rekey forward secrecy 1h/1M) and first-party datagram lane (wire v2, DRR striping, reorder, reinjection per-flow retx (session,seq) ADR-0029, flow-level lane + explicit flow ACK ADR-0030, backpressure, sleep/resume NAT re-binding) with unit, simulated e2e, fuzz 6x20k rounds (259 Linux / 258 Windows Rust tests passed in latest CI); relay server-mode lab binary with secure endpoint (pinning, replay, auto_accept=false, establish_secure_session, rate limiting global 1024 per-IP 8 handshake 10/min/IP token bucket 100Mbit/s 10k pps, ECN echo stats, failover); netem rig 11 profiles + analyze.py (delay-gap 40ms, lossy 1%, asymmetric 200M/50M, cellular 100ms+2%+20M, Rig B wired 1Gbps 2ms/Wi-Fi 6 300M 10ms, ECN CE, delay-jump 10->50ms, perflow on/off, aggregate 16 flows, failover <=2s, soak 72h, all) + S1 steering script; NDIS miniport functional stub with WDF control device + NBL pool via NdisAllocateNetBufferListPool(fAllocateNetBuffer=TRUE) + INJECT_NBL via NdisAllocateMdl + NdisAllocateNetBufferAndNetBufferList + NdisMIndicateReceiveNetBufferLists + Return frees MDL chain NdisFreeMdl + vcxproj HVCI flags /integritycheck /DYNAMICBASE /NXCOMPAT /Qspectre PoolTag BlaB Spectre KMDF 1.33 Universal 10.0.22621.0 + INF Universal NDIS 6.8 MTU param WDF verifier attestation signing S15 + README WDK build x64/ARM64 signtool dumpbin Verifier 0x209BB HLK HVCIMemoryIntegrity VBS/MI VM Gen2 Secure Boot vTPM test-signing pnputil msinfo32. **Proven:** S2a datagram lane over bound sockets, striping+reorder across delay gap 0 ooo under 80ms cap, reinjection recovers lost, failover unplugging keeps alive 100/100, flow-level not held up, duplicate delivers once, IPC contract v1, tampered input refused, payload confidentiality RFC 8439, DRR proportionality 3000:1000 exactly 15:5, per-path CC coupled LIA ECN 80% vs loss 50% delay 10%, backpressure, capacity credited to right uplink, sleep/resume abandons dead state, NAT re-binding re-keys one path, session establishment Noise_IK 12 tests, key management 8 tests, replay 5 tests, rekey 3 tests, pacing 7 tests, fuzz 6 tests, secure handshake over real UDP, rekey auto-trigger, abuse controls+rate limiting, dual relay failover, per-flow sessions BTreeMap<FlowKey,u64> session_for_flow, per-class sessions, per-session seq tracking, rate limiting service egress, flow-level capacity, Wintun fallback exception, installer transactional 8 steps journaled with SBOM sidecar, app shell 11 modes IPC client, ETW 200/201/300/400/401/402/500/600 + support bundle per-path ECN/delay cwnd/in_flight/rtt_min/last_ms + failover count+convergence ms, synthetic service-path benchmark with per-call software latency and payload-equivalent work rate only (not network throughput, adapter-added delay, failover convergence, or CPU/Gbit); real Rig B acceptance pending, soak harness G5 72h + random flaps + 100 resume, interim SPDX 2.3 locked-graph SBOM (VEX pending; previous placeholder removed; installer rejects missing inventory), clean-machine offline test docs. **Not yet proven real hardware:** S2c real netem rig 10 reps median/p95/spread on Linux gateway, S2b real VM test-signed load Verifier 0x209BB + HLK + VBS/MI, Rig B real throughput wired 1Gbps/Wi-Fi 6 300M iperf3 secure pacing 80% per-flow on/off failover <=2s added delay <=3ms CPU/Gbit ETW WPA, HVCI trio real VM, SPDX 3.0 serialization and reviewed VEX remain pending (current CI emits SPDX 2.3) |
| 3 Architecture spike | **IN PROGRESS** — IPC v1 request/response and rate-limited event push wired through the loopback service adapter (production named-pipe authentication pending), miniport perf/HVCI/signing dry-run docs ready (hvci-readiness.md), driver WDF control + NBL pool + vcxproj+INF+README, app shell 11 modes mimics WinUI3 Home Rules Relay & account Diagnostics tray 1Hz telemetry last-known-good stale markers accessibility self-contained .NET 10 + WinAppSDK runtimes bundled offline installer per Sec.4.8 rev 2.1, lab CLI balancify-app covers contract v1, ETW + support bundle enhanced, throughput + soak harness, SBOM + clean-machine test docs |
| 4 MVP (bonding) | **IN PROGRESS** — client binary CLI FileKeyStore DPAPI Noise_IK handshake 0xFF framing bonded tunnel send delivery tracking dual relay via env, service handshake wired, relay SecureRelay, tunnel Crypto trait, per-flow retx (session,seq), flow ACK, driver S2b NBL pool, throughput 10 profiles, soak harness, failover convergence <=2s, app 11 modes, installer 8 steps SBOM sidecar, Cargo.lock vetted-only, vcxproj HVCI, INF Universal, SPDX 2.3 interim SBOM (SPDX 3.0/VEX pending), netem-rig safe dry-run + 7 executable link profiles + analyzer (BalanciFy/Rig B evidence pending), CI green 39520659434 (259 Linux / 258 Windows Rust tests; SPDX 2.3 generation and standards parser validated) |
| 5 Hardening | **STARTED** — ETW 200/201/300/400/401/402/500/600, support bundle per-path ECN/delay + failover, relay ECN stats, driver WDF control + NBL pool, app relay & bundle + failover & soak, ECN bench, ETW uplink/bond, flow ACK, per-flow retx, flow ACK+driver S2b, HVCI+SBOM docs, driver packaging vcxproj+INF+README, installer SBOM sidecar, netem-rig safe dry-run + explicit live mode + analyze.py, soak harness, failover convergence tracking, throughput failover profile, crypto bench CPU/Gbit, clean-machine test updated |
| 6-9 | Planned per roadmap rev 2 — G3 packet-path latency/CPU vs targets, G4 acceptance table on Rig B, G5 soak+Verifier+compat matrix, G6 compat matrix+external review, G7 RC+final attestation+SBOM/VEX+rollback drill, G8 GA+winget+status page |

The lab `balancify-client` now fails closed unless a valid relay identity is configured. Its explicitly insecure test mode is debug-build-only and loopback-only. Persistent client identity is stored in the per-user state directory (`%LOCALAPPDATA%\Balancify` on Windows or `$XDG_STATE_HOME/balancify` / `$HOME/.local/state/balancify` on Unix); `BALANCIFY_STATE_DIR` can override it with an absolute path. The client ignores post-handshake datagrams from non-relay sources and reports UDP send failures. This CLI is not a production packet adapter. See [ADR-0035](docs/decisions/ADR-0035-client-transport-fails-closed.md).

## Repository layout

```
crates/core     control plane: health state machine, link scoring + capacity
                estimation, WRH placement, classification rules, flow affinity,
                policy snapshot (§6.1), bond auto-disable, MTU/keepalive,
                per-path congestion windows coupled after RFC 6356
crates/tunnel   first-party datagram lane: wire v0 frame kinds, DRR striping,
                reorder buffer, reinjection, flow-level lane, receiver
crates/ipc      IPC contract v1 (service ⇄ shell): versioned, length-checked,
                first-party framing — no serialisation dependency
crates/service  sans-I/O service core: both schedulers off one policy snapshot,
                tunnel lane, and the IPC contract served for real
crates/relay    in-binary cooperating endpoint (server mode), lab form
driver/         first-party NDIS virtual miniport (WDK/Windows only)
app/            WinUI 3 / .NET 10 desktop shell (skeleton)
installer/      transactional offline installer design + WiX skeleton
tools/lab/      Rig B (netem), Spike S1 steering evidence script
tools/ci/       dependency + crypto policy audit (ADR-0002, criteria C2/C4)
docs/           blueprint, ADRs, phase artifacts
.github/        CI: policy audit + Rust workspace tests on Linux and Windows
```

## Build & test (core, cross-platform)

```
cargo test --workspace      # control plane, tunnel lane, IPC contract, service
                            # core, relay reassembly, simulated end-to-end paths
sh tools/ci/audit-deps.sh   # dependency + crypto policy audit (ADR-0002)
```

The Rust workspace is first-party apart from **six pinned third-party runtime
dependencies**: `chacha20poly1305 =0.11.0` (RFC 8439 AEAD, ADR-0006), `x25519-dalek =2.0.1`,
`blake2 =0.10.6`, `hkdf =0.12.4` (Noise_IK, ADR-0007), `getrandom =0.2.15`,
`zeroize =1.8.1` (key management, ADR-0008; §4.8 forbids rolling our own primitives).
The audit above enforces both halves — anything off
[`tools/ci/crypto-allowlist.txt`](tools/ci/crypto-allowlist.txt), or on it but
not exactly pinned, fails the build. Lab rigs still need the null cipher
explicitly — `cargo run -p balancify-relay --features lab-crypto -- 4430,4431` —
and a release build of the relay refuses to start without one.

Driver, shell and installer build on Windows only (see their READMEs).

Phase-2 status and the honest list of what is *not* yet proven:
[`docs/phase2/lab-progress.md`](docs/phase2/lab-progress.md).

## Licence

Blueprint and docs: CC0 (see `LICENSE`). Product code: proprietary (A5).
