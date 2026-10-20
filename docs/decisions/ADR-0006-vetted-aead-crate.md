# ADR-0006 — a vetted, pinned AEAD crate behind the `Crypto` trait

Status: accepted · Date: 2026-10-18 · Supersedes: the "no third-party runtime
dependency at all" reading of ADR-0002 · Related: ADR-0003 (wire v0/v1), §4.8,
§9.2

## Context

Until now the tunnel lane ran on `PassThrough`, a null cipher compiled only for
tests and the `lab-crypto` feature. That made every confidentiality claim
vacuous, and it made one of the rig's own tests vacuous with it: "a tampered
frame is refused" could not fail, because nothing was verified.

The blueprint anticipates this and constrains the fix in two places:

- §4.8 policy: a runtime component is "(a) first-party code, or (b) a vendored
  fork we maintain in-house under a permissive licence, or (c) **a vetted,
  pinned cryptography crate**."
- §4.8 component table, "Crypto primitives": "Vendored, pinned, audited crates
  (RustCrypto / aws-lc-rs) + first-party Noise-style handshake — *Policy
  exception: never roll our own primitives*."

So writing our own ChaCha20-Poly1305 is explicitly out, however attractive a
zero-dependency tree looks. The seam was already in place: `Crypto` in
`crates/tunnel` is the only place the lane touches a cipher.

## Decision

**1. One third-party runtime dependency, exactly pinned.**
`chacha20poly1305 = "=0.11.0"` with `default-features = false, features =
["alloc"]`, declared once in `[workspace.dependencies]`. RustCrypto, Apache-2.0
OR MIT, pure Rust, RFC 8439. `getrandom` is off: nonces here are derived, never
drawn.

**2. A first-party `crates/crypto` owns the discipline, not the primitive.**
`TunnelCipher` is the whole surface: a 32-byte key plus a 3-byte session salt in,
sealed frames out. There is deliberately no API through which a caller can
choose a nonce.

**3. The nonce is derived, never random and never a resettable counter.**

```text
nonce = (salt[0] XOR frame_kind) || salt[1..3] || path[1] || seq_be[8]
```

Reuse of a key/nonce pair under ChaCha20-Poly1305 leaks the XOR of the two
plaintexts and permits tag forgery, so this is the load-bearing decision:

- `seq` is unique within a frame-kind lane and session. Reinjections and
  duplicate-mode copies deliberately reuse it, because they carry the same
  plaintext and must produce the same ciphertext.
- `frame_kind` domain-separates independent sequence spaces by XORing its byte
  into the first salt byte. Thus equal sequence values in Data, FlowData, and
  control lanes cannot reuse a nonce; the authenticated header binds the kind.
- `path` is included so a frame copied onto another path fails to open.
- `salt` separates sessions, so two sessions under one key cannot collide even
  if their counters start equal. The initial wire-v0 formula did not include a
  frame-kind domain; wire v1 is required for this corrected construction.

**4. The frame header is authenticated as AAD.**
`Crypto::seal`/`open` gained an `aad: &[u8]` parameter, and the tunnel always
passes the raw 20-byte header. The receiver must read version, kind, session,
path, sequence and flags before it can decrypt, so they cannot be encrypted —
but they are now covered by the tag and cannot be rewritten in flight (§9.1b).

**5. The dependency audit becomes an allowlist, not a blanket ban.**
`tools/ci/audit-deps.sh` step 1 previously required every declared dependency to
be a path dependency. It now requires every non-path dependency to appear in
`tools/ci/crypto-allowlist.txt` **and** to carry an exact `=x.y.z` pin, either
inline or in `[workspace.dependencies]` when a crate inherits it. Step 2 checks
any committed `Cargo.lock` against the same list. Both negative cases were
exercised by hand: an unpinned allowlisted crate and a pinned unlisted crate
each fail the audit.

## Rejected

- **A first-party AEAD.** Forbidden by §4.8 ("never roll our own primitives"),
  and the wrong trade even if it were allowed: a subtle fault in Poly1305 is
  invisible, while a fault in the reorder buffer is not.
- **Vendoring the source.** Permitted by §4.8 rule (b), but it converts a
  security-fix subscription into a maintenance chore, and the blueprint names
  pinned audited crates for exactly this component.
- **Waiting for the Phase-4 handshake.** The cipher and the handshake are
  separable: the lane can be confidential before it is authenticated, and the
  alternative is another milestone with a null cipher.

## Consequences

- The product now has exactly one third-party runtime dependency. That is a
  real supply-chain surface, and it is why the pin is exact, the allowlist is
  one line, and the audit fails closed.
- **No `Cargo.lock` is committed**, because no Rust toolchain is reachable from
  this environment. The transitive names in the allowlist were read off the
  published manifests and must be regenerated with `cargo tree -p
  balancify-crypto` the first time a toolchain is available. Until then the
  lock check is inert and says so.
- **Key management is still absent.** Nothing yet derives, stores, rotates or
  revokes a key; `TunnelCipher` takes one as input. DPAPI/TPM storage and
  per-session forward secrecy belong to the handshake (§9.2), which is the next
  increment.
- `PassThrough` remains, still `cfg`-gated, still audit-checked, and still the
  cipher the deterministic rig uses — a real cipher in the rig would make
  failures about crypto rather than about scheduling.
- Overhead is now 16 bytes per frame on top of the 20-byte header, i.e. 36
  bytes against a 1400-byte tunnel MTU (2.6 %). `MtuPolicy`'s 60-byte overhead
  assumption still covers it, but the margin is worth remembering.
