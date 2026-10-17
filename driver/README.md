# Balancify NDIS virtual miniport (first-party driver)

Phase 2 / Spike S2b starting point. **Built only with the WDK on Windows**
(Visual Studio + WDK 26100+); never built by the Linux CI job.

- `src/miniport.c` — NDIS 6.x miniport skeleton: `DriverEntry`,
  `MiniportInitializeEx`, `MiniportSendNetBufferLists`, `MiniportReturnNetBufferLists`,
  `MiniportHaltEx`, `MiniportResetEx`. Packets cross to the service through a
  shared ring (design: §4.1/§4.3 of `docs/technical-blueprint.md`).
- `miniport.inf` — class Net, PnP-friendly, no co-installers.
- Signing: attestation path first (§9.3); HVCI readiness trio per §9.4.

## Hard rules carried from the blueprint (§9.2)

1. Minimal IOCTL surface; every buffer length and pointer validated.
2. No pageable-code surprises: NDIS callbacks at DISPATCH_LEVEL respected.
3. Driver Verifier (standard + code-integrity checks) clean in CI lab from first build.
4. On unload/halt: never strand routes; service reconciles (§4.5).
5. SBOM (SPDX 3.0) + VEX generated per submission from March 2027 rule (§9.4).

## Build (Windows only)

```
# Visual Studio 2026 / WDK, x64 and ARM64, Debug+Release
msbuild balancify.sln /p:Configuration=Release /p:Platform=x64
```

Test-signing for Spike S2b: `bcdedit /set testsigning on` on the lab rig only.
