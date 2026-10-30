# Trust boundaries and threat model (draft)

**Review status:** a scoping aid only; no independent review has been completed. Risk ratings are initial triage, not signed findings.

## Assets

- Client static identity secret, ephemeral private keys, relay pins, and derived tunnel traffic keys.
- Integrity/confidentiality of encapsulated traffic, per-direction nonce/sequence state, and authentication tags.
- Machine-wide policy, driver configuration, route/adapter state, and service/driver availability.
- IPC request integrity and caller privilege; support diagnostics and any data exported by them.
- Relay availability and the integrity of packets forwarded to/from the remote network.

## Trust boundaries

| Boundary | From → to | Attacker capability / exposed input | Source controls to verify | Residual questions |
|---|---|---|---|---|
| TB-1 | Unprivileged GUI/user process → service IPC | A local process can open a permitted local pipe and submit arbitrary frames; user can crash/restart their client. | Protected named pipe DACL; remote clients rejected; nonzero client session; impersonation and Interactive Users check; v1 frame length limit; stream deadlines; machine-admin classification before dispatch. | Validate behavior with standard, split-token admin, elevated admin, SYSTEM/session-0, malformed-frame and disconnect tests on Windows 11. Confirm service account/pipe ACL in production packaging. |
| TB-2 | Local unprivileged user → machine-wide policy | A standard user may attempt mode changes, custom rules, policy import, or corrupt policy files. | Six mutating request variants classified as requiring machine admin; denied with `PermissionDenied`; machine policy is versioned/bounded and persisted transactionally; restrictive directory ACL/modes intended. | Verify actual Windows token/UAC semantics, ACL inheritance, restart/backup recovery, race conditions and filesystem behavior on a target VM. |
| TB-3 | Service user mode → kernel miniport/control device | Kernel receives IOCTL buffers and packet/NBL lifecycle events; errors can bugcheck the host. | Explicit IOCTLs, validation, bounded queues, source tests and WDK x64/ARM64 builds. | No signed install, IOCTL fuzzing on Windows, Driver Verifier, HVCI, unload-under-load, packet-path or HLK evidence is supplied. Treat this as a critical open boundary. |
| TB-4 | Client ↔ uplink network ↔ relay | Network attacker can drop, delay, reorder, replay, spoof or observe UDP; relay sees traffic after tunnel termination. | Authenticated handshake/pinned relay key, AEAD tunnel frames, sequence/ACK/reorder logic, bounded message handling and relay source checks. | No production relay deployment/interop/abuse review, internet-path measurement, full replay analysis, key-rotation exercise or incident process evidence. Relay compromise exposes forwarded plaintext. |
| TB-5 | Process ↔ persistent local state | Local process may attempt to read/replace identity, pin or policy state; power loss can interrupt writes. | DPAPI-protected identity/pin storage on Windows by default; service-owned policy directory ACL, checksummed fixed-format data, bounded schema, atomic replacement and backup recovery. | Confirm production service account, directory ownership, ACLs, migration from legacy raw key files, DPAPI scope, backup secrecy, rollback resistance and secrets in crash dumps. DPAPI is not a substitute for an independently validated service identity/ACL. |
| TB-6 | Service diagnostics/support export → operator | Support bundles can expose system/network details or secrets if redaction/schema boundaries fail. | Bounded allow-list support bundle and redaction source/tests. | Live ETW export, filesystem ACL, privacy review, malformed bundle and accessibility are not fully accepted on Windows. |

## Adversaries and assumptions

- **Remote network attacker:** controls an on-path uplink or can send arbitrary UDP to exposed relay endpoints, but does not possess a pinned static key. Availability attacks remain possible.
- **Unprivileged local user/process:** can connect to the pipe if granted by its DACL, send malformed requests, race clients, and attempt forbidden policy mutations. They are not assumed to have Administrator/SYSTEM access.
- **Compromised relay/operator:** can deny service and can observe/alter plaintext at the relay boundary. The design does not claim end-to-end confidentiality beyond the relay.
- **Malicious/buggy kernel peer or hostile administrator:** out of the normal privilege boundary; Administrator/SYSTEM can replace binaries, policy or trust anchors. The security model must not claim protection from a fully compromised host administrator.
- **Build/CI compromise:** can substitute artifacts or dependencies; exact pins, dependency allow-list, CI provenance, artifact hashes and release signing are required but not all are independently reviewed.

