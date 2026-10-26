# ADR-0046: Persist authoritative policy in the service

- Status: Accepted for implementation; Windows runtime validation pending
- Date: 2026-10-26
- Supersedes the policy-restoration portion of ADR-0045; the legacy local snapshot remains an explicit migration input only.
- Related: ADR-0002, ADR-0045, ADR-0035, ADR-0038

## Context

The service is machine-wide, but ADR-0045 made each interactive user's `%LOCALAPPDATA%` snapshot authoritative whenever that shell connected. Two users could alternately replace the shared service policy, and a service restart reset policy until a shell replayed it. Durable settings must instead be owned and loaded by the service; the shell's local file must never silently override them.

The dependency policy does not allow adding a general serialization runtime solely for a few fixed configuration fields. The technical blueprint's state/settings comparison prefers inspectable JSON in ProgramData. This decision therefore records a scoped deviation: the machine policy uses a small fixed, versioned, big-endian binary schema with bounded UTF-8 strings and no parser dependency. If a vetted JSON parser is later approved by the dependency policy, the format can be migrated explicitly.

## Decision

- The service owns the canonical policy: bond toggle, duplicate mode, metered-link behavior, and at most 128 validated custom rules. Transient uplink measurements, flows, sessions, tunnel keys, relay addresses, and user-interface state are not persisted.
- The policy store is opened and loaded before the IPC listener starts. Version 1 uses `ProgramData\\Balancify\\machine-policy\\machine-policy.bin` on Windows (or the service state directory on non-Windows lab hosts), a 64 KiB hard size limit, and a 32-bit FNV-1a corruption check. This checksum detects accidental truncation/corruption; it is not cryptographic integrity protection.
- On Windows the dedicated directory and policy files receive a protected DACL granting full access only to LocalSystem and local Administrators, and their owner/group are set to LocalSystem; the service accesses them as LocalSystem. Unix lab files/directories use `0600`/`0700`. The GUI never opens the file; it changes policy over the existing local named pipe.
- Mutations validate and build the complete next in-memory policy first, write a unique same-directory temporary and flush it, rotate the previous generation to `.bak`, atomically promote the new file, and flush the containing directory where supported. Only after a successful save does the service publish the in-memory change. A failed write returns an IPC error and leaves the live policy unchanged.
- Startup uses a valid primary. If it is missing or corrupt and a valid backup exists, the service repairs the primary from the backup and records recovery. If both generations are corrupt, or the primary is from a future schema, startup fails closed rather than silently resetting policy or downgrading data. Stale unique temporary files are ignored.
- Existing per-user preference schema v1 is migrated only after a confirmation dialog. The shell sends one bounded IPC transaction; the service updates the mode settings and merges imported custom rules with current machine rules. ID conflicts are remapped by the service, unrelated rules are preserved, and retrying an import is idempotent by rule content. On success, the shell deletes the obsolete local snapshot. There is no automatic replay or ongoing local policy save.
- Existing single-setting and rule mutation IPC requests remain supported and now persist through the same service transaction. Additive IPC opcode 12 carries the explicit bulk migration/settings transaction; a pre-ADR-0046 service may reject that opcode, while the existing request codes remain unchanged.
- The existing named-pipe transport admits Interactive Users. This ADR does not claim per-user roles or administrator-only policy authorization; that remains an IPC authority follow-up. The UI makes the machine-wide effect explicit during legacy import. This is not a substitute for Windows identity/authorization testing.

## Consequences

Service restarts retain accepted settings for all clients. Local user files are no longer a source of authority and cannot overwrite newer machine policy without an explicit import. The fixed format avoids a new runtime dependency but is less human-editable than the blueprint's JSON preference; support and repair use the versioned decoder and backup recovery, not manual edits. Future schema changes require an explicit migration and regression tests. Source and unit tests do not establish Windows ACL correctness, service-account access, or installed-service restart behavior; those require native Windows CI and VM validation.
