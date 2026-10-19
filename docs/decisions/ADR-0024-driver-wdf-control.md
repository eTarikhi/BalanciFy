# ADR-0024: Driver WDF control device + IOCTLs for G2 S2b

Date: 2026-10-19
Status: accepted

## Context

Miniport in the loop is functional stub, not WDF control device (gap for G2 S2b). Blueprint Sec.6.1 TAP-like adapter, Sec.4.5 pause/resume, Sec.8.2 diagnostics require control device for service to talk to miniport: set MTU, inject NBL, query stats, set quanta.

Previously, `driver/src/miniport.c` had only NDIS handlers (InitializeEx, HaltEx, Pause/Restart Sec.4.5, Send/Return NBLs, OidRequest MTU/stats, Shutdown, PnP) with adapter context, MTU, counters, paused flag — compiles on WDK, Verifier basic, but no WDF control device.

## Decision

- `driver/src/miniport.c`: add `WDFDRIVER wdfDriver` in `DriverEntry`, call `WdfDriverCreate` before `NdisMRegisterMiniportDriver`, store, add forward decl `BalancifyCreateControlDevice`, in `MiniportInitializeEx` after allocating adapter and MTU, call `WdfGetDriver()` + `BalancifyCreateControlDevice(wdfDriver, adapter)` — non-fatal for skeleton, logs failure but continues
- `driver/src/ioctl.c` (new): WDF control device `\\Device\\Balancify` + DOS symlink `\\DosDevices\\Balancify`, `WDFDEVICE` with `BALANCIFY_CONTROL` context holding adapter, parallel queue, `EvtDeviceFileCreate`, `EvtFileClose`, `EvtIoDeviceControl` with 4 IOCTLs:
  - `IOCTL_BALANCIFY_SET_MTU (0x800)`: input ULONG mtu, validate 576-9000, update `adapter->mtu` with spinlock, Sec.6.1
  - `IOCTL_BALANCIFY_GET_STATS (0x801)`: output `BALANCIFY_STATS { txPackets, rxPackets, txBytes, rxBytes, mtu, paused }`, copy with spinlock
  - `IOCTL_BALANCIFY_INJECT_NBL (0x802)`: input raw IP packet max 9000, validate, count as rx, log, real impl would allocate NBL/NB/MDL + `NdisMIndicateReceiveNetBufferLists` for S2b loopback test
  - `IOCTL_BALANCIFY_SET_QUANTA (0x803)`: input array `BALANCIFY_QUANTA { path, quanta }`, validate count and size (max 256), log, real impl would update DRR weights via shared memory/service ring Sec.6.4 rule 2
- All IOCTLs METHOD_BUFFERED, FILE_ANY_ACCESS, length-validated, max sizes to prevent overflow, fuzzed same gate as tunnel (no third-party kernel component in data path C2)
- `driver/README.md`: updated to mention control device and IOCTLs
- Non-ASCII replaced with ASCII (Sec. instead of Sec., -- instead of --, etc.) to avoid Rust 2021 prefix errors in other crates (service had similar issue)

## Consequences

- Positive: driver now has WDF control device for service IPC, IOCTLs for MTU, stats, NBL inject, quanta — enables S2b socket test through miniport (next step), test-signed load with Driver Verifier
- Negative: control device creation is non-fatal for skeleton, real driver needs proper WDF init and security descriptor `SDDL_DEVOBJ_SYS_ALL_ADM_RWX_WORLD_RW_RES_R` (already used)
- Neutral: no change to data path yet, Send/Return still loopback

## References

- Blueprint Sec.6.1 TAP-like adapter, Sec.4.5 pause/resume, Sec.6.4 DRR quanta, Sec.8.2 diagnostics, C2 no third-party kernel component
- Code: `driver/src/miniport.c` `DriverEntry` WdfDriverCreate, `MiniportInitializeEx` control device, `driver/src/ioctl.c` new
- CI: 39235545738 success (ubuntu+windows+audits)
- ADR-0009 Wintun fallback, ADR-0024 driver WDF control
