# BalanciFy WinUI shell (preview)

This is the first native WinUI 3 / .NET 10 desktop shell for BalanciFy. It is an unpackaged, self-contained Windows App SDK app and connects to `\\.\pipe\BalanciFy.Service.v1` as the signed-in interactive user. It requests IPC v1 `GetStatus` and renders service version, bond state, policy revision, MTU, active flows, uplink state/RTT/loss/capacity, and selected counters. Home applies bonding, duplication, and metered-link settings; Rules supports add/edit/remove for up to 128 custom rules while built-ins remain read-only; Relay health displays redacted failover and key-update status; Diagnostics runs the service capacity self-test and exports the service-generated redacted support bundle to a user-selected JSON file. Per-user preferences are stored in `%LOCALAPPDATA%\BalanciFy\ui-preferences.json` and restored when the shell connects or reconnects; Home offers a confirmed reset that deletes only the saved snapshot and leaves live service policy unchanged. Home status refreshes every two seconds.

## Native Windows 11 development

Use Windows 11 x64 or ARM64 with the .NET 10 SDK and Visual Studio 2022 components required by WinUI 3. WSL is not a local prerequisite.

```powershell
cd windows\BalanciFy.App
dotnet restore
dotnet build -c Debug -r win-x64
dotnet run -c Debug -r win-x64
```

For ARM64, substitute `win-arm64`. The default IPC transport is the local named pipe; keep `BALANCIFY_IPC_TRANSPORT` unset for the normal Windows workflow. To test the TCP compatibility path, set `BALANCIFY_IPC_TRANSPORT=loopback-tcp` in both the debug service and app processes and set `BALANCIFY_IPC_PORT` for the service.

The app does not open relay sockets or access the driver. Service status and policy/rule operations use the versioned local IPC contract. The service policy is still in memory only; this shell stores a schema-versioned, size-bounded per-user copy and re-applies it on connection/reconnection. Other IPC clients do not share this preference file, and the service remains at defaults until this shell reconnects after a service restart. The preferences contain policy values and custom rules, not keys or relay credentials. Do not treat this preview shell as release-ready; advanced Diagnostics, accessibility review, navigation polish, tray integration, installer packaging, and Windows runtime validation remain outstanding.
