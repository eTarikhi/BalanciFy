# Balancify NDIS virtual miniport -- Phase 2 S2b source scaffold

This directory contains first-party NDIS miniport and WDF control-device source for the planned packet path (Sec.4.8, Sec.5, Sec.7). No third-party kernel component is used in the intended data path (C2).

**Status:** source-level scaffold only. `MiniportInitializeEx` now submits NDIS registration attributes followed by revision-2 general attributes, reads the `Mtu` adapter parameter with a bounded 1440-byte fallback, configures raw-IP `NdisMediumIP` to match the INJECT_NBL payload contract, publishes its supported OID list, and unwinds resources if initialization fails. This source has not been built with WDK or test-signed; no Driver Verifier, HVCI, or packet-path validation is claimed, and the miniport is still not a functional service data path. The lab prototype continues to use bound UDP sockets (S2a); G2 still requires a WDK build, completion of the adapter/data-path lifecycle, and validation on a Windows test rig.

**Structure:**
- `src/miniport.c` + `src/miniport.h` -- NDIS 6.8 callback skeleton, registration/general attributes, supported OID queries for media/size/filter/lookahead/statistics and packet counters, and shared adapter/NBL ownership definitions. The actual packet path and runtime lifecycle remain incomplete and unvalidated.
- `src/ioctl.c` -- source implementation of a minimal WDF IOCTL surface: set MTU, inject NBL, query stats, and set quanta. `INJECT_NBL` accepts one raw IPv4/IPv6 packet, validates its version-specific declared length and the configured MTU, rejects requests while paused, then copies it to owned nonpaged memory until NDIS returns the NBL. This source has not yet been built or verified with WDK/Verifier.
- `balancify.inf` -- INF for PnP install, `*IfType = IF_TYPE_PROP_VIRTUAL (53)`, `*MediaType = NdisMediumIP (19)`, and `LowerRange = nolower` for raw IP packets without Ethernet framing.
- `driver.vcxproj` -- WDK 10.0.22621+ build.

**Signing & HVCI:**
- Partner Center enrolment + EV cert ordered in Phase 1 (G1).
- Attestation signing first, HLK only if Server support added (Sec.9.3).
- HVCI readiness trio: Driver Verifier with code-integrity checks, HLK HVCI readiness test, run on VBS/Memory-Integrity machine (Sec.9.5, S15).
- Current CI interim SBOM is SPDX 2.3 from `cargo metadata --locked`; standards-valid SPDX 3.0 and reviewed VEX remain pending (S13).

**Performance targets (G3):**
- Added median delay <= 3 ms wired lab (Sec.10.3)
- CPU per Gbit/s reported, limits set after Spike S2
- Working set bounded

**Security:**
- Minimal IOCTLs, validated lengths, portable C boundary tests for raw IPv4/IPv6 parsing; kernel fuzzing and Driver Verifier remain pending.
- Memory Integrity compatibility is intended but unverified until WDK/HVCI testing.

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
