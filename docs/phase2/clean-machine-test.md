# Clean-machine offline test (installer transactional, Sec.8.3, Sec.10.3)

Date: 2026-10-20
Status: procedure documented, not yet run on Hyper-V farm. Installer now requires and copies the generated SPDX sidecar; it no longer fabricates a fallback SBOM. The generated CI artifact is SPDX 2.3 only; SPDX 3.0/VEX are pending.

## Goal

Validate that the installer is truly offline (one signed bundle, zero user prerequisites) and that journaled rollback works on a clean machine with no prior Balancify state, no .NET 10, no WinAppSDK, no driver.

## Prerequisites

- Hyper-V VM templates (x64 + ARM64, HVCI on/off, VBS/Memory-Integrity on/off) — Phase 4 CI (Sec.10.3 row)
- Offline bundle: `Balancify-0.1.0-offline.exe` (bootstrapper carrying MSI + driver + runtimes + SBOM sidecar)
- No internet access in VM (unplug vSwitch or firewall block)
- Snapshot before install for rollback verification

## Steps

1. **Snapshot clean VM**: no Balancify driver, service, adapter, no .NET 10, no WinAppSDK.
2. **Copy offline bundle** via host drive share (not network).
3. **Run installer dry-run**: `balancify-installer.exe --dry-run C:\temp\journal.json`
   - Expect: preflight OK, all steps logged, no changes.
4. **Run installer**: `Balancify-0.1.0-offline.exe /quiet`
   - Check journal at `C:\ProgramData\Balancify\install.json` — 8 entries (preflight, driver, service, adapter, runtimes, gui, sbom, self-test), each with rollback.
5. **Verify install**:
   - `pnputil /enum-drivers | findstr balancify` — driver present, attestation-signed, HVCI flags /integritycheck /DYNAMICBASE /NXCOMPAT /Qspectre PoolTag BlaB.
   - `sc query BalancifyService` — running, restricted account `NT SERVICE\BalancifyService`.
   - `netsh interface ip show config` — virtual adapter present, MTU 1440.
   - `dotnet --list-runtimes` — .NET 10 present if bundled, or self-contained shell runs.
   - `Get-AppxPackage -Name Balancify` — GUI registered (if MSIX path).
   - Firewall: `netsh advfirewall firewall show rule name=Balancify` — UDP allow.
   - SBOM: `C:\ProgramData\Balancify\sbom.spdx.json` exists and is byte-for-byte the CI-generated SPDX 2.3 inventory from the locked Cargo graph. Validate it with an SPDX 2.3 parser; do not expect VEX or claim vulnerability status until reviewed VEX is implemented.
6. **Self-test hook**: installer runs first-run self-test — loop through embedded endpoint, local mode, expect `passed=true`, checks driver loaded, service reachable via IPC, adapter MTU, SBOM sidecar exists, ETW provider registered, offline VM per clean-machine-test.md.
7. **Induce failure** (rollback test):
   - Corrupt driver INF (or block service creation) and rerun installer — expect rollback in reverse order, journal shows inverse ops, no leftover driver/service/adapter/runtimes/sbom.
   - Verify: `pnputil /enum-drivers` no balancify, `sc query` no service, adapter gone, routes restored, SBOM sidecar removed.
8. **Uninstall**:
   - `Balancify-0.1.0-offline.exe /uninstall /quiet`
   - Verify same as rollback: no leftover, routes restored, runtimes we added removed, pre-existing runtimes kept, SBOM sidecar removed.
9. **Offline check**: repeat with VM offline (no internet) — installer must not attempt download, must use bundled runtimes only.
10. **HVCI**: run on VM with Memory Integrity on — driver must load, Verifier `verifier /standard /driver balancify.sys` flags 0x209BB no bugcheck, HLK HVCIMemoryIntegrity test passes, VBS Running MI enabled via msinfo32.
11. **Throughput Rig B**: use `throughput-rows.md` for real iperf3/UDP-echo runs. `throughput.rs` is a local software-path microbenchmark and must not be used for throughput, added-delay, or failover acceptance. Collect 10 reps, median/p95/spread, ETW CPU/Gbit, and remote-ACK failover convergence.
12. **Soak + Verifier + compat**: run `soak.rs` 1000 iterations 72h simulated + random flaps + 100 resume with Verifier standard+random, no crash, no leaked route/filter/handle/IOCTL buffer, compat matrix VPN clients security suites Hyper-V WSL2 MI on/off.

## Expected artifacts

