# ADR-0031: HVCI readiness trio + SBOM/VEX SPDX 3.0

Date: 2026-10-19
Status: accepted

> **Implementation status update (2026-10-20):** the handwritten SPDX 3.0/VEX
> JSON described below was not standards-valid and included placeholder
> vulnerability identifiers. It has been superseded by `tools/ci/sbom.py`, which
> generates an SPDX 2.3 inventory from `cargo metadata --locked`, with declared
> license metadata and no vulnerability-status claims. SPDX 3.0 serialization
> and reviewed VEX statements remain pending; S13 is not complete.

## Context

Blueprint S15 HVCI readiness trio and S13 SBOM/VEX SPDX 3.0 are required for WHCP submissions from March 2027 (Microsoft Tech Community 2026-09-02). The driver is now on critical path from Phase 2 (rev 2), so HVCI validation and SBOM generation must run in CI from Phase 2-3.

Previously:
- `driver/src/miniport.c` had full handler set but no NBL pool, no Driver Verifier annotations
- `tools/ci/sbom.sh` was stub generating minimal SPDX 3.0 JSON with 6 vetted crates + first-party, no VEX, no cargo cyclonedx
- No HVCI readiness doc, no clean-machine test automation

## Decision

### HVCI trio (S15)

1. **Driver Verifier with code-integrity checks**: driver must pass Verifier with flags `0x209BB` (special pool, force IRQL checking, pool tracking, I/O verification, deadlock detection, DMA checking, security checks, DDI compliance, code integrity). Documented in `docs/phase2/hvci-readiness.md` (new).

2. **HLK HVCI readiness test**: run `HVCIMemoryIntegrity` HLK test (from Windows HLK) on driver binary. Test checks for non-executable pools, executable memory allocations, etc. Documented.

3. **Run on VBS/Memory-Integrity-enabled machine**: test driver load on Hyper-V VM with VBS enabled, Memory Integrity on, Secure Boot on. Documented in `docs/phase2/clean-machine-test.md` (existing, enhanced).

Implementation in code:
- `driver/src/miniport.c`: use `NonPagedPoolNx` (via `NormalPoolPriority` already, but ensure no executable pool), no inline assembly, no RWX sections, proper MDL handling with `NdisFreeMdl`, NBL pool with `fAllocateNetBuffer=TRUE` (non-executable)
- `driver/src/ioctl.c`: length validation for all IOCTLs (already), SDDL `SDDL_DEVOBJ_SYS_ALL_ADM_RWX_WORLD_RW_RES_R` (ACL'd pipe equivalent for device), no METHOD_NEITHER (uses METHOD_BUFFERED)
- WDK build: `/integritycheck` linker flag (HVCI compatible), `/Qspectre` for Spectre mitigation, `PoolTag` defined

### Historical SBOM/VEX SPDX 3.0 placeholder (superseded; not a valid deliverable)

The following records the earlier handwritten stub only. Its `SPDX-3.0` shape and
placeholder vulnerability identifiers were not standards-valid; none of those
VEX claims are present in the current generator.

- `tools/ci/sbom.sh` previously emitted a handwritten JSON inventory with:
  - `spdxVersion: SPDX-3.0`
  - `creationInfo` with timestamp and creators
  - `packages` for each first-party crate + each vetted crate (chacha20poly1305 0.11.0, x25519-dalek 2.0.1, blake2 0.10.6, hkdf 0.12.4, getrandom 0.2.15, zeroize 1.8.1) with downloadLocation, licenseConcluded, comment per §4.8
  - `relationships` DEPENDS_ON
  - `annotations` for C2 (no third-party kernel component)
  - VEX section: vulnerabilities array with `vulnId`, `status`, `justification` for each vetted crate (initially no known vulns, status `not_affected` or `fixed`)
- This historical stub claimed that a dependency-free shell implementation satisfied S13; that claim is withdrawn. Build-time tools are permitted under §4.8 but are not shipped. The current SPDX 2.3 inventory is a truthful interim output; SPDX 3.0 and reviewed VEX generation remain future work.
- `installer/src/main.rs`: journaled install includes SBOM sidecar copy to `ProgramData\Balancify\sbom.spdx.json` (step 7 self-test)

### Clean-machine offline provisioning

- `docs/phase2/clean-machine-test.md` already has checklist for offline VM install/uninstall, HVCI on/off, x64+ARM64, atomic rollback, no residue. Enhanced with SBOM sidecar check and HVCI trio.

## Consequences

- Positive: HVCI trio is documented; the current generator produces a graph-based SPDX 2.3 inventory.
- Pending: S15 still requires real Verifier/HLK/VBS validation. S13 still requires standards-valid SPDX 3.0 output and reviewed VEX statements; the former JSON placeholder does not satisfy the gate.
- Negative: more docs, but required for WHCP
- Neutral: no change to data path, only build/docs

## References

- Blueprint Sec.9.3 driver signing, Sec.9.4 platform security (Memory Integrity auto-enablement Oct 2026), Sec.9.5 HVCI, S13 SBOM/VEX, S15 HVCI trio, §11.2 exception, §4.8 dependency policy
- Code: `driver/src/miniport.c` NBL pool, `driver/src/ioctl.c` length validation, `tools/ci/sbom.py` SPDX 2.3 interim inventory, `installer/src/main.rs` SBOM sidecar
- Docs: `docs/phase2/hvci-readiness.md` (new), `docs/phase2/clean-machine-test.md`, `docs/phase2/throughput-rows.md`
- Microsoft Learn: VBS / Memory Integrity OEM guidance, Driver Verifier, HLK HVCI readiness test, SBOM tool
- CI: f89a617 success
