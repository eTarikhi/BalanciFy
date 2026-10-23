# ADR-0039: Bounded raw-IP ingress and optional miniport injection

- **Status:** Accepted
- **Date:** 2026-10-23
- **Scope:** bounded service ingress and optional one-way Windows packet-injection adapter; not complete NDIS/service integration.

## Context

The miniport's intended medium is raw IP (`NdisMediumIP`), while the sans-I/O scheduler accepts a separate `FlowKey` and payload. The adapter must derive a stable directional key from the exact packet it handles. Independent parsing risks inconsistent fragment affinity, malformed lengths, and state changes before validation.

## Decision

Add `packet::flow_key_from_packet` and `Service::send_ip_packet` as a bounded, platform-independent ingress API:

- Accept complete raw IPv4 or IPv6 packets up to 9000 bytes; the IP-declared length must exactly match the supplied buffer.
- Bounds-check IPv4 options, TCP/UDP headers, and IPv6 extension traversal; cap extension processing at 16 headers.
- Reject IPv6 jumbograms. Do not reassemble fragments; use zero ports for all fragments. IPv6 fragments use the fragment header's immediate next-header value so first and later fragments share a key.
- Do not validate IP or transport checksums or inspect TCP/UDP payloads. Non-TCP/UDP protocols use zero ports.
- Parse fully before `send_ip_packet` mutates scheduling state; pass the original bytes unchanged into the existing `send_app_packet` path.
- Synthetic app traffic is opt-in with `--simulate-app-traffic`, allowed only with explicit debug-only `--insecure-lab`, and generated as raw IPv4/UDP before using the parser. Normal secure mode does not emit fabricated app traffic.
- On Windows, optional `--driver-device` opens `\\.\Balancify`, validates delivered inner packets, and synchronously issues `IOCTL_BALANCIFY_INJECT_NBL`. This remains opt-in pending runtime validation.

## Consequences

- Parser, service-entry, and IOCTL-contract tests can run on Linux and Windows CI without WDK/device handles.
- Fragment affinity is coarser than a full 5-tuple and may combine simultaneous fragmented datagrams between the same addresses/protocol; this avoids inconsistent placement without a reassembly cache.
- Malformed/unsupported packets fail before `send_ip_packet` changes counters, flow tables, or tunnel sequence state. Injection input is independently validated before the user-to-kernel transition.
- The service-to-miniport receive direction is now present as an opt-in source path, but it has not been exercised with an installed driver. The reverse miniport-to-service transmit queue/ring, end-to-end path, and runtime/hardware acceptance remain outstanding S2b work.
