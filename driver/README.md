# Balancify NDIS virtual miniport -- Phase 2 S2b source scaffold

This directory contains first-party NDIS miniport and WDF control-device source for the planned packet path (Sec.4.8, Sec.5, Sec.7). No third-party kernel component is used in the intended data path (C2).

**Status:** source-level scaffold only. No WDK build, test-signed installation, Driver Verifier run, HVCI test, or packet-path validation is claimed. `MiniportInitializeEx` still has a TODO where valid NDIS adapter attributes must be registered, so this is not an installable/functional driver. The lab prototype continues to use bound UDP sockets (S2a); G2 requires completing the initialization and lifecycle work, then validating on a Windows test rig.

**Structure:**
- `src/miniport.c` + `src/miniport.h` -- NDIS 6.8 callback skeleton and shared adapter/NBL ownership definitions. Initialization attributes, adapter lifecycle, and actual packet path remain incomplete.
- `src/ioctl.c` -- source implementation of a minimal WDF IOCTL surface: set MTU, inject NBL, query stats, and set quanta. Injected packets are copied to owned nonpaged memory and released when NDIS returns the NBL; this source has not yet been built or verified with WDK/Verifier.
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
- S2b (pending): socket test through miniport -- same as `ordered_stream_survives_a_real_udp_socket` but via NDIS path. IOCTL INJECT_NBL now copies request bytes into owned nonpaged memory, indicates the NBL, and defers freeing its MDL and packet buffer until `MiniportReturnNetBufferLists`; this ownership path still requires WDK + Driver Verifier validation.
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
