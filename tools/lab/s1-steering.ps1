# Spike S1 (blueprint §5.3): prove a user-mode socket can be forced out of a
# chosen uplink on Windows 11 while the routing table prefers another one.
# Run elevated on the lab rig. Records evidence to s1-evidence-<date>.log.
#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'
$log = "s1-evidence-$(Get-Date -Format yyyyMMdd-HHmmss).log"
function Note($m) { $m | Tee-Object -Append $log }

Note "== S1 evidence start $(Get-Date) =="
Note "-- adapters --"
Get-NetAdapter | Where-Object Status -eq 'Up' |
  Select-Object Name, InterfaceDescription, LinkSpeed, ifIndex | Format-Table | Out-String | Note

foreach ($if in (Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.PrefixOrigin -ne 'WellKnown' })) {
    $ip = $if.IPAddress; $idx = $if.InterfaceIndex
    Note "-- forcing egress via $ip (ifIndex $idx) --"
    # Method 1: source-address binding
    try {
        $c = New-Object Net.Sockets.Socket([Net.Sockets.AddressFamily]::InterNetwork,
              [Net.Sockets.SocketType]::Dgram, [Net.Sockets.ProtocolType]::Udp)
        $c.Bind([Net.IPEndPoint]::new([Net.IPAddress]::Parse($ip), 0))
        $c.SendTo([Text.Encoding]::UTF8.GetBytes("s1-$ip"),
                  [Net.IPEndPoint]::new([Net.IPAddress]::Parse("8.8.8.8"), 53)) | Out-Null
        Note "  source-bind: sent"
        $c.Close()
    } catch { Note "  source-bind: FAILED $_" }
    # Method 2: IP_UNICAST_IF (option 31) on a fresh socket
    try {
        $c = New-Object Net.Sockets.Socket([Net.Sockets.AddressFamily]::InterNetwork,
              [Net.Sockets.SocketType]::Dgram, [Net.Sockets.ProtocolType]::Udp)
        $bytes = [BitConverter]::GetBytes([System.Net.IPAddress]::HostToNetworkOrder([int]$idx))
        $c.SetSocketOption([Net.Sockets.SocketOptionLevel]::IP, 31, $bytes)
        Note "  IP_UNICAST_IF: set on ifIndex $idx"
        $c.Close()
    } catch { Note "  IP_UNICAST_IF: FAILED $_" }
}

Note "-- strong/weak host settings --"
Get-NetIPInterface -AddressFamily IPv4 |
  Select-Object ifIndex, WeakHostSend, WeakHostReceive | Format-Table | Out-String | Note

Note "-- capture check: run pktmon side-by-side in a second shell --"
Note "pktmon filter add -t UDP -p 53 ; pktmon start --etw ; ... ; pktmon stop"
Note "== S1 evidence end =="
Write-Host "Evidence written to $log (attach pktmon capture for the pass decision)."
