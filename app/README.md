# Balancify desktop shell (WinUI 3 / .NET 10)

Phase 3+ workstream. Skeleton only; builds on Windows with the
Windows App SDK 2.5.1 NuGet + .NET 10 SDK (`dotnet build`).

Rules carried from the blueprint (§8):
- The shell **never** opens sockets or touches the driver; it renders only
  messages pushed by the service over the ACL'd named pipe.
- Runs unprivileged; elevation happens once, in the installer (§8.3).
- Last-known-good + stale markers; 1 Hz telemetry subscription; virtualized lists.
- Shipped self-contained or with bundled runtimes inside our installer (§4.8 rev 2.1).

Views to implement (Phase 4): Home, Rules, Relay & account, Diagnostics, tray flyout.
