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
7. **First-run self-test hook** (loop through embedded endpoint, local mode).

Each step writes a journal entry; any failure executes the inverse operations
in reverse order, including removal of runtimes we added.

## Tooling

- WiX v5 (MSI) wrapped by a bootstrapper (exe) carrying the payloads; or
  MSIX-for-GUI + MSI-for-driver/service split inside one bundle.
- `signtool` + Azure Artifact Signing / OV cert; SHA-256 sidecar published.
- Clean-machine test farm: Hyper-V VM templates (offline, x64 + ARM64,
  HVCI on/off) executed in CI from Phase 4 (§10.3 row).

`balancify.wxs` is the Phase-4 starting skeleton.
