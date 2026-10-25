# BalanciFy WinUI shell (preview)

This is the first native WinUI 3 / .NET 10 desktop shell for BalanciFy. It is an unpackaged, self-contained Windows App SDK app and connects to `\\.\pipe\BalanciFy.Service.v1` as the signed-in interactive user. It requests IPC v1 `GetStatus` and renders service version, bond state, policy revision, MTU, active flows, uplink state/RTT/loss/capacity, and selected counters. The Rules view lists service rules read-only, and Diagnostics can invoke the service's capacity self-test. Home status refreshes every two seconds; rule editing and richer Diagnostics/support-bundle workflows remain in progress.

## Native Windows 11 development

Use Windows 11 x64 or ARM64 with the .NET 10 SDK and Visual Studio 2022 components required by WinUI 3. WSL is not a local prerequisite.

```powershell
cd windows\BalanciFy.App
dotnet restore
dotnet build -c Debug -r win-x64
dotnet run -c Debug -r win-x64
```

For ARM64, substitute `win-arm64`. The default IPC transport is the local named pipe; keep `BALANCIFY_IPC_TRANSPORT` unset for the normal Windows workflow. To test the TCP compatibility path, set `BALANCIFY_IPC_TRANSPORT=loopback-tcp` in both the debug service and app processes and set `BALANCIFY_IPC_PORT` for the service.

The app does not open relay sockets or access the driver. All service state is obtained through the versioned IPC contract. Do not treat this preview shell as a release-ready UI; rule editing, advanced Diagnostics/support-bundle workflows, accessibility review, navigation flows, persistent state, tray integration, installer packaging, and Windows runtime validation remain outstanding.
