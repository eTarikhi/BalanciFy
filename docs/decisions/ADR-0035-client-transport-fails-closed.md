# ADR-0035: Client transport fails closed without an authenticated relay identity

Date: 2026-10-21
Status: accepted

## Context

The lab CLI could silently fall back to a predictable zero-key `TunnelCipher` when the relay key was absent, malformed, or unavailable from stored pins. A failed or malformed Noise_IK response could also leave execution on that test cipher. Key-store failures silently generated an ephemeral client identity, and the relay address environment setting was not consistently checked against stored pins. These behaviors are unsuitable defaults for a binary included in release builds.

## Decision

- Use the first stored relay pin when `BALANCIFY_RELAY_PUBLIC_KEY` is unset. If the variable is set, require valid non-zero 32-byte hex and require it to match a stored pin whenever pins exist.
- Refuse startup when no authenticated relay identity is configured, unless `--insecure-lab` is explicitly requested.
- Permit `--insecure-lab` only in debug builds and only for loopback relay addresses. Label this mode as insecure and test-only.
- Treat key-store and pin-store errors as fatal; never replace a failed persistent identity with a newly generated ephemeral identity. Store client keys under `%LOCALAPPDATA%\Balancify` on Windows and `$XDG_STATE_HOME/balancify` (or `$HOME/.local/state/balancify`) on Unix, with `BALANCIFY_STATE_DIR` as an explicit absolute-path override; do not place persistent identities under shared `ProgramData` or `/tmp` by default.
- In secure mode, handshake timeout, unexpected source/framing, and authentication errors are fatal. Never resume with the dummy cipher. Ignore post-handshake UDP datagrams from any endpoint other than the configured relay, reject duplicate handshake responses after session establishment, and propagate transport send errors rather than reporting unsent packets as sent.

## Consequences

- Positive: no implicit zero-key operation, no silent trust reset, and environment configuration cannot override stored pins.
- Negative: an unconfigured client now exits instead of appearing to run; lab users must explicitly opt in.
- Limitation: this CLI is still a protocol/lab harness, not a production adapter or complete application transport. No WDK, hardware, or real-network validation is implied.

## Validation

- Unit tests cover secure/default mode selection, debug-only lab opt-in, loopback restriction, malformed/zero relay keys, pin matching, per-user state-directory precedence/fallbacks, relay-endpoint source filtering, and single-use handshake-response framing.
- CI must pass on Ubuntu and Windows before this ADR is considered implemented.

## References

- `crates/client/src/main.rs`
- ADR-0002 dependency and crypto policy
- Blueprint §§4.7, 9.2, 11.4
