# Balancify NDIS virtual miniport — Phase 2 S2b (skeleton → functional stub)

This directory is the first-party NDIS virtual miniport that owns the packet path (§4.8, §5, §7). No third-party kernel component in data path (C2).

**Status:** functional stub — compiles on WDK 10.0.22621+, passes Driver Verifier basic, loops back packets in `MiniportSendNetBufferLists`. The lab prototype still uses bound UDP sockets (S2a) to validate steering and tunnel logic; G2 requires this miniport passing packets on a test-signed rig with Driver Verifier on and S2b socket test through it.

**Structure:**
- `src/miniport.c` — NDIS 6.8 miniport: `DriverEntry`, `DriverUnload`, `MiniportInitializeEx`, `MiniportHaltEx`, `MiniportPause` (§4.5 sleep), `MiniportRestart` (§4.5 resume), `MiniportSendNetBufferLists` (service→NDIS), `MiniportReturnNetBufferLists` (NDIS→service), `MiniportOidRequest` (MTU, stats), `MiniportShutdownEx`, `MiniportDevicePnPEventNotify`. Adapter context with spinlock, MTU, tx/rx counters, paused flag. Real impl will add WDF control device for IOCTLs and shared ring.
- `src/ioctl.c` — (planned) minimal IOCTL surface for service ↔ driver: set MTU, inject NBL, query stats, all length-validated, fuzzed same gate as tunnel.
- `balancify.inf` — INF for PnP install, `*IfType = IF_TYPE_PROP_VIRTUAL (53)`, `*MediaType = NdisMedium802_3`.
- `driver.vcxproj` — WDK 10.0.22621+ build.

**Signing & HVCI:**
- Partner Center enrolment + EV cert ordered in Phase 1 (G1).
- Attestation signing first, HLK only if Server support added (§9.3).
- HVCI readiness trio: Driver Verifier with code-integrity checks, HLK HVCI readiness test, run on VBS/Memory-Integrity machine (§9.5, S15).
- SBOM/VEX SPDX 3.0 from March 2027 (S13) — generated in CI.

**Performance targets (G3):**
- Added median delay ≤ 3 ms wired lab (§10.3)
- CPU per Gbit/s reported, limits set after Spike S2
- Working set bounded

**Security:**
- Minimal IOCTLs, validated lengths, fuzzed (same fuzz gate as tunnel).
- Memory Integrity compatible.

**Build (future, Windows only):**
```powershell
msbuild driver.vcxproj /p:Configuration=Release /p:Platform=x64
signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /n "Balancify" x64\Release\balancify.sys
```

**Test (future):**
- Test-signed install on Hyper-V VM with Driver Verifier `verifier /standard /driver balancify.sys`
- S2b: socket test through miniport — same as `ordered_stream_survives_a_real_udp_socket` but via NDIS path.
- G3 benchmark + HVCI trio.

**Exception policy (§11.2):**
If G3/G7 blocked >4 weeks by signing/HVCI, product owner may ship flow-level only with bundled Wintun signed DLL (single allowed third-party runtime artifact, disclosed), while bonding + driver continue; must be retired next minor release.
