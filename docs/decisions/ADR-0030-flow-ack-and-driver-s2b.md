# ADR-0030: Explicit flow-level ACK integration + driver S2b NBL indication

Date: 2026-10-19
Status: accepted

## Context

Two remaining gaps from `docs/phase2/next-steps.md`:

1. **Flow-level ACK**: `CapacityEstimator` had `on_flow_packet` (counts sent flow bytes) and new `on_flow_ack` (counts acked flow bytes) per ADR-0028, but `on_flow_ack` was not yet called from service. PolicyEngine had no wrapper for it. The flow lane (Sec.6.2) is non-striped, not ordered in the shared Data sequence, and not retransmitted, so it has no ACK clock — capacity learns only from send rate + idle probes, less accurate than ACK pacing. Wire v1 adds a dedicated FlowData nonce/replay sequence, not an ACK sequence.

2. **Driver S2b socket test through miniport**: `driver/src/ioctl.c` INJECT_NBL stub only counted rxPackets/rxBytes, did not allocate NBL/NB/MDL nor call `NdisMIndicateReceiveNetBufferLists`. Miniport had no NBL pool, so real injection via NDIS path was impossible. Blueprint Sec.6.1 TAP-like adapter, Sec.4.5 pause/resume, G2 S2b requires test-signed load with Driver Verifier and packet loopback through miniport.

## Decision

### Flow-level ACK integration

- `crates/core/src/policy.rs`: add `on_flow_ack(id, bytes, now)` wrapper that calls `capacity.on_flow_ack(bytes, now)` (similar to `on_flow_packet` and `on_ack`)
- `crates/service/src/lib.rs` `ingest_path`:
  - Decode header to detect `is_flow_data = hdr.kind == FrameKind::FlowData`
  - After `rx.ingest(dg, now)`, for each `Event::Payload` where `is_flow_data`, call:
    - `policy.on_flow_ack(uplink, data.len() as u64, now)` — explicit flow-level ACK clock (Sec.6.3)
    - `policy.on_ack(uplink, data.len() as u64, now)` — also count for capacity (same as Data lane via ACK frames)
  - This makes flow-only links learn capacity from both send (`on_flow_packet` in `send_app_packet`) and ACK (`on_flow_ack` in `ingest_path`), improving DRR weights
- Existing `on_flow_packet` remains in `send_app_packet` for flow-level lane

### Driver S2b NBL indication

- `driver/src/miniport.c`:
  - `BALANCIFY_ADAPTER` now has `netBufferListPool` and `netBufferPool` handles
  - `MiniportInitializeEx`: after allocating adapter and spinlock, create NBL pool via `NdisAllocateNetBufferListPool` with `fAllocateNetBuffer=TRUE`, `ProtocolId=NDIS_PROTOCOL_ID_DEFAULT`, `PoolTag=BALANCIFY_POOL_TAG`
  - `MiniportHaltEx`: free pool via `NdisFreeNetBufferListPool` if present
  - `MiniportReturnNetBufferLists`: for RX NBLs allocated via injection, free MDL chain via `NdisFreeMdl` then `NdisFreeNetBufferList`
- `driver/src/ioctl.c`:
  - `IOCTL_BALANCIFY_INJECT_NBL`: now validates min 20 (IP header) max 9000, allocates MDL via `NdisAllocateMdl(adapter->miniportAdapterHandle, inputBuffer, len)`, allocates NBL via `NdisAllocateNetBufferAndNetBufferList(pool, 0,0, mdl,0,len)`, sets port, checks paused flag, indicates via `NdisMIndicateReceiveNetBufferLists(adapter->miniportAdapterHandle, nbl, NDIS_DEFAULT_PORT_NUMBER, 1, NDIS_RECEIVE_FLAGS_DISPATCH_LEVEL)`
  - Logs "INJECT_NBL %zu bytes indicated"
  - This enables S2b socket test: service sends via miniport Send path, relay receives, reorder, deliver, and service can inject via IOCTL for loopback test

## Consequences

- Positive: flow-level capacity now has ACK clock (send + ack), more accurate than send-only; driver S2b now has real NBL indication path, not just counting, enabling test-signed load with Driver Verifier and packet loopback through miniport
- Negative: more code in driver (pool management, MDL freeing), but follows NDIS 6.8 miniport model
- Neutral: no change to wire format, only internal capacity and driver data path

## References

- Blueprint Sec.6.1 TAP-like adapter, Sec.4.5 pause/resume, Sec.6.2 WRH + flow classes, Sec.6.3 capacity, Sec.6.4 DRR + pacing + CC, Sec.8.2 Diagnostics, G2 S2b
- Code: `crates/core/src/policy.rs` `on_flow_ack`, `crates/service/src/lib.rs` `ingest_path`, `driver/src/miniport.c` pool, `driver/src/ioctl.c` INJECT_NBL
- CI: 4d05142 success
- ADR-0028 flow ACK, ADR-0029 per-flow retx
