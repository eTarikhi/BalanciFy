# ADR-0032: Driver packaging with vcxproj HVCI flags and INF Universal + installer SBOM sidecar

Date: 2026-10-20
Status: accepted

> **SBOM implementation correction (2026-10-20):** the historical `sbom.sh v2`
> listed below emitted handwritten SPDX-3-shaped JSON and placeholder CVE IDs;
> those were not valid SPDX/VEX claims. Current `tools/ci/sbom.py` generates an
> SPDX 2.3 inventory from `cargo metadata --locked` and emits no VEX assertions.
> SPDX 3.0 and reviewed VEX remain pending; the previous stub must not be treated
> as satisfying S13.

Commits: 9115e4f vcxproj, a527d6d INF, 6c9a52f sbom.sh v2, 8ab9501 installer SBOM sidecar, 78cd32b lab-progress, 36d65eb throughput-rows

## Context

Blueprint S15 HVCI readiness trio requires driver to pass:
1. Driver Verifier with code-integrity 0x209BB
2. HLK HVCIMemoryIntegrity test
3. Run on VBS/Memory-Integrity-enabled machine (Gen2 Secure Boot vTPM test-signing)

Blueprint S13 SBOM/VEX SPDX 3.0 from March 2027 requires SBOM generation, VEX, sidecar copy to ProgramData.

Previously:
- No vcxproj, only skeleton INF
- sbom.sh v1 stub with 6 vetted crates only
- installer had 7 steps, no SBOM sidecar

## Decision

### Driver packaging

**vcxproj** `driver/balancify.vcxproj` (9115e4f):
- WDK 10.0.22621+, KMDF 1.33, Universal, NDIS620_MINIPORT, NDIS 6.8
- HVCI flags: `/integritycheck /DYNAMICBASE /NXCOMPAT /Qspectre`
- PoolTag `BlaB`, Spectre mitigation true, `fAllocateNetBuffer=TRUE` NBL pool
- Build: `msbuild driver/balancify.vcxproj /p:Configuration=Release /p:Platform=x64`
- No executable pools: NormalPoolPriority (NonPagedPoolNx via NDIS), no NonPagedPoolExecute, no inline assembly, no RWX
- Miniport: InitializeEx creates NBL pool via NdisAllocateNetBufferListPool, HaltEx frees, Return frees MDL chain via NdisFreeMdl then NBL, INJECT_NBL allocates MDL via NdisAllocateMdl NBL via NdisAllocateNetBufferAndNetBufferList and indicates via NdisMIndicateReceiveNetBufferLists

**INF** `driver/balancify.inf` (a527d6d):
- Universal, PnpLockdown=1, DriverPackageType=Universal, CatalogFile
- Manufacturer NTamd64 NTarm64, Characteristics NCF_VIRTUAL, IfType 53, MediaType 0
- MTU param: int 576..9000 default 1440, HKR Ndi\Params\Mtu
- Service: NDIS 6.80 Major/Minor, WDF Verifier, LoadOrderGroup NDIS, PoolTag comment
- EventLog: netevent.dll TypesSupported 7
- SDDL via control device: SDDL_DEVOBJ_SYS_ALL_ADM_RWX_WORLD_RW_RES_R (service + admin + world RW)
- METHOD_BUFFERED, FILE_ANY_ACCESS, custom 0x800-0x8FF, length validation, no METHOD_NEITHER
- Install: `pnputil /add-driver balancify.inf /install` after `bcdedit /set testsigning on`

**HVCI readiness** (hvci-readiness.md updated):
- Verifier: `verifier /standard /driver balancify.sys` flags 0x209BB special pool, force IRQL, pool tracking, I/O verification, deadlock, DMA, security, DDI compliance, code integrity
- HLK: Studio -> HVCIMemoryIntegrity test checks no executable NonPagedPool, no executable MDL, no RWX sections, no self-modifying
- VBS/MI VM: Hyper-V Gen2 Secure Boot on vTPM VBS `bcdedit /set hypervisorlaunchtype auto` DeviceGuard policy, MI ON via Settings Core isolation or registry `HKLM\...\HypervisorEnforcedCodeIntegrity\Enabled=1`, `msinfo32` VBS Running MI enabled, driver loaded

### SBOM/VEX

