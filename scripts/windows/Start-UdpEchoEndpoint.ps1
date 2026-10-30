[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [System.Net.IPAddress]$BindAddress,
    [ValidateRange(1024,65535)]
    [int]$Port = 47321,
    [ValidateRange(1,100000)]
    [int]$MaxPacketsPerSecond = 100,
    [string]$OutputDirectory
)

# Test-only echo endpoint. Bind to a dedicated lab IP and firewall the UDP port
# to the Rig B source address. It echoes at most 256 bytes and rate-limits replies.
$ErrorActionPreference = 'Stop'
$udp = [System.Net.Sockets.UdpClient]::new([System.Net.IPEndPoint]::new($BindAddress, $Port))
$start = [DateTime]::UtcNow
$packets = 0L; $dropped = 0L; $windowStart = [Diagnostics.Stopwatch]::StartNew(); $windowPackets = 0
$log = New-Object System.Collections.ArrayList
function Log([string]$Text) { $line = '{0:o} {1}' -f [DateTime]::UtcNow,$Text; [void]$script:log.Add($line); Write-Host $Text }
Log "UDP echo test endpoint bound to $BindAddress`:$Port. Ctrl+C stops it."
try {
    while ($true) {
        $remote = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any,0)
        $payload = $udp.Receive([ref]$remote)
        $windowPackets++
        if ($payload.Length -gt 256) { $dropped++; continue }
        if ($windowStart.Elapsed.TotalSeconds -ge 1) { $windowStart.Restart(); $windowPackets = 1 }
        if ($windowPackets -gt $MaxPacketsPerSecond) { $dropped++; continue }
        [void]$udp.Send($payload,$payload.Length,$remote)
        $packets++
    }
} finally {
    $udp.Dispose()
    if (-not $OutputDirectory) { $OutputDirectory = Join-Path $PSScriptRoot '..\..\target\acceptance-evidence\rig-b' }
    $OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
    New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
    $evidence = [pscustomobject]@{
        evidenceType = 'UDP echo endpoint operational log; not performance acceptance'
        bindAddress = $BindAddress.ToString(); port = $Port; startUtc = $start.ToString('o'); stopUtc = [DateTime]::UtcNow.ToString('o')
        packetsEchoed = $packets; packetsDropped = $dropped; transcriptLog = @($log)
    }
    $path = Join-Path $OutputDirectory ("udp-echo-endpoint-{0}.json" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))
    $evidence | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $path -Encoding UTF8
    Write-Host "Endpoint log: $path"
}
