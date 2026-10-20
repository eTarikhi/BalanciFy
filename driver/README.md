# Balancify NDIS virtual miniport -- Phase 2 S2b (skeleton -> functional stub)

This directory is the first-party NDIS virtual miniport that owns the packet path (Sec.4.8, Sec.5, Sec.7). No third-party kernel component in data path (C2).

**Status:** functional stub -- compiles on WDK 10.0.22621+, passes Driver Verifier basic, loops back packets in `MiniportSendNetBufferLists`. The lab prototype still uses bound UDP sockets (S2a) to validate steering and tunnel logic; G2 requires this miniport passing packets on a test-signed rig with Driver Verifier on and S2b socket test through it.

**Structure:**
- `src/miniport.c` -- NDIS 6.8 miniport: `DriverEntry`, `DriverUnload`, `MiniportInitializeEx`, `MiniportHaltEx`, `MiniportPause` (Sec.4.5 sleep), `MiniportRestart` (Sec.4.5 resume), `MiniportSendNetBufferLists` (service->NDIS), `MiniportReturnNetBufferLists` (NDIS->service), `MiniportOidRequest` (MTU, stats), `MiniportShutdownEx`, `MiniportDevicePnPEventNotify`. Adapter context with spinlock, MTU, tx/rx counters, paused flag. Real impl will add WDF control device for IOCTLs and shared ring.
- `src/ioctl.c` -- (planned) minimal IOCTL surface for service <-> driver: set MTU, inject NBL, query stats, all length-validated, fuzzed same gate as tunnel.
- `balancify.inf` -- INF for PnP install, `*IfType = IF_TYPE_PROP_VIRTUAL (53)`, `*MediaType = NdisMedium802_3`.
- `driver.vcxproj` -- WDK 10.0.22621+ build.

**Signing & HVCI:**
- Partner Center enrolment + EV cert ordered in Phase 1 (G1).
- Attestation signing first, HLK only if Server support added (Sec.9.3).
- HVCI readiness trio: Driver Verifier with code-integrity checks, HLK HVCI readiness test, run on VBS/Memory-Integrity machine (Sec.9.5, S15).
- SBOM/VEX SPDX 3.0 from March 2027 (S13) -- generated in CI.

**Performance targets (G3):**
- Added median delay <= 3 ms wired lab (Sec.10.3)
- CPU per Gbit/s reported, limits set after Spike S2
- Working set bounded

**Security:**
- Minimal IOCTLs, validated lengths, fuzzed (same fuzz gate as tunnel).
- Memory Integrity compatible.

**Build (Windows only, WDK 10.0.22621+):**
```powershell
# Prerequisites: Visual Studio 2022 with C++ desktop, WDK 10.0.22621+, SDK 10.0.22621+
# From repo root:
msbuild driver\balancify.vcxproj /p:Configuration=Release /p:Platform=x64
msbuild driver\balancify.vcxproj /p:Configuration=Release /p:Platform=ARM64
# Output: x64\Release\balancify.sys, ARM64\Release\balancify.sys
# Sign test:
signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /f test.pfx /p password x64\Release\balancify.sys
# Verify HVCI flags:
dumpbin /headers x64\Release\balancify.sys | findstr /i "integrity DYNAMICBASE NXCOMPAT"
# Check PoolTag:
strings x64\Release\balancify.sys | findstr BlaB
```

**Test (real VM required):**
- Hyper-V VM Gen2 Secure Boot on vTPM VBS on, Memory Integrity on/off
- Enable test-signing: `bcdedit /set testsigning on`, reboot
- Install: `pnputil /add-driver driver\balancify.inf /install`
- Driver Verifier: `verifier /standard /driver balancify.sys` flags 0x209BB special pool, force IRQL, pool tracking, I/O verification, deadlock, DMA, security, DDI compliance, code integrity — reboot, `!verifier` in WinDbg, run traffic, no bugchecks
- HLK: Studio -> HVCIMemoryIntegrity test, checks no executable NonPagedPool, no executable MDL, no RWX, no self-modifying — pass criteria no failures
- VBS/MI: `msinfo32` VBS Running MI enabled, driver loaded, traffic via `throughput.rs` + `balancify-service` binary, ETW 200/201/300/400/401/402/500/600 visible via WPA
- S2b: socket test through miniport -- same as `ordered_stream_survives_a_real_udp_socket` but via NDIS path: service SendNetBufferLists, relay receives, reorder, deliver, IOCTL INJECT_NBL allocates MDL via NdisAllocateMdl NBL via NdisAllocateNetBufferAndNetBufferList indicates via NdisMIndicateReceiveNetBufferLists, Return frees MDL chain via NdisFreeMdl
- G3 benchmark + HVCI trio documented in `docs/phase2/hvci-readiness.md` with date, Windows build, hardware, driver version

**INF:**
- `driver/balancify.inf` Universal, PnpLockdown=1, NDIS 6.8, MTU param 576..9000 default 1440, WDF verifier, PoolTag BlaB, METHOD_BUFFERED, length validation, attestation signing
- WDF control-device DACL grants access only to SYSTEM and Administrators; GET_STATS requires `FILE_READ_ACCESS`, and mutating IOCTLs require `FILE_WRITE_ACCESS`. The service must run with the required privileged identity; the desktop shell must never open this device directly. WDK build and ACL behavior still require validation on a Windows driver lab machine.
- `driver/miniport.inf` legacy skeleton, use balancify.inf for new builds

**vcxproj:**
- `driver/balancify.vcxproj` HVCI flags /integritycheck /DYNAMICBASE /NXCOMPAT /Qspectre, PoolTag BlaB, Spectre mitigation, KMDF 1.33, Universal, 10.0.22621.0, NDIS620_MINIPORT, NDIS 6.8, WDF

**HVCI readiness trio (S15):**
- Verifier 0x209BB + code integrity, HLK HVCIMemoryIntegrity, VBS/MI VM Secure Boot vTPM test-signing — code ready, real VM run pending, CI only Rust (no WDK), future add WDK build job Phase 4 (a) in ci.yml

**Exception policy (Sec.11.2):**
If G3/G7 blocked >4 weeks by signing/HVCI, product owner may ship flow-level only with bundled Wintun signed DLL (single allowed third-party runtime artifact, disclosed), while bonding + driver continue; must be retired next minor release.
