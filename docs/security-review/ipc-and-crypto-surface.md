# IPC and cryptographic surface (review guide)

## Windows IPC v1

### Transport/authentication

- Canonical pipe: `\\.\pipe\BalanciFy.Service.v1` (`crates/ipc/src/server.rs`). Server creates a first instance with `FILE_FLAG_FIRST_PIPE_INSTANCE`, byte-mode nonblocking reads, and `PIPE_REJECT_REMOTE_CLIENTS`.
- Explicit DACL: SYSTEM and built-in Administrators receive generic-all; Interactive Users receive read/write. The pipe is local-only; do not expose the loopback lab TCP adapter as a production control plane.
- On the first non-empty pipe read, the server queries the client session ID (rejects session 0), impersonates the named-pipe client, verifies Interactive Users membership, and checks enabled built-in Administrators membership. It reverts impersonation before continuing. The resulting admin bit is attached to the stream.
- `Request::requires_machine_admin()` classifies `SetFlowRule`, `RemoveFlowRule`, `SetBondMode`, `SetDuplicateMode`, `SetAllowMetered`, and `ApplyMachinePolicy`. The stream handler returns IPC `PermissionDenied` (numeric status 5) before dispatch if the peer is not an authenticated elevated administrator.
- Frame header is 6 bytes (schema, kind, big-endian payload length); `MAX_PAYLOAD` is 1 MiB. The stream handler enforces exact request frames, idle/frame deadlines, and a bounded concurrent client count in the service executable.

### IPC review questions

1. Verify DACL and token APIs under standard, split-token/elevated Administrator, SYSTEM/session-0, remote and service-account clients on supported Windows builds.
2. Check `RevertToSelf` error handling, impersonation lifetime, error paths, pipe handle inheritance, first-instance race behavior and whether `machine_admin` can become stale across client impersonation.
3. Fuzz frame header/payload lengths, partial reads, repeated connections, events, oversized strings/rule sets and duplicate/out-of-order requests; measure per-client memory/threads under exhaustion.
4. Review policy-store ACL/atomic-write race and event behavior after denied/failed writes. Check that a service running as a dedicated account can create/own the state directory and that UI preferences cannot bypass the service gate.
5. Treat standard-user denial and elevated policy-write success as **not accepted until supplied Windows 11 evidence is reviewed**. The Windows hosted CI uses `-Role Auto -ReadOnlyOnly`; it deliberately skips machine mutations and runs under the runner's actual account, so it cannot represent a non-admin user or prove policy-write authorization.

## Noise-style handshake / X25519

- Protocol name in `crates/crypto/src/handshake.rs`: `Noise_IK_25519_ChaChaPoly_BLAKE2s`. The handshake is a first-party implementation of the Noise IK pattern using X25519 DH, ChaCha20-Poly1305 AEAD, BLAKE2s transcript hashing and HKDF-BLAKE2s key schedule. Verify all details against the exact Noise specification and vectors; the repository does not use a general Noise framework for the state machine.
- IK assumes the initiator pins the responder's static key. Flight 1 carries the initiator ephemeral, encrypted initiator static, and encrypted payload; flight 2 carries responder ephemeral and encrypted payload. Directional transport keys and transcript hash are derived at split; transport salt is taken from the handshake hash.
- DH rejects non-contributory X25519 results (`SharedSecret::was_contributory()`); source tests include low-order inputs. The initiator's first flight propagates failure instead of emitting a message when DH fails.
- Ephemerals/static identities are derived from OS CSPRNG or persisted identity material. On Windows the default key store uses DPAPI protection for secret and pin files; legacy raw-key migration exists. Verify the actual configured state root, DPAPI user scope, ACLs and migration behavior in the product service account.
- The code's documented limitations include no replay cache for a captured first flight replayed into a new responder session and no coordinated live post-handshake rekey procedure. `TransportKeys::rekey` derives next keys but does not synchronize/activate them with a peer. These are explicit protocol/session-management review items, not accepted security properties.

### Cryptographic review questions

- Independently verify Noise IK message pattern, DH order, transcript/AAD updates, `MixKey`/`Split` HKDF use, tag-failure behavior, directional key mapping, nonce construction, sequence exhaustion and salt uniqueness.
- Validate the exact pinned crate versions, `Cargo.lock`, feature flags, target-specific implementations, key copies/zeroization, DPAPI FFI buffer release, panic/failure behavior and OS CSPRNG error handling.
- Evaluate ephemeral reuse, responder state consumption after a failed flight, handshake retries, replay/session binding, downgrade/configuration behavior, key rotation and version negotiation.
- Run interoperable vectors against an independent implementation; add property/fuzz tests for all flights, truncation, tampering and low-order points. Source tests and successful CI are not an external cryptographic audit.

## Pinned direct crypto dependencies

Exact versions in the workspace manifest at preparation (verify against the release lockfile before review):

| Crate | Pin | Role |
|---|---:|---|
| `chacha20poly1305` | `0.11.0` | AEAD primitive |
| `x25519-dalek` | `2.0.1` | X25519 DH/static secret wrapper |
| `blake2` | `0.10.6` | BLAKE2s transcript hash |
| `hkdf` | `0.12.4` | Key schedule |
| `getrandom` | `0.2.15` | OS-backed randomness |
| `zeroize` | `1.8.1` | Explicit secret-buffer clearing support |

The complete transitive graph, checksums and target-specific dependencies are in `Cargo.lock`; this table is not a substitute for the exact release SBOM, dependency audit, or reviewed VEX.
