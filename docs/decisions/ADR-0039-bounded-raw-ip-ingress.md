# ADR-0039: Bounded raw-IP ingress for service flow classification

- **Status:** Accepted
- **Date:** 2026-10-23
- **Scope:** platform-independent service-core boundary only; not an NDIS/service adapter implementation.

## Context

The miniport's intended medium is raw IP (`NdisMediumIP`), while the sans-I/O service scheduler currently accepts a separate `FlowKey` and payload. A future adapter must derive a stable directional flow key from the exact packet it dequeues. Letting adapter code parse arbitrary IP headers independently risks malformed lengths, inconsistent fragment affinity, and state changes before packet validation.

## Decision

Add `packet::flow_key_from_packet` and `Service::send_ip_packet` as a bounded, platform-independent ingress API:

- Accept only complete raw IPv4 or IPv6 packets up to 9000 bytes; the IP-declared length must exactly match the supplied buffer.
- Bounds-check IPv4 header options, TCP header length, UDP header length, and IPv6 extension traversal. Cap IPv6 extension processing at 16 headers.
- Reject IPv6 jumbograms for now; the current driver contract caps inner packets at 9000 bytes.
- Do not reassemble fragments. All fragments use a zero-port key. IPv6 fragments use the fragment header's immediate next-header value so first and later fragments share a key.
- Do not validate IP or transport checksums and do not inspect TCP/UDP payloads. Non-TCP/UDP protocols use zero ports.
- Parse fully before mutating service scheduling state; on success, pass the original packet bytes unchanged into the existing `send_app_packet` path.

## Consequences

- Pure parser and service-entry tests can run on Linux and Windows CI without WDK or device handles.
- Fragment affinity is intentionally coarser than a full 5-tuple and may combine simultaneous fragmented datagrams between the same addresses/protocol; this avoids inconsistent placement between fragments without adding a reassembly cache.
- Malformed/unsupported packets fail before service counters, flow tables, or tunnel sequence state can change.
- This does **not** connect the service to `\\Device\\Balancify`, implement a driver TX queue/ring, reinject received packets, or establish any runtime/hardware acceptance. Those remain separate S2b work.
