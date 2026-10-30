# Architecture overview

## Product shape

BalanciFy is a Windows multi-uplink networking prototype. Its intended path combines a user-mode Rust service, a first-party Windows NDIS virtual miniport, an unprivileged WinUI client, and a cooperating relay. Independent flows can be placed across uplinks; packet striping requires the cooperating relay to reassemble/forward traffic and is not available for arbitrary remote endpoints without cooperation.

## Components and principal flows

1. **Desktop client (`windows/BalanciFy.App`)** — unprivileged UI for status and machine settings. It sends bounded IPC v1 requests to the local service; it is not supposed to open tunnel sockets or issue driver IOCTLs directly.
2. **IPC crate (`crates/ipc`)** — binary request/response/event frames, message encode/decode, request classification, stream polling, and Windows named-pipe transport. A loopback TCP adapter exists for non-Windows lab use; it is not a production remote control interface.
3. **Service (`crates/service`)** — service core, policy, scheduling, UDP/tunnel orchestration, IPC request dispatch, persistent machine policy and support/diagnostics. Current executable has explicit debug-only `--insecure-lab` mode and an optional `--driver-device` switch. The CI smoke test runs only the debug lab mode, without the driver switch.
4. **First-party miniport (`driver/`)** — NDIS kernel component and control device; intended to expose an IP interface and bounded data path between Windows networking and the service. This is a high-impact kernel boundary. Hosted WDK compilation does not show that it is installable, safe at runtime, or production-signed.
5. **Crypto/tunnel (`crates/crypto`, `crates/tunnel`)** — first-party handshake/state-machine implementation around pinned cryptographic primitives, tunnel framing, authenticated payloads, sequencing, acknowledgements and receive ordering.
6. **Relay (`crates/relay`)** — cooperating endpoint that terminates the client tunnel and forwards traffic. The relay is a trusted plaintext boundary for traffic it forwards; it is not a blind end-to-end encrypted destination.
7. **Local state** — service-owned bounded binary machine policy and file-backed identity/pin stores. Windows identity secret storage uses DPAPI when the default feature is enabled. State-directory ACLs and service-account deployment need runtime review.

## Data flow

Outbound application packet → Windows IP stack → BalanciFy virtual interface/miniport → service control/data adapter → classifier and scheduler → per-uplink tunnel crypto/UDP socket → remote relay → destination network. Inbound traffic reverses these steps; the relay packet is authenticated/decrypted/reordered before delivery into the local IP path. Exact production routing, service account and miniport deployment remain product/runtime acceptance items, not facts established by this diagram.

## Control flow

The UI opens the local named pipe and sends v1 requests. The service decodes bounded messages, validates the pipe peer, authorizes machine-wide mutation requests, dispatches to service logic, persists accepted machine policy before publishing it, and returns a response. Event subscriptions use the same stream and are bounded by the IPC implementation.

## Current implementation vs. target architecture

The blueprint is a target, not proof of shipped behavior. The service binary currently uses simulated uplink sockets and has optional driver I/O; installers and production service registration are not complete. The NDIS driver and the network data path have source/WDK build evidence, but no signed Windows installation, live packet path, VM stability, performance, HVCI, Verifier or hardware acceptance is asserted here.
