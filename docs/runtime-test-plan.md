# Isolated Windows driver/runtime test plan

**Status: NOT RUN.** No driver has been installed, loaded, test-signed, or exercised by this work. All outcomes below remain **NOT RUN** until the owner supplies the corresponding logs and evidence bundle. GitHub WDK builds are build-only evidence, not runtime acceptance.

## Safety boundary and prerequisites

- Use a disposable Windows 11 Hyper-V Generation 2 VM, a saved host checkpoint, and a private test vSwitch. Record Secure Boot/vTPM and VBS state for every run. If the platform's Secure Boot policy conflicts with test-signing, use a separate explicitly recorded VM configuration/checkpoint for test-signing; never weaken Secure Boot on the host or treat HVCI-on results from a different boot configuration as equivalent. Do not use a daily-use workstation, production VPN endpoint, or the host.
- The host script `scripts/windows/Manage-DriverVmCheckpoint.ps1` only creates/restores named checkpoints. Run it on the Hyper-V host. The guest script `scripts/windows/Invoke-DriverVmTest.ps1` verifies it is inside a Microsoft Hyper-V VM and requires `-ConfirmDisposableVM` for all modifications.
- Test-signing is enabled only by the guest script's explicit `EnableTestSigning` stage plus `-EnableTestSigning`; it invokes `bcdedit` only after the VM and elevated-account checks pass. It was **not run here**. Never enable test-signing on the host.
- Supply a complete package signed with a VM-only test certificate (`.inf`, `.sys`, `.cat`). If trust installation is required, import only the verified public test certificate into the disposable guest's test trust stores; never import a trust anchor on the host or put production signing keys in the VM. Record the certificate thumbprint and revert the guest checkpoint afterward. Do not bypass catalog/signature verification. The install stage refuses a catalog whose Authenticode status is not `Valid`.
- Make no test claim from unsigned WDK artifacts, a successful package staging command, source-contract tests, or a synthetic throughput run. Preserve raw output, Windows build, driver/catalog hashes and signer thumbprint, VM checkpoint names, test configuration, and timestamps.

## Host/guest sequence

1. On the Hyper-V host, create a checkpoint before testing:
   ```powershell
   .\scripts\windows\Manage-DriverVmCheckpoint.ps1 -Action Before -VMName BalanciFy-DriverTest -ConfirmDisposableVM
   ```
   Save the reported checkpoint name in the evidence record. The script uses `ShouldProcess`; confirm the operation. Never let this script stop the VM automatically.
2. In the guest, use an elevated native Windows PowerShell 5.1 console. Capture preflight:
   ```powershell
   .\scripts\windows\Invoke-DriverVmTest.ps1 -Stage Preflight
   ```
3. Only after restoring/confirming the pre-test checkpoint and validating the package, enable test-signing in the guest:
   ```powershell
   .\scripts\windows\Invoke-DriverVmTest.ps1 -Stage EnableTestSigning -ConfirmDisposableVM -EnableTestSigning
   ```
   Reboot the guest manually. Verify the boot option and watermark. Record the reboot and state. Do not issue the command on the host.
4. Install and verify the supplied, already test-signed package:
   ```powershell
   .\scripts\windows\Invoke-DriverVmTest.ps1 -Stage Install -ConfirmDisposableVM -PackageInf C:\Lab\BalanciFy\balancify.inf
   ```
   Record the exact published `oemNN.inf`, PnP instance ID, device state, driver version/hash, service state, and SetupAPI log. If Windows blocks the package, stop and investigate the signature/package; do not weaken enforcement.
5. Run the bidirectional packet-path test using a known test peer and packet probe that emits a unique success marker:
   ```powershell
   .\scripts\windows\Invoke-DriverVmTest.ps1 -Stage PacketPath -ConfirmDisposableVM `
     -ProbeExe C:\Lab\BalanciFyPacketProbe.exe -ProbeArguments @('--peer','10.20.0.10','--seconds','60','--bidirectional') `
     -ExpectedMarker 'BALANCIFY_PACKET_PATH_OK'
   ```
   The probe must be configured so the traffic traverses the BalanciFy adapter, authenticated tunnel, and remote peer. Attach pktmon/Wireshark or ETW evidence, counters, captured packet direction, and peer-side acknowledgement. A normal IP ping that bypasses the virtual adapter is not sufficient.
6. Run teardown-under-load only after a fresh checkpoint and explicit confirmation:
   ```powershell
   .\scripts\windows\Invoke-DriverVmTest.ps1 -Stage UnloadUnderLoad -ConfirmDisposableVM `
     -ConfirmUnloadUnderLoad -PnpInstanceId 'ROOT\BALANCIFY\0000' `
     -ProbeExe C:\Lab\iperf3.exe -ProbeArguments @('-c','10.20.0.10','-t','120')
   ```
   Check system responsiveness, NDIS/WDF logs, crash dumps, PnP removal, completion/cancellation counts, and reinstallation after reboot. A process exit by itself does not prove ownership correctness.
7. For Driver Verifier, take another host checkpoint. In the guest:
   ```powershell
   .\scripts\windows\Invoke-DriverVmTest.ps1 -Stage VerifierConfigure -ConfirmDisposableVM -ApplyVerifier
   ```
   Reboot manually, confirm `verifier /querysettings`, run packet traffic and unload-under-load, then capture System/Application logs and any dump. Expected: `balancify.sys` is listed with the requested checks; no bugcheck, verifier violation, leaked NBL/MDL, stuck pause/halt, or route/filter residue. After capturing output, reset with `verifier /reset` via the guest `Rollback` stage and reboot.