- Journal JSON at `C:\ProgramData\Balancify\install.json` — 8 entries (preflight, driver, service, adapter, runtimes, gui, sbom, self-test) with rollback reverse order
- Support bundle via IPC: `balancify-app.exe diagnostics` — includes cwnd/in_flight per uplink, congestion diag loss/ecn/delay+paths per_path Vec<PathCongestionDiag> cwnd/in_flight/ecn_ce/delay_events/rtt_min/last_ms, rekey diag, relay stats, failover count+convergence ms (relay_failover_count, relay_conv_ms, uplink_failover_count, uplink_conv_ms), history
- ETW logs: provider `Balancify` with event IDs 100-600 (100 start, 101 stop, 200 up, 201 down, 300 bond reverted, 400 rekey, 401 ECN CE, 402 delay, 500 failover, 600 auth failed) — WPA CPU sampling, working set bounded, power delta
- SBOM at `C:\ProgramData\Balancify\sbom.spdx.json` sidecar: generated SPDX 2.3 inventory from locked Cargo metadata; VEX and vulnerability claims are intentionally absent; verify the installer copy matches the validated CI artifact byte-for-byte.
- Throughput: real Rig B iperf3/UDP echo results with 10 reps, median/p95/spread, ETW CPU/Gbit and remote-ACK failover convergence; local harness output is not acceptance evidence
- Soak: 72h + random flaps + 100 resume with Verifier, no crash, no leaked route/filter/handle/IOCTL buffer, compat matrix VPN/Hyper-V/WSL2/MI

## Checklist (Sec.10.3)

- [ ] 10 reps per VM template (x64, ARM64, HVCI on/off) — report median, p95, spread for install time, self-test time
- [ ] Record Windows build, hardware, driver versions, uplink types, relay region with every result
- [ ] Clean-machine offline test passes with no internet — installer uses bundled runtimes only
- [ ] SPDX 2.3 inventory passes a standards-aware parser, includes the locked production dependency closure and declared license/checksum metadata, and installer copy matches the CI artifact byte-for-byte (no VEX claim)
- [ ] Rollback test passes with no leftovers — driver/service/adapter/runtimes/sbom removed, routes restored
- [ ] HVCI trio passes — Verifier 0x209BB + code integrity, HLK HVCIMemoryIntegrity, VBS/MI VM Secure Boot vTPM test-signing, msinfo32 VBS Running MI enabled
- [ ] Throughput Rig B passes — aggregate >=90% sum, single-flow >=70% sum, mismatched >=100% best (auto-revert Sec.6.4.6), added delay <=3ms wired, loss/reordering <=0.1%, failover <=2s
- [ ] Soak + Verifier + compat passes — 72h soak + random flaps + 100 resume with Verifier standard+random, no crash, no leaked route/filter/handle/IOCTL buffer, compat matrix VPN clients security suites Hyper-V WSL2 MI on/off documented

## Automation

Future CI (Phase 4) will run this via Hyper-V PowerShell Direct:
```powershell
New-VM -Name Balancify-Clean -MemoryStartupBytes 4GB -Generation 2
Set-VM -Name Balancify-Clean -ProcessorCount 4
Add-VMDvdDrive -VMName Balancify-Clean -Path Balancify-0.1.0-offline.iso
Start-VM Balancify-Clean
Invoke-Command -VMName Balancify-Clean -ScriptBlock { C:\Balancify-0.1.0-offline.exe /quiet }
# Then run throughput.rs + soak.rs + netem-rig.sh + analyze.py
# And collect support bundle + ETW + SBOM sidecar
```

## References

- Blueprint Sec.4.8 rev 2.1, Sec.8.3 installer, Sec.9.3 signing, Sec.9.5 HVCI, Sec.10.1 Rig A/B 10 reps, Sec.10.3 acceptance rows, S13 SBOM/VEX, S15 HVCI trio, Sec.4.5 failover, G5 soak+Verifier+compat
- Installer: `installer/src/main.rs` — journaled 8 steps (preflight, driver, service, adapter, runtimes, gui, sbom, self-test) with reverse rollback, dry-run, tests, SBOM sidecar
- Driver: `driver/balancify.vcxproj` HVCI flags, `driver/balancify.inf` Universal, `driver/src/miniport.c` NBL pool, `driver/src/ioctl.c` INJECT_NBL via NdisMIndicateReceiveNetBufferLists
- Throughput: `crates/service/src/bin/throughput.rs` 10 profiles, `crates/service/src/bin/soak.rs` G5, `crates/crypto/src/bin/bench.rs` CPU/Gbit, `tools/lab/netem-rig.sh` 11 profiles, `tools/lab/analyze.py`
- App: `app/src/main.rs` 11 modes (home/rules/diagnostics/tray/relay/bundle/perflow/rigb/hvci/sbom/failover/soak)
- CI: 1b7fa25 success (latest), 89e1f26 success soak fix, 50bf7c8 success app 11 modes