Assume Windows kernel, DPAPI, OS CSPRNG, Rust compiler/toolchain and pinned crypto crates behave as specified. These are assumptions pending platform and supply-chain review, not test results.

## Threats, current mitigations and review priority

| Threat | Initial priority | Current design/source mitigation | Needed evidence or action |
|---|---:|---|---|
| Standard user changes system-wide network policy | High | Request classification + elevated Administrators token check before service dispatch; explicit error status 5 | Standard-user and elevated-admin Windows 11 runs; verify all six request opcodes and state before/after. |
| Pipe squatting, remote pipe connection, or session-0 service client | High | First-instance creation, local DACL, remote-client rejection, session check and peer impersonation | Native Windows parser/integration CI plus independent Windows 11 VM tests for remote/SYSTEM/session-0. Hosted CI does not test standard-user authorization. |
| Malformed/oversized/stalled IPC retaining resources | High | 1 MiB frame cap, exact frame validation, polling and idle/frame deadlines, bounded clients | Parser/fuzz tests, run-time timeout cases, resource/connection exhaustion tests on Windows. |
| X25519 small-order/non-contributory shared secret | High | Handshake rejects non-contributory shared secrets and has low-order regression tests | Independent protocol/code review, known-answer/interoperability vectors, mutation fuzzing, audit all call sites and transcript state after failure. |
| Handshake replay or state-machine confusion | High | Fresh ephemeral keys and strict flight order; AEAD and transcript hash | The source explicitly lacks a responder replay cache for a captured first flight at a new session; define session/anti-replay requirements and test duplicate/reordered flights. |
| AEAD nonce reuse or cross-session confusion | Critical | Per-session derived directional keys, transcript-bound salt and checked sequence allocators | Review all nonce domains, retransmission semantics and rekey/salt transitions; cryptographic tests do not substitute for a protocol audit. |
| Driver memory-safety/ownership bug | Critical | Rust user mode, C ownership contracts, bounded queues, source tests and WDK build checks | Independent NDIS review, targeted fuzzing, Driver Verifier, HVCI, signed install, crash dump review and sustained traffic under load. All runtime evidence absent. |
| Compromised relay observes/forges forwarded traffic | High | Authenticated client-relay channel protects the untrusted network leg | Relay is intentionally trusted at termination; document operator access, logging, abuse/rate limits, tenant isolation, update/incident controls. |
| Corrupt/tampered machine policy or rollback | High | Fixed versioned bounded encoding, checksum, backup and atomic replacement | Checksum is corruption detection, not cryptographic authenticity; rely on OS ACL and admin boundary. Test ACL, power failure, rollback and recovery on Windows. |
| Supply-chain vulnerable or substituted dependency | High | Exact direct dependency pins and repository allow-list/SBOM CI | Review complete resolved Cargo.lock and transitive inventory for the exact release artifact; reviewed VEX and provenance are still required. |
| Secrets in logs, dumps or support bundles | High | Redacted bounded bundle schema and key zeroization wrappers at selected points | Memory-dump and ETW review, crash diagnostics testing, secret scanning and privacy/security review. Do not claim comprehensive zeroization without tracing clones and FFI buffers. |

## Security acceptance boundary

Passing Rust tests, hosted Windows tests, parser/analyzer CI, WDK compile/link, and SBOM generation are software/build evidence only. They do not establish installed driver safety, Windows token/ACL runtime, protected key lifecycle, HVCI, packet-path correctness, performance or field readiness. Keep the corresponding ledger entries `NOT RUN` or `BLOCKED` until reviewed logs exist.
