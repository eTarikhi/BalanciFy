# ADR-0036: Selectively repair unacknowledged FlowData

Date: 2026-10-21
Status: accepted

## Context

The independent FlowData sequence lane made non-striped flows replay-safe and allowed authenticated FlowAck frames to contribute capacity samples. However, FlowData was not retained for recovery: loss of a payload or its FlowAck left the flow packet unaccounted for indefinitely. Reusing the ordered Data lane would create gaps and conflate the two delivery semantics.

## Decision

- Retain eligible FlowData plaintext and retry metadata in the existing bounded per-session pending map until its authenticated selective FlowAck arrives.
- After the configured RTO (120 ms by default), reinject an unacknowledged packet at most once by default, using an active path other than the one used for its previous attempt. Preserve its session and FlowData sequence; set `FLAG_REINJECTED`.
- The receiver's existing independent FlowData replay window suppresses duplicate delivery and emits another FlowAck. This repairs both a lost payload and a lost acknowledgment without introducing ordered gaps in the Data lane.
- Account the old path as lost and the new path as a reinjection through the existing congestion-control loss signal and aggregate reinjection counters.
- Bound retained payload to the existing `max_retx_bytes` budget (4 MiB by default) and the existing 4096-entry cap. A held/rate-limited packet removes its pending entry; oversize or evicted entries are not recoverable. A session with only one usable path is not reinjected.

## Consequences

- Positive: the non-striped lane now has bounded, selective, cross-path repair with at-most-once delivery and ACK recovery.
- Negative: retry state consumes memory up to the configured cap; delivery remains best-effort if the retry is evicted, exceeds the cap, has no alternate path, or exhausts the retry budget.
- Compatibility: no wire-version or frame-format change; FlowData and FlowAck already carry the session/sequence and the reinjected flag is already defined.
- Limitation: this is a software-level protocol implementation. It does not constitute WDK, hardware, or end-to-end network validation.

## Validation

- Tunnel regression checks RTO gating, alternate-path selection, same session/sequence, reinjected flag, bounded retry count, duplicate suppression, duplicate re-ACK, and pending-state release.
- Service regression verifies timer-driven FlowData repair emits on another uplink and increments existing reinjection telemetry.
- CI must pass on Ubuntu and Windows before merge.
