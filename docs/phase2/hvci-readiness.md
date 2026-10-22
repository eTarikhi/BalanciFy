# HVCI readiness trio — S15

Date: 2026-10-22
Status: in progress (x64 unsigned compile/link passed; signed install and real VM run pending)

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
- WDK build flags: `/integritycheck` (HVCI compatible), `/Qspectre`, `/DYNAMICBASE`, `/NXCOMPAT`, PoolTag `BlaB`, Spectre mitigation, KMDF 1.33, Universal, target 10.0.26100.0, NDIS620_MINIPORT, NDIS 6.8. CI uses pinned WDK NuGet 10.0.26100.6584. Run 39729084768 successfully compiled and linked the x64 driver with signing disabled; the unsigned `balancify.sys` and build log were uploaded as seven-day artifacts. This validates the CI compile/link path only—not signed installation, Driver Verifier, HVCI, or packet-path behavior.
- Send path: service transmit ring is absent; each NET_BUFFER increments `txErrors` and completes with `NDIS_STATUS_FAILURE` (or `NDIS_STATUS_PAUSED` during pause). TX success/byte counters remain zero.
- Return path: for RX NBLs injected via IOCTL, frees MDL chain via `NdisFreeMdl` then `NdisFreeNetBufferList`

### Control device (`driver/src/ioctl.c`)

- WDF control device `\\Device\\Balancify`, symlink `\\DosDevices\\Balancify`, SDDL `SDDL_DEVOBJ_SYS_ALL_ADM_RWX_WORLD_RW_RES_R` (service + admin + world RW, same pattern as named pipe ACL)
- IOCTLs use METHOD_BUFFERED with explicit `FILE_READ_ACCESS` for GET_STATS and `FILE_WRITE_ACCESS` for mutations; device DACL allows SYSTEM and Administrators only:
  - `IOCTL_BALANCIFY_SET_MTU`: ULONG mtu, validates 576..9000 and only confirms the configured value; different values return `STATUS_NOT_SUPPORTED` because dynamic NDIS MTU reconfiguration is not implemented
  - `IOCTL_BALANCIFY_GET_STATS`: returns BALANCIFY_STATS (tx/rx packets/bytes, mtu, paused)
  - `IOCTL_BALANCIFY_INJECT_NBL`: raw IPv4/IPv6 packet limited by the current MTU; validates version-specific header and exact declared packet length, returns `STATUS_DEVICE_NOT_READY` while paused, then allocates an MDL/NBL and indicates the IP-only NBL via `NdisMIndicateReceiveNetBufferLists`. The miniport/INF now use `NdisMediumIP` / `*MediaType = 19` with `LowerRange = nolower`, not Ethernet framing. WDK/runtime validation remains pending.
  - `IOCTL_BALANCIFY_SET_QUANTA`: array of (u8 path, u64 quanta), validates count <=256 and exact element sizing, but currently only logs the count; no scheduling state is changed
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
- Build: `driver/balancify.vcxproj` targets 10.0.26100.0 with HVCI flags, PoolTag BlaB, KMDF 1.33, Universal. CI run 39729084768 installed the pinned WDK and successfully compiled/linked `balancify.sys` on `windows-2022`/VS 2022 with `SignMode=Off`; unsigned driver and log artifacts are retained for seven days. No signed install or hardware acceptance is implied.
- Docs: this file + `clean-machine-test.md` + `throughput-rows.md` + `lab-progress.md`
- Real VM run: pending (requires a WDK-built driver, Hyper-V VM Gen2 Secure Boot/vTPM/VBS, test-signing, and pnputil install)
- CI: PR run 39729084768 passed all five jobs: x64 WDK compile/link, Rust Windows and Linux tests, lab analysis tests, and dependency/crypto audit. It uploaded `balancify-driver-unsigned-x64` and `balancify-wdk-build-log` artifacts with seven-day retention. This is software/CI evidence only; signed installation, Driver Verifier, HVCI/VM, and packet-path behavior remain unverified.

## Next actions

1. Preserve the unsigned CI artifact as software evidence; review the driver lifecycle and prepare a test-signed, isolated VM build.
2. Review remaining NDIS/WDF lifecycle issues and decide the safe first-load procedure.
3. Run Driver Verifier on a test-signed VM with `0x209BB` flags; record bugcheck or clean run.
4. Run the HLK HVCI readiness test and VBS/Memory-Integrity VM check; record pass/fail and traffic behavior.
5. Document results here with date, Windows build, hardware, and driver version. CI compilation alone does not close G2.

## References

- Microsoft Learn: VBS / Memory Integrity OEM guidance, Driver Verifier, HLK tests, NDIS 6.8 miniport
- Blueprint Sec.9.4, Sec.9.5, S15, G3
- Code: `driver/src/miniport.c`, `driver/src/ioctl.c`, `driver/balancify.vcxproj`, `driver/balancify.inf`
- CI: 4d05142 success (Rust only), f89a617 success