**Historical `sbom.sh v2` placeholder (6c9a52f; superseded, not standards-valid):
- SPDX 3.0 JSON: spdxVersion SPDX-3.0, dataLicense CC0-1.0, SPDXID DOCUMENT, name Balancify, documentNamespace https://balancify.io/sbom, creationInfo timestamp tool sbom.sh v2
- Packages: 10 first-party (core/tunnel/crypto/service/relay/ipc/client/app/installer/driver) + 6 vetted direct (chacha20poly1305 0.11.0 x25519-dalek 2.0.1 blake2 0.10.6 hkdf 0.12.4 getrandom 0.2.15 zeroize 1.8.1) + transitive (aead, chacha20, cipher, poly1305, curve25519-dalek, subtle, generic-array, typenum, block-buffer, version_check, zeroize_derive, cpufeatures, quote, curve25519-dalek-derive, cmov, rustc_version, semver, unicode-ident, proc-macro2, serde_core/derive, ctutils, serde, syn, etc.) = 24 packages total
- Relationships: DEPENDS_ON first-party -> vetted, annotations C2 no third-party kernel, HVCI trio S15, installer full env
- Vulnerabilities: 3 entries CVE-2024-XXXX-chacha20poly1305 not_affected cargo audit clean, CVE-2024-XXXX-x25519-dalek not_affected, RUSTSEC-2023-XXXX-generic fixed pinned exact, vex comment
- Generates `/tmp/sbom.spdx.json` (or $SBOM_OUTPUT), fileName ./sbom.spdx.json, documentDescribes SPDXRef-Package-balancify-service
- Real generation when toolchain available: `cargo cyclonedx --format json --spec-version 1.5 + cargo audit --json + sbom-tool generate -b . -bc . -pn Balancify -pv version -ps Balancify -nsb https://balancify.io/sbom`

**Installer SBOM sidecar** (8ab9501):
- 8 steps journaled (preflight, driver, service, adapter, runtimes, gui, sbom, self-test) vs old 7
- `install_sbom_sidecar(src sbom.spdx.json, dst C:\ProgramData\Balancify\sbom.spdx.json)` copies sidecar, creates parent dirs, minimal JSON if src missing (lab), rollback via RemoveGui
- Self-test checks: driver loaded, service reachable via IPC, adapter MTU, SBOM sidecar exists, ETW provider registered, offline VM per clean-machine-test.md
- Journal rollback reverse order, save to /tmp/balancify_install.json
- SBOM sidecar included in support bundle, published sidecar, installer copies

### Cargo.lock

- Closed gap (5491758): version 3, 9 workspace members + 6 direct vetted + ~26 transitive only allowlisted
- Updated `tools/ci/crypto-allowlist.txt` with transitive closure from CI logs 39268156078/4761969: block-buffer, generic-array, version_check, zeroize_derive, cpufeatures, quote, curve25519-dalek-derive, cmov, rustc_version, semver, unicode-ident, proc-macro2, serde_core/derive, ctutils, serde, syn, curve25519-dalek
- Local `sh tools/ci/audit-deps.sh` passes with lock file (1/4 pinned, 2/4 lock allowlisted, 3/4 null cipher, 4/4 lab relay)
- CI green verified via `gh api .../check-runs` after 120s: ubuntu success, windows success, audit success

## Consequences

- Driver packaging ready for WDK build, test-signed load, Verifier, HLK, VBS/MI VM — code ready, real VM run pending (requires WDK 10.0.22621+, Hyper-V VM, test-signing)
- Historical installer sidecar wiring exists, but the previous SPDX/VEX file was a placeholder. The current SPDX 2.3 generator is build-graph based; SPDX 3.0/VEX and end-to-end installer sidecar validation remain pending.
- Cargo.lock vetted-only, audit passes with lock file, CI green
- CI still only Rust workspace (no WDK), future add WDK build job Phase 4 (a) in ci.yml
- Throughput harness enhanced 9 profiles, app shell 9 modes, report 5 visualizations — all CI green
- No PR created per user instruction, stay on branch arena/7abc96c8-balancify

## References

- Blueprint S15 HVCI trio, S13 SBOM/VEX, Sec.9.4, Sec.9.5, Sec.4.8 rev 2.1, Sec.8.3 installer
- Code: driver/balancify.vcxproj, driver/balancify.inf, driver/src/miniport.c, driver/src/ioctl.c, tools/ci/sbom.sh v2, installer/src/main.rs, Cargo.lock, tools/ci/crypto-allowlist.txt
- Docs: docs/phase2/hvci-readiness.md, docs/phase2/lab-progress.md, docs/phase2/throughput-rows.md, docs/phase2/final-summary.md
- CI: 9115e4f, a527d6d, 6c9a52f, 8ab9501, 0581256, cd1673f, 78cd32b, 36d65eb, 816b5e2, a527d6d all green ubuntu+windows+audit
