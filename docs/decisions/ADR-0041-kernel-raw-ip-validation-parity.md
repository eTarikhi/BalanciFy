# ADR-0041: Validate raw-IP transport boundaries in the miniport

- **Status:** Accepted
- **Date:** 2026-10-23
- **Scope:** portable C validation at `INJECT_NBL` and miniport TX enqueue boundaries; not checksum validation or runtime driver acceptance.

## Context

The service parser already bounds IPv4/IPv6 lengths, IPv4 options, TCP/UDP headers, and IPv6 extension traversal. The miniport's C prefilter previously checked only IP version and declared IP length. That allowed malformed TCP/UDP records to be copied into owned NBLs or retained in the TX queue, only to be rejected later by the service. Although the service revalidates queued packets, inconsistent boundary checks make send completion and drop behavior harder to reason about.

## Decision

Extend the portable C validator so the driver checks the same structural boundaries as `packet::flow_key_from_packet` before it retains or indicates a packet:

- Require IPv4 IHL and total length to be consistent; reject the reserved IPv4 fragment flag. For non-fragmented TCP/UDP, validate the minimum header and TCP data offset; require the UDP length to exactly cover the unfragmented IP payload. Fragmented IPv4 packets do not require a complete transport header.
- Require IPv6 payload length to exactly match the packet. Walk the same supported extension-header set with a 16-header ceiling, bound each declared extension length, and validate non-fragmented TCP/UDP headers. A structurally bounded non-atomic fragment does not require a complete transport header.
- Keep checksum validation, IPv6 jumbograms, reassembly, and transport payload inspection out of scope. The service continues to revalidate dequeued records before tunnel processing.
- Keep the helper WDK-independent and compile it under strict portable C test flags; add positive and negative fixtures for TCP, UDP, fragmentation, IPv6 extensions, and malformed lengths. Ubuntu CI repeats the C fixtures under AddressSanitizer and UndefinedBehaviorSanitizer.

## Consequences

- Malformed transport boundaries are rejected before NDIS-owned buffers or queue slots are consumed; source tests and x64/ARM64 WDK builds can validate the shared C helper.
- This does not prove checksum correctness or runtime packet delivery, and does not replace service-side validation. Installation, Driver Verifier, HVCI, and end-to-end packet-path acceptance remain pending.
