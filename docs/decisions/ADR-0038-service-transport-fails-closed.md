# ADR-0038: Fail closed in the service lab binary when relay identity is absent

**Status:** Accepted  
**Date:** 2026-10-21

## Context

The service binary is a lab adapter, not the production Windows service. It previously generated a replacement identity when persistent key storage failed, silently treated malformed/missing relay identity settings as absent, ignored pinned-store errors, and opened a session with a zero-key test cipher whenever no relay key was resolved. That made a demonstration fallback implicit and could hide identity or configuration failures.

## Decision

- Treat service identity persistence and pin-store failures as fatal; never silently generate an ephemeral identity after a storage error.
- Parse `BALANCIFY_RELAY_PUBLIC_KEY` strictly as exactly 32 nonzero bytes of hex. When pins exist, a configured relay must match one; when no explicit key is configured, use the first stored pin.
- Require a resolved relay identity for normal operation. The zero-key session is available only with `--insecure-lab`, only in debug builds, and only when both primary and secondary relay addresses (if supplied) are loopback.
- Reject malformed positional relay addresses and unknown options instead of silently substituting defaults.
- Accept inbound UDP datagrams only from the active configured relay endpoint and propagate UDP send failures/short sends instead of treating them as successful delivery.
- Keep service identity files under the Windows service's `ProgramData\Balancify` or the Unix service account's XDG/home state directory. `BALANCIFY_STATE_DIR` is an explicit absolute-path override.

## Consequences

- Positive: release/default startup cannot silently downgrade to the zero-key lab cipher; relay pins and identity-storage errors are enforced.
- Negative: developers running the service adapter without configured relay keys must explicitly opt into debug-only loopback lab mode.
- Scope: this does not turn the binary into a production service. Its loopback TCP IPC transport, simulated uplinks, and missing NDIS packet integration remain lab scaffolding; production IPC ACLs and caller authentication are still required.

## Validation

Unit tests cover missing identity rejection, debug-only lab mode, primary/secondary loopback restrictions, strict relay-key parsing and pin matching, active-relay source filtering, checked UDP sends, absolute state-directory policy, and IPC client-slot limits. CI validates the binary on Linux and Windows; no production service or named-pipe validation is implied.