8. Repeat packet-path and teardown tests for HVCI **off** and **on**. Capture raw `Win32_DeviceGuard` values, `msinfo32` VBS/Memory Integrity state, Secure Boot/vTPM state, Code Integrity event logs, and driver signature details for each boot. Toggle Memory Integrity only through the VM's Windows Security/Device Guard configuration, reboot, and record the before/after state; the scripts intentionally do not toggle HVCI automatically.
9. Uninstall in the guest using the exact package/device identifiers, without `/force`:
   ```powershell
   .\scripts\windows\Invoke-DriverVmTest.ps1 -Stage Uninstall -ConfirmDisposableVM `
     -PublishedInf oem42.inf -PnpInstanceId 'ROOT\BALANCIFY\0000'
   ```
10. Run guest rollback and reboot; verify test-signing is off, Verifier is reset, package/device/service/routes are absent, and the VM is stable. If anything is unexpected, stop, preserve logs, and restore the named host checkpoint:
    ```powershell
    .\scripts\windows\Invoke-DriverVmTest.ps1 -Stage Rollback -ConfirmDisposableVM `
      -PublishedInf oem42.inf -PnpInstanceId 'ROOT\BALANCIFY\0000'
    # On host, after shutting down the guest manually:
    .\scripts\windows\Manage-DriverVmCheckpoint.ps1 -Action RestoreBefore `
      -VMName BalanciFy-DriverTest -CheckpointName 'BalanciFy-Before-<timestamp>' -ConfirmDisposableVM
    ```
11. On the Hyper-V host, create an after checkpoint only after evidence has been copied out and the guest is in a known-good state. Keep before/after checkpoint names and do not delete the rollback checkpoint until review.

## Driver Verifier and HVCI evidence expectations

- Proposed Driver Verifier flags: `0x209BB` (special pool, force IRQL checking, pool tracking, I/O verification, deadlock detection, DMA/security/DDI compliance and code-integrity checks, subject to the installed Windows/WDK mapping). Record `verifier /querysettings` output as authoritative; do not claim all checks were enabled based only on a command exit code.
- Expected Driver Verifier result: no violation or bugcheck during sustained traffic, pause/restart, PnP removal, service restart, and teardown. Preserve full dumps and `!verifier` output on failure; restore the VM checkpoint rather than repeatedly rebooting a broken guest.
- HVCI off/on is a two-configuration matrix. For each, record boot-specific `Win32_DeviceGuard` `VirtualizationBasedSecurityStatus`, `SecurityServicesConfigured`, `SecurityServicesRunning`, registry state, Secure Boot/vTPM, and Code Integrity logs. HVCI acceptance requires successful signed driver load and packet/teardown behavior in the HVCI-on VM; PE flags alone are not enough.
- HLK's HVCI readiness test needs a separate HLK controller/client and is an external lab gate. Record its test package, version, result, and logs; do not mark it passed from VM checks.

## Pass/fail evidence table — all NOT RUN

| ID | Test | Pass criteria | Required evidence | Status |
|---|---|---|---|---|
| VM-01 | Hyper-V before checkpoint | Named checkpoint exists before modifications | Host, VM generation/state, checkpoint name/time | NOT RUN |
| VM-02 | Guest identity and test-signing setup | Confirmed disposable Hyper-V guest; test-signing changed only inside that guest; reboot/state captured | Preflight JSON, `bcdedit /enum {current}`, screenshot/log | NOT RUN |
| VM-03 | Package install/load | Valid test-signed catalog; expected device/service present and operational | INF/CAT/SYS hashes, signer thumbprint, PnP/SC/SetupAPI logs | NOT RUN |
| VM-04 | Bidirectional packet path | Test payload reaches remote endpoint in both directions through the driver/tunnel; acknowledgements and captures agree | Probe output, peer log, pktmon/Wireshark/ETW, counters | NOT RUN |
| VM-05 | Teardown/unload under load | Controlled removal completes; no bugcheck, stuck NBL/callback/handle, or persistent device/route | Full process output, event logs, dump review, PnP/routes after reboot | NOT RUN |
| VM-06 | Driver Verifier | Requested settings verified; traffic and teardown complete without violation/bugcheck | `verifier /querysettings`, `!verifier`, ETW, dumps, event logs | NOT RUN |
| VM-07 | HVCI off | Driver loads and packet/teardown tests pass in recorded HVCI-off configuration | DeviceGuard + msinfo32 + CI logs + test output | NOT RUN |
| VM-08 | HVCI on | Driver loads and packet/teardown tests pass under Memory Integrity | DeviceGuard + msinfo32 + CI logs + test output | NOT RUN |
| VM-09 | Uninstall/rollback | Driver/device/service/routes removed; Verifier reset; test-signing off; checkpoint recoverable | PnP/SC/route snapshots, bcdedit/verifier output, before/after checkpoints | NOT RUN |
| VM-10 | After checkpoint | Named post-test checkpoint exists after logs exported | Host checkpoint name/time and evidence hash list | NOT RUN |

Every row remains NOT RUN until logs are supplied and reviewed. Capture one JSON evidence bundle per guest stage under ignored `target/acceptance-evidence/driver-vm/`; retain raw logs outside Git or in the agreed evidence store.
