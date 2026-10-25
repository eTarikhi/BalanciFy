# ADR-0045: Persist WinUI policy preferences in the interactive user's profile

- Status: Accepted
- Date: 2026-10-26
- Related: ADR-0035, ADR-0038, ADR-0044

## Context

The WinUI shell could edit service policy and custom flow rules, but the service keeps these values in memory and resets them on restart. Persisting policy in the service would require a new service-owned configuration format, ACL policy, migration behavior, and IPC coordination. The shell already represents an interactive user's preferences and can restore them through the existing versioned IPC without changing the service or wire contract.

## Decision

- The WinUI shell stores its own preference snapshot at `%LOCALAPPDATA%\\BalanciFy\\ui-preferences.json`. The file contains only bonding/duplication/metered-link booleans and at most 128 custom flow rules; it never stores keys, relay identities, or credentials.
- The JSON document is schema-versioned, validated on load and save, and limited to 256 KiB. Writes use a uniquely named same-directory temporary created with `CreateNew`, flush data to disk, then replace the destination. The local file is explicitly user-scoped, not shared machine configuration.
- After the shell connects, it applies its saved policy and upserts its saved custom rules through IPC. It retries restoration after a detected disconnect/service restart. It does not remove unrelated custom rules held by the service.
- If the service accepts an update but the local write fails, the UI reports that the service change succeeded but persistence did not. The UI does not claim the service itself has durable policy.
- Service policy remains volatile for CLI, other IPC clients, and periods before this user's shell reconnects. A future service-owned persistent configuration would require a separate design for machine-wide versus per-user ownership and the service's security/ACL model.

## Consequences

The shell now preserves an interactive user's preferences across service and application restarts without changing IPC v1. The service still starts with defaults until a shell carrying saved preferences connects, and multiple Windows users can apply different local preference sets to the same machine-wide service. Windows compilation, application runtime, named-pipe restoration, and multi-user behavior require native Windows validation; source-level tests do not establish those runtime properties.
