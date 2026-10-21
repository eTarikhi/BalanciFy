# Balancify installer (transactional environment provisioning)

Implements §4.8 rev 2.1 + §8.3. One signed offline bundle; zero user prerequisites.

## Install sequence (journaled, atomic rollback)

1. **Preflight:** OS build ≥ target, HVCI/Memory-Integrity state, disk space,
   AV coexistence probe; snapshot existing routes/adapters/services.
2. **Driver package:** `balancify.sys` + INF (attestation-signed), PnP install.
3. **Service:** create + configure (restricted account, pipe SDDL).
4. **Adapter/routes:** bring up virtual adapter, apply route mode.
5. **Runtimes:** detect .NET Desktop Runtime 10 + Windows App SDK runtime;
   install bundled redistributables only if absent (or self-contained shell).
6. **GUI + tray:** register start-up entries, firewall rules for tunnel UDP.
7. **SPDX sidecars:** install the generated SPDX 2.3 and 3.0.1 inventories.
   Each is copied to a same-directory staging file before replacement; any
   pre-existing sidecar is preserved so a later step failure can restore it.
8. **First-run self-test hook** (loop through embedded endpoint, local mode).

The JSON journal records each step and its inverse action. A journal persistence
failure during installation aborts and triggers rollback rather than silently
continuing; failure to write the post-commit backup-cleanup update is reported
as a warning because installation has already succeeded. Other step failures
execute inverse operations in reverse order, including removal of runtimes we
added. Sidecar rollback restores prior files, while a successful installation
discards their backups.
The sidecar transaction protects against ordinary step failures; power-loss
recovery and a production installer integration test remain separate work.

The current lab binary accepts `--dry-run` and an optional journal path, either
as a positional argument or with `--journal PATH`. Without an explicit path it
reserves a per-process, per-invocation journal file in the system temporary
directory using exclusive creation; on Unix it is created and maintained with
mode `0600`.
Flags are not interpreted as paths, duplicate paths are rejected, and `--help`
prints usage.

## Tooling

- WiX v5 (MSI) wrapped by a bootstrapper (exe) carrying the payloads; or
  MSIX-for-GUI + MSI-for-driver/service split inside one bundle.
- `signtool` + Azure Artifact Signing / OV cert; SHA-256 sidecar published.
- Clean-machine test farm: Hyper-V VM templates (offline, x64 + ARM64,
  HVCI on/off) executed in CI from Phase 4 (§10.3 row).

`balancify.wxs` is the Phase-4 starting skeleton.
