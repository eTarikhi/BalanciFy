# ADR-0044: Secure relay failover must include handshake recovery

- Status: Accepted
- Date: 2026-10-25
- Related: ADR-0035, ADR-0038, ADR-0043

## Context

The service could switch its selected relay after a liveness timeout, but the socket adapter did not re-establish the secure session at the new endpoint. Further, the failover timer was based only on authenticated relay acknowledgements, so a relay that never completed the initial handshake could leave the service on that relay indefinitely. A single configured relay identity was also insufficient to authenticate a failover target with a distinct static key.

## Decision

- Start relay failure detection when the first outbound handshake flight is sent. Keep a single-use Noise initiator active for the full failure-detection interval; retry only after that interval if no secondary switch occurred. Retries do not reset the failure timeout, allowing delayed responses to match the active transcript.
- On endpoint change, send a new handshake to that endpoint, using the primary or secondary identity explicitly associated with the selected address.
- Validate an explicitly configured secondary identity against the pinned relay set before creating persistent service identity state. The secondary identity defaults to the primary identity to support replicated relays sharing a static key.
- `BALANCIFY_SECONDARY_RELAY_PUBLIC_KEY` is accepted only when a secondary endpoint is configured; it must be strict nonzero 32-byte hex and pinned whenever the pin store is nonempty.
- While a secure handshake is pending, ignore non-handshake datagrams and suppress old-session tunnel sends and driver TX dequeues. Resume traffic only after the new handshake response authenticates.
- A valid handshake response counts as relay liveness and failover convergence.

## Consequences

This closes the gap between relay selection and secure transport establishment, including initial handshake loss and failover to a differently keyed relay. Unit tests cover timeout initiation/retry-clock stability and endpoint-to-identity selection. Packet loss, multi-relay failover timing, and service/driver behavior under real network failure remain to be verified in the lab; source tests do not establish runtime acceptance.
