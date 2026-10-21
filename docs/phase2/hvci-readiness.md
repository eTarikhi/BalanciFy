# HVCI readiness trio — S15

Date: 2026-10-19
Status: in progress (code ready, real VM run pending)

## Requirement

Blueprint S15 + Sec.9.4 + Sec.9.5: driver must pass HVCI readiness trio before G3:

1. **Driver Verifier with code-integrity checks**
2. **HLK HVCI readiness test**
3. **Run on VBS/Memory-Integrity-enabled machine**

Microsoft began expanding automatic Memory Integrity enablement to eligible devices with quality updates beginning October 2026 (S14). Treat HVCI-on as normal case.

## What code does today

### Miniport (`driver/src/miniport.c`)

- Handlers: InitializeEx, HaltEx, Pause (Sec.4.5 sleep), Restart (resume), SendNetBufferLists, ReturnNetBufferLists, OidRequest (MTU/stats), Shutdown, DevicePnPEventNotify
- Adapter context: miniportAdapterHandle, netBufferListPool, controlDevice, spinlock, paused, mtu, tx/rx counters
- NBL pool: `NdisAllocateNetBufferListPool` with `fAllocateNetBuffer=TRUE`, `ProtocolId=NDIS_PROTOCOL_ID_DEFAULT`, `PoolTag=BALANCIFY_POOL_TAG`, created in InitializeEx, freed in HaltEx
- No executable pools: uses `NormalPoolPriority` (NonPagedPoolNx equivalent via NDIS), no `NonPagedPoolExecute`, no inline assembly, no RWX sections
- WDK build flags (now in vcxproj 9115e4f): `/integritycheck` (HVCI compatible), `/Qspectre`, `/DYNAMICBASE`, `/NXCOMPAT`, PoolTag `BlaB`, Spectre mitigation, KMDF 1.33, Universal, 10.0.22621.0, NDIS620_MINIPORT, NDIS 6.8
- Send path: counts txPackets/txBytes, completes NBL immediately (skeleton, real impl copies to service ring)
- Return path: for RX NBLs injected via IOCTL, frees MDL chain via `NdisFreeMdl` then `NdisFreeNetBufferList`

### Control device (`driver/src/ioctl.c`)

- WDF control device `\\Device\\Balancify`, symlink `\\DosDevices\\Balancify`, SDDL `SDDL_DEVOBJ_SYS_ALL_ADM_RWX_WORLD_RW_RES_R` (service + admin + world RW, same pattern as named pipe ACL)
- IOCTLs METHOD_BUFFERED, FILE_ANY_ACCESS, custom 0x800-0x8FF:
  - `IOCTL_BALANCIFY_SET_MTU`: ULONG mtu, validates 576..9000
  - `IOCTL_BALANCIFY_GET_STATS`: returns BALANCIFY_STATS (tx/rx packets/bytes, mtu, paused)
  - `IOCTL_BALANCIFY_INJECT_NBL`: raw IPv4/IPv6 packet limited by the current MTU; validates version-specific header and exact declared packet length, returns `STATUS_DEVICE_NOT_READY` while paused, then allocates an MDL/NBL and indicates the IP-only NBL via `NdisMIndicateReceiveNetBufferLists`. The miniport/INF now use `NdisMediumIP` / `*MediaType = 19` with `LowerRange = nolower`, not Ethernet framing. WDK/runtime validation remains pending.
  - `IOCTL_BALANCIFY_SET_QUANTA`: array of (u8 path, u64 quanta), validates count <=256, length % struct size ==0
- Length validation on all IOCTLs, no METHOD_NEITHER, no direct user pointer dereference

### Verifier flags

Proposed flags for `verifier.exe /standard /driver balancify.sys`:

- `0x209BB`: special pool, force IRQL checking, pool tracking, I/O verification, deadlock detection, DMA checking, security checks, DDI compliance, code integrity (HVCI)
- Also enable `0x800000` for code integrity checks (if available)

Command:

```cmd
verifier /standard /driver balancify.sys
verifier /querysettings
```

After reboot, check `!verifier` in WinDbg, run traffic through driver, ensure no bugchecks.

### HLK HVCI readiness test

- HLK Studio -> Tests -> `HVCIMemoryIntegrity` (or `Hypervisor Code Integrity Readiness Test`)
- Requires HLK controller + client with driver installed test-signed
- Test checks:
  - No executable NonPagedPool
  - No executable MDL
  - No RWX sections
  - No inline self-modifying code
- Pass criteria: test passes with no failures

### VBS / Memory Integrity VM

- Hyper-V VM: Generation 2, Secure Boot on, vTPM enabled, VBS enabled via `bcdedit /set hypervisorlaunchtype auto` + `DeviceGuard` policy
- Enable Memory Integrity: Settings -> Core isolation -> Memory integrity ON, or via registry `HKLM\SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity\Enabled=1`
- Install driver test-signed: `bcdedit /set testsigning on`, reboot, `pnputil /add-driver balancify.inf /install`
- Verify: `msinfo32` shows Virtualization-based security Running, Memory Integrity enabled, driver loaded
- Run traffic: `throughput.rs` + `balancify-service` binary, ensure no bugcheck, ETW events 200/201/300/400/401/402/500/600 visible

## Lab progress

- Code: NBL pool + INJECT_NBL via NdisMIndicateReceiveNetBufferLists implemented (ADR-0030), length validation, SDDL, no executable pools
- Build: `driver/balancify.vcxproj` exists (9115e4f) with HVCI trio flags, PoolTag BlaB, KMDF 1.33, Universal, 10.0.22621.0 — `msbuild driver/balancify.vcxproj /p:Configuration=Release /p:Platform=x64` (requires WDK)
- Docs: this file + `clean-machine-test.md` + `throughput-rows.md` + `lab-progress.md`
- Real VM run: pending (requires WDK 10.0.22621+, Hyper-V VM Gen2 Secure Boot vTPM VBS, test-signing bcdedit, pnputil)
- CI: driver not yet built in CI (Rust only), future add WDK build job (Phase 4 job (a) in ci.yml), currently verified via docs + code review + pool flags

## Next actions

1. Add WDK build to CI (windows-2022 runner with WDK, `msbuild driver/balancify.vcxproj /p:Configuration=Release /p:Platform=x64`)
2. Run Driver Verifier on test-signed VM with `0x209BB` flags, record bugcheck or clean run
3. Run HLK HVCI readiness test, record pass/fail
4. Run on VBS/Memory-Integrity VM, record `msinfo32` + driver load + traffic
5. Document results in this file with date, Windows build, hardware, driver version

## References

- Microsoft Learn: VBS / Memory Integrity OEM guidance, Driver Verifier, HLK tests, NDIS 6.8 miniport
- Blueprint Sec.9.4, Sec.9.5, S15, G3
- Code: `driver/src/miniport.c`, `driver/src/ioctl.c`, `driver/balancify.vcxproj` (to be created), `driver/balancify.inf` (to be created)
- CI: 4d05142 success (Rust only), f89a617 success
