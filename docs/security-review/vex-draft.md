# VEX draft notes — not reviewed, no vulnerability assertions

**Status:** DRAFT / NOT ASSESSED. This is not a signed or reviewed VEX and must not be published as a product security statement. The companion `vex-draft.json` is an OpenVEX-shaped draft scaffold with an intentionally empty `statements` array. Empty statements mean **no vulnerability status has been asserted**; they do not mean “not affected,” “fixed,” or “no known vulnerabilities.” The JSON has not been validated against the current OpenVEX schema.

## Product identity required before issuing VEX

| Field | Draft value / required action |
|---|---|
| Product | BalanciFy Windows client/service/driver/relay; separate component/product identifiers may be needed |
| Release/version | Workspace currently declares `0.1.0`; no supported production release candidate is identified |
| Artifact identity | Missing: final installer/package SHA-256, driver `.sys`/`.cat` hashes, release SBOM serial number and package URLs |
| Source identity | Preparation baseline `528739405ef08404e48574808d5fcc513feef2bb`; final review/release SHA must replace this |
| Author/reviewer | Placeholder; assign product-security owner and independent reviewer |
| Timestamp | Draft timestamp only; regenerate when a real release VEX is reviewed |

## Components requiring vulnerability triage

Use the exact release SBOM (SPDX 2.3/3.0.1 artifacts) to enumerate resolved direct and transitive packages and platform components. At minimum, review:

- `chacha20poly1305` 0.11.0
- `x25519-dalek` 2.0.1
- `blake2` 0.10.6
- `hkdf` 0.12.4
- `getrandom` 0.2.15
- `zeroize` 1.8.1
- Every dependency resolved by `Cargo.lock`, including target-specific crates
- Windows 11 supported builds, the WDK/SDK inputs, driver package, installer and bundled runtime components, once their release inventory exists

This list is a triage checklist, not a claim that these are the only shipped components. Confirm whether each crate is present in the final binary/package and bind each statement to the actual package URL, purl and hashes.

## VEX authoring checklist

1. Freeze the release candidate and generate/validate SPDX SBOMs from that exact build; record hashes and provenance.
2. Run dependency/advisory triage against the frozen component versions; keep `under_investigation` where reachability/configuration is unknown.
3. Create an OpenVEX statement for every relevant vulnerability with the correct product identifier, status, justification/impact/action statements and reproducible evidence. Do not use `not_affected` solely because a component is believed unreachable or a test passed.
4. Link affected code paths, feature flags, compiled target, mitigations, regression tests and source/artifact hashes for each status.
5. Have an independent reviewer approve statuses and exceptions. Validate against the published OpenVEX schema and release process; record validator version/result.
6. Reissue on package/source changes or new advisories. Keep this draft separate from public vulnerability disclosures.

## Known information limits

No vulnerability database scan or release-SBOM-to-binary correlation was performed for this draft. Existing CI dependency policy/SBOM checks are useful build evidence, but they are not vulnerability triage, VEX review, exploitability analysis, or a security certification.
