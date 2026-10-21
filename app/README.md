# Balancify desktop application status

The repository currently contains a Rust lab CLI at `app/src/main.rs`; there is
no WinUI 3 project, .NET 10 project, Windows App SDK package, or GUI build yet.
The CLI exercises the IPC v1 contract and stale-Home behavior, but it is not the
production desktop shell. The planned GUI requires a Windows App SDK 2.5.1 and
.NET 10 build pipeline, accessibility work, packaging, and named-pipe transport.

The lab CLI in `app/src/main.rs` uses IPC v1 over TCP. The service binary now
serves request/response messages on `127.0.0.1:5000` by default; override the
port with `BALANCIFY_IPC_PORT`. This listener refuses non-loopback addresses
and caps concurrent local IPC clients at 16. The CLI client also accepts only
loopback socket addresses; malformed and remote addresses fail explicitly
instead of silently falling back to the default.
It is a development adapter, not a substitute for the production ACL-protected
Windows named pipe and caller-identity authentication. The lab adapter supports
v1 event push at the requested capped 1–10 Hz rate. The CLI Home view keeps an in-memory last-known-good snapshot, applies Stats/uplink events, and marks cached status stale after a read timeout, IPC stream failure, rejected event subscription, or partial policy/bond event. A new event alone does not clear a connection-stale marker; only a full status refresh does. Event-read timeouts are treated as idle polls only before any frame byte arrives; a timeout mid-frame is propagated as a stream error and shown with the stale cached view. The CLI exits gracefully after displaying the stale view. This is not yet a persisted cache or production GUI.

Planned production-shell rules carried from the blueprint (§8); these apply to
the future GUI, not the current lab CLI:
- The GUI **never** opens sockets or touches the driver; it renders only
  messages pushed by the service over the ACL-protected named pipe.
- Runs unprivileged; elevation happens once, in the installer (§8.3).
- Session-scoped last-known-good snapshot + stale markers; 1 Hz telemetry subscription; virtualized lists remain production GUI work.
- Shipped self-contained or with bundled runtimes inside our installer (§4.8 rev 2.1).

Views to implement (Phase 4): Home, Rules, Relay & account, Diagnostics, tray flyout.
