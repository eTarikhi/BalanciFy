[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Aggregate16','MatchedSingle','MismatchedSingle','UdpEcho','Failover','CleanBefore','CleanAfter','CompatibilityTemplate','SoakMonitor')]
    [string]$Scenario,
    [ValidateRange(10,10000)]
    [int]$Repetitions = 10,
    [ValidateRange(5,3600)]
    [int]$DurationSeconds = 30,
    [string]$IperfServer,
    [string]$UdpEchoServer,
    [ValidateRange(1024,65535)]
    [int]$UdpEchoPort = 47321,
    [string]$SourceAddress,
    [ValidateRange(1,100000)]
    [int]$ProbeCount = 1000,
    [ValidateRange(5,1000)]
    [int]$ProbeIntervalMs = 20,
    [double[]]$UplinkBaselineMbps = @(),
    [double]$BestUplinkMbps = 0,
    [double]$NoAppMedianRttMs = -1,
    [string]$AdapterName,
    [ValidateRange(1,120)]
    [int]$FlapDownSeconds = 1,
    [switch]$ConfirmUplinkFlap,
    [ValidateRange(1,1000)]
    [int]$SoakHours = 72,
    [ValidateRange(1,10000)]
    [int]$RequiredResumeCycles = 100,
    [string]$BaselineSnapshot,
    [string]$OutputDirectory
)

# Native Windows 11 PowerShell harness. It never installs a driver or changes
# HVCI/test-signing state. Failover interface flapping is an explicit opt-in.
$ErrorActionPreference = 'Stop'
$script:RunId = [Guid]::NewGuid().ToString('N')
$script:Rows = New-Object System.Collections.ArrayList
$script:Cases = New-Object System.Collections.ArrayList
$script:Log = New-Object System.Collections.ArrayList
$script:RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $script:RepoRoot ("target\acceptance-evidence\rig-b\{0}" -f (Get-Date -Format 'yyyyMMdd-HHmmss')) }
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$csvPath = Join-Path $OutputDirectory 'measurements.csv'
$summaryPath = Join-Path $OutputDirectory 'summary.json'
$os = Get-CimInstance Win32_OperatingSystem

function Log([string]$Text) {
    $line = '{0:o} {1}' -f [DateTime]::UtcNow,$Text
    [void]$script:Log.Add($line); Write-Host $Text
}
function Add-Case([string]$Id,[object]$Result,[string]$Command,[string]$Expected,[string]$Observed) {
    if ($Result -is [bool]) { $Result = if ($Result) { 'PASS' } else { 'FAIL' } }
    [void]$script:Cases.Add([pscustomobject]@{ id=$Id; result=[string]$Result; command=$Command; expected=$Expected; observed=$Observed })
    Log ("{0} [{1}] expected={2}; observed={3}" -f $Result,$Id,$Expected,$Observed)
}
function Add-Measurement([string]$Metric,[int]$Rep,[double]$Value,[string]$Unit,[string]$Operator,[double]$Target,[string]$Evidence) {
    $result = 'NOT ASSESSED'
    if ($Operator -eq '>=') { if ($Value -ge $Target) { $result='PASS' } else { $result='FAIL' } }
    elseif ($Operator -eq '<=') { if ($Value -le $Target) { $result='PASS' } else { $result='FAIL' } }
    [void]$script:Rows.Add([pscustomobject]@{
        RunId=$script:RunId; TimestampUtc=[DateTime]::UtcNow.ToString('o'); Scenario=$Scenario; Repetition=$Rep
        Metric=$Metric; MeasuredValue=[Math]::Round($Value,6); Unit=$Unit; TargetOperator=$Operator; TargetValue=[Math]::Round($Target,6)
        Result=$result; TargetIsThreshold=$true; MeasurementIsObservedData=$true; Details=$Evidence
    })
    Log ("{0} [{1} rep {2}] measured={3} {4}; target {5} {6}" -f $result,$Metric,$Rep,[Math]::Round($Value,3),$Unit,$Operator,$Target)
}
function Get-Stats([double[]]$Values) {
    if (-not $Values -or $Values.Count -eq 0) { return $null }
    $sorted = @($Values | Sort-Object)
    $mean = ($Values | Measure-Object -Average).Average
    $sumSq = 0.0; foreach ($x in $Values) { $sumSq += [Math]::Pow(($x - $mean),2) }
    $sd = if ($Values.Count -gt 1) { [Math]::Sqrt($sumSq / ($Values.Count - 1)) } else { 0.0 }
    if (($sorted.Count % 2) -eq 1) { $median=$sorted[[int][Math]::Floor($sorted.Count/2)] }
    else { $median=($sorted[($sorted.Count/2)-1]+$sorted[$sorted.Count/2])/2.0 }
    $p95Index = [Math]::Min($sorted.Count - 1,[int][Math]::Ceiling(0.95*$sorted.Count)-1)
    return [pscustomobject]@{ Count=$Values.Count; Mean=$mean; Median=$median; P95=$sorted[$p95Index]; Min=$sorted[0]; Max=$sorted[-1]; SampleVariance=[Math]::Pow($sd,2); StdDev=$sd; CoefficientOfVariationPct=$(if ($mean -ne 0) { 100*$sd/$mean } else { 0 }) }
}
function Add-MissingMeasurement([string]$Metric,[int]$Rep,[string]$Unit,[string]$Operator,[double]$Target,[string]$Evidence) {
    [void]$script:Rows.Add([pscustomobject]@{
        RunId=$script:RunId; TimestampUtc=[DateTime]::UtcNow.ToString('o'); Scenario=$Scenario; Repetition=$Rep
        Metric=$Metric; MeasuredValue=$null; Unit=$Unit; TargetOperator=$Operator; TargetValue=[Math]::Round($Target,6)
        Result='FAIL'; TargetIsThreshold=$true; MeasurementIsObservedData=$false; Details=$Evidence
    })
    Log ("FAIL [{0} rep {1}] no valid observed value; target {2} {3}; {4}" -f $Metric,$Rep,$Operator,$Target,$Evidence)
}
function Add-Observation([string]$Metric,[int]$Rep,[double]$Value,[string]$Unit,[string]$Evidence) {
    [void]$script:Rows.Add([pscustomobject]@{
        RunId=$script:RunId; TimestampUtc=[DateTime]::UtcNow.ToString('o'); Scenario=$Scenario; Repetition=$Rep
        Metric=$Metric; MeasuredValue=[Math]::Round($Value,6); Unit=$Unit; TargetOperator='N/A'; TargetValue='';
        Result='NOT ASSESSED'; TargetIsThreshold=$false; MeasurementIsObservedData=$true; Details=$Evidence
    })
    Log ("OBSERVED [{0} rep {1}] measured={2} {3}; no acceptance threshold specified" -f $Metric,$Rep,[Math]::Round($Value,3),$Unit)
}
function Save-Rows {
    if ($script:Rows.Count -gt 0) {
        if (Test-Path $csvPath) { $script:Rows | Export-Csv -LiteralPath $csvPath -NoTypeInformation -Append }
        else { $script:Rows | Export-Csv -LiteralPath $csvPath -NoTypeInformation }
    }
}
function Get-EnvironmentSnapshot {
    $adapters = @(Get-NetAdapter -ErrorAction SilentlyContinue | Select-Object Name,InterfaceDescription,InterfaceIndex,Status,LinkSpeed,MacAddress)
    $balAdapters = @($adapters | Where-Object { $_.Name -match 'Balancify' -or $_.InterfaceDescription -match 'Balancify' })
    $services = @(Get-CimInstance Win32_Service -ErrorAction SilentlyContinue | Where-Object { $_.Name -match 'Balancify' -or $_.DisplayName -match 'Balancify' } | Select-Object Name,DisplayName,State,StartMode)
    $drivers = @(Get-CimInstance Win32_SystemDriver -ErrorAction SilentlyContinue | Where-Object { $_.Name -match 'Balancify' -or $_.PathName -match 'balancify' } | Select-Object Name,State,StartMode,PathName)
    $devices = @(Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object { $_.FriendlyName -match 'Balancify' -or $_.InstanceId -match 'ROOT\\BALANCIFY' } | Select-Object FriendlyName,InstanceId,Status,Class)
    $routes = @()
    foreach ($a in $balAdapters) { $routes += @(Get-NetRoute -InterfaceIndex $a.InterfaceIndex -ErrorAction SilentlyContinue | Select-Object DestinationPrefix,NextHop,RouteMetric,InterfaceIndex) }
    return [pscustomobject]@{ CapturedUtc=[DateTime]::UtcNow.ToString('o'); Computer=$env:COMPUTERNAME; WindowsCaption=$os.Caption; WindowsVersion=$os.Version; WindowsBuild=$os.BuildNumber; BalancifyAdapters=$balAdapters; BalancifyServices=$services; BalancifyDrivers=$drivers; BalancifyDevices=$devices; BalancifyRoutes=$routes }
}
function Test-Admin {
    $id=[Security.Principal.WindowsIdentity]::GetCurrent()
    (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}
function New-ProbeClient {
    $client = [System.Net.Sockets.UdpClient]::new()
    if ($SourceAddress) { $client.Client.Bind([System.Net.IPEndPoint]::new([System.Net.IPAddress]::Parse($SourceAddress),0)) }
    $client.Client.ReceiveTimeout = 5
    return $client
}
function New-ProbePayload([UInt64]$Sequence) {
    $buffer = New-Object byte[] 12
    [Array]::Copy([Text.Encoding]::ASCII.GetBytes('BFR1'),0,$buffer,0,4)
    $seqBytes=[BitConverter]::GetBytes($Sequence); if ([BitConverter]::IsLittleEndian) { [Array]::Reverse($seqBytes) }
    [Array]::Copy($seqBytes,0,$buffer,4,8)
    return ,$buffer
}
function Get-Sequence([byte[]]$Buffer) {
    if ($Buffer.Length -ne 12 -or [Text.Encoding]::ASCII.GetString($Buffer,0,4) -ne 'BFR1') { return $null }
    $b=$Buffer[4..11]; if ([BitConverter]::IsLittleEndian) { [Array]::Reverse($b) }
    return [BitConverter]::ToUInt64($b,0)
}
function Invoke-EchoRun([int]$Rep,[switch]$DoFailover) {
    if (-not $UdpEchoServer) { throw '-UdpEchoServer is required for UdpEcho/Failover.' }
    $ip=[System.Net.IPAddress]::Parse($UdpEchoServer)
    $endpoint=[System.Net.IPEndPoint]::new($ip,$UdpEchoPort)
    $client=New-ProbeClient
    $sent=@{}; $latencies=New-Object System.Collections.ArrayList; $recvSeq=New-Object System.Collections.ArrayList
    $lastSuccess=$null; $maxGap=0.0; $outOfOrder=0; $received=0; $sentCount=0
    $firstAfterDisable=$null; $adapterWasUp=$false; $flapStart=$null; $flapEnd=$null; $flapRestoreAt=$null
    if ($DoFailover) {
        if (-not $ConfirmUplinkFlap) { throw 'Failover mutates link state; pass -ConfirmUplinkFlap only in the isolated lab.' }
        if (-not (Test-Admin)) { throw 'Failover test requires elevated PowerShell to disable/enable the named uplink.' }
        if (-not $AdapterName) { throw '-AdapterName is required for Failover.' }
        $adapter=Get-NetAdapter -Name $AdapterName -ErrorAction Stop
        $adapterWasUp=($adapter.Status -eq 'Up')
        if (-not $adapterWasUp) { throw "Adapter '$AdapterName' was not Up before the test." }
    }
    $clock=[Diagnostics.Stopwatch]::StartNew(); $nextProbe=0.0; $duration=if($DoFailover){[Math]::Max($DurationSeconds,20)}else{[Math]::Max(5,[Math]::Ceiling($ProbeCount*$ProbeIntervalMs/1000.0)+2)}
    try {
        while ($clock.Elapsed.TotalSeconds -lt $duration -and ($DoFailover -or $sentCount -lt $ProbeCount)) {
            if ($DoFailover -and $null -eq $flapStart -and $clock.Elapsed.TotalSeconds -ge 5) {
                Disable-NetAdapter -Name $AdapterName -Confirm:$false -ErrorAction Stop
                $flapStart=$clock.Elapsed.TotalSeconds
                $flapRestoreAt=$flapStart + $FlapDownSeconds
                $script:Log.Add("Failover flap '$AdapterName': disabled at $flapStart s; probes continue while it is down.") | Out-Null
            }
            if ($DoFailover -and $null -ne $flapStart -and $null -eq $flapEnd -and $clock.Elapsed.TotalSeconds -ge $flapRestoreAt) {
                Enable-NetAdapter -Name $AdapterName -Confirm:$false -ErrorAction Stop
                $flapEnd=$clock.Elapsed.TotalSeconds
                $script:Log.Add("Failover flap '$AdapterName': re-enabled at $flapEnd s.") | Out-Null
            }
            if ($clock.Elapsed.TotalSeconds -ge $nextProbe -and ($DoFailover -or $sentCount -lt $ProbeCount)) {
                $payload=New-ProbePayload ([UInt64]$sentCount)
                $sent[$sentCount]=$clock.Elapsed.TotalSeconds
                [void]$client.Send($payload,$payload.Length,$endpoint)
                $sentCount++
                $nextProbe=$clock.Elapsed.TotalSeconds + ($ProbeIntervalMs/1000.0)
            }
            $draining=$true
            while ($draining) {
                try {
                    $remote=[System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any,0)
                    $data=$client.Receive([ref]$remote)
                    $seq=Get-Sequence $data
                    if ($null -ne $seq -and $sent.ContainsKey([UInt64]$seq)) {
                        $now=$clock.Elapsed.TotalSeconds; $received++
                        [void]$recvSeq.Add([UInt64]$seq)
                        $rtt=($now - [double]$sent[[UInt64]$seq])*1000.0
                        [void]$latencies.Add($rtt)
                        if ($null -ne $lastSuccess) { $maxGap=[Math]::Max($maxGap,($now-$lastSuccess)) }
                        $lastSuccess=$now
                        if ($DoFailover -and $null -ne $flapStart -and $null -eq $firstAfterDisable -and [double]$sent[[UInt64]$seq] -ge $flapStart) { $firstAfterDisable=$now }
                    }
                } catch [System.Net.Sockets.SocketException] { $draining=$false }
            }
            Start-Sleep -Milliseconds 5
        }
        # Drain in-flight responses with a bounded tail.
        $tail=[Diagnostics.Stopwatch]::StartNew()
        while ($tail.Elapsed.TotalSeconds -lt 2) {
            try {
                $remote=[System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any,0)
                $data=$client.Receive([ref]$remote); $seq=Get-Sequence $data
                if ($null -ne $seq -and $sent.ContainsKey([UInt64]$seq)) {
                    $now=$clock.Elapsed.TotalSeconds; $received++; [void]$recvSeq.Add([UInt64]$seq)
                    [void]$latencies.Add(($now - [double]$sent[[UInt64]$seq])*1000.0)
                    if ($null -ne $lastSuccess) { $maxGap=[Math]::Max($maxGap,($now-$lastSuccess)) }; $lastSuccess=$now
                    if ($DoFailover -and $null -ne $flapStart -and $null -eq $firstAfterDisable -and [double]$sent[[UInt64]$seq] -ge $flapStart) { $firstAfterDisable=$now }
                }
            } catch [System.Net.Sockets.SocketException] { break }
        }
    } finally {
        if ($DoFailover -and $adapterWasUp) { Enable-NetAdapter -Name $AdapterName -Confirm:$false -ErrorAction SilentlyContinue }
        $client.Dispose()
    }
    $last=-1L
    foreach ($seq in $recvSeq) { if ($seq -lt $last) { $outOfOrder++ }; if ($seq -gt $last) { $last=$seq } }
    $reorderPct=if($received){100.0*$outOfOrder/$received}else{100.0}
    $lossPct=if($sentCount){100.0*($sentCount-$received)/$sentCount}else{100.0}
    $stats=Get-Stats ([double[]]@($latencies | ForEach-Object { [double]$_ }))
    $convergence=if($null -ne $firstAfterDisable -and $null -ne $flapStart){[Math]::Max(0,($firstAfterDisable-$flapStart))}else{$null}
    return [pscustomobject]@{ Sent=$sentCount; Received=$received; LossPct=$lossPct; OutOfOrder=$outOfOrder; ReorderPct=$reorderPct; Latency=$stats; MaxSuccessGapSeconds=$maxGap; FailoverConvergenceSeconds=$convergence; FlapDownSeconds=$FlapDownSeconds }
}
function Invoke-IperfRun([int]$Rep,[int]$Parallel) {
    if (-not $IperfServer) { throw '-IperfServer is required for throughput scenarios.' }
    $iperf=Get-Command iperf3.exe -ErrorAction SilentlyContinue
    if (-not $iperf) { $iperf=Get-Command iperf3 -ErrorAction SilentlyContinue }
    if (-not $iperf) { throw 'iperf3 executable not found in PATH.' }
    $args=@('-c',$IperfServer,'-t',"$DurationSeconds",'-P',"$Parallel",'-J')
    $text=@(& $iperf.Source @args 2>&1 | ForEach-Object { "$_" }) -join "`n"
    $exit=$LASTEXITCODE
    if ($exit -ne 0) { throw "iperf3 exited $exit: $text" }
    $result=$text | ConvertFrom-Json
    $bps=$null
    if ($result.end.sum_received.bits_per_second) { $bps=[double]$result.end.sum_received.bits_per_second }
    elseif ($result.end.sum.bits_per_second) { $bps=[double]$result.end.sum.bits_per_second }
    if ($null -eq $bps) { throw 'iperf3 JSON had no end.sum_received.bits_per_second or end.sum.bits_per_second.' }
    return [pscustomobject]@{ Mbps=$bps/1000000.0; Json=$result }
}
function Get-Threshold {
    $sum=0.0; foreach($x in $UplinkBaselineMbps){$sum += [double]$x}
    switch ($Scenario) {
        'Aggregate16' { return [pscustomobject]@{ Operator='>='; Value=0.90*$sum; Label='90% of summed uplink baselines' } }
        'MatchedSingle' { return [pscustomobject]@{ Operator='>='; Value=0.70*$sum; Label='70% of summed matched-link baselines' } }
        'MismatchedSingle' { return [pscustomobject]@{ Operator='>='; Value=$BestUplinkMbps; Label='best individual uplink baseline' } }
        'UdpEcho' { return [pscustomobject]@{ Operator='<='; Value=3.0; Label='median added RTT <= 3 ms (requires no-app baseline)' } }
        'Failover' { return [pscustomobject]@{ Operator='<='; Value=2.0; Label='remote echo recovery <= 2 s' } }
        default { return $null }
    }
}

try {
    if ($Scenario -in @('Aggregate16','MatchedSingle','MismatchedSingle')) {
        if (-not $IperfServer) { throw 'Provide -IperfServer for throughput scenarios.' }
        if ($UplinkBaselineMbps.Count -lt 1) { throw 'Provide explicit -UplinkBaselineMbps values (one per healthy uplink); baselines are targets, not measurements.' }
        if ($Scenario -eq 'MismatchedSingle' -and $BestUplinkMbps -le 0) { throw 'Provide -BestUplinkMbps from a separate single-link baseline run.' }
        $parallel=if($Scenario -eq 'Aggregate16'){16}else{1}
        $threshold=Get-Threshold
        for ($rep=1; $rep -le $Repetitions; $rep++) {
            $test=Invoke-IperfRun $rep $parallel
            Add-Measurement 'throughput_mbps' $rep $test.Mbps 'Mbps' $threshold.Operator $threshold.Value ("iperf3 parallel=$parallel duration=$DurationSeconds server=$IperfServer")
        }
        Add-Case "throughput-$Scenario" (@($script:Rows | Where-Object Result -eq 'FAIL').Count -eq 0) "iperf3 -c $IperfServer -P $parallel -t $DurationSeconds -J repeated $Repetitions times" $threshold.Label ("Measured Mbps median={0}; target={1} {2}" -f (Get-Stats ([double[]]@($script:Rows.MeasuredValue))).Median,$threshold.Operator,$threshold.Value)
    } elseif ($Scenario -eq 'UdpEcho' -or $Scenario -eq 'Failover') {
        if (-not $UdpEchoServer) { throw 'Provide -UdpEchoServer (a numeric IP for the controlled UDP echo endpoint).' }
        if ($Scenario -eq 'UdpEcho' -and $NoAppMedianRttMs -lt 0) { throw 'Provide -NoAppMedianRttMs measured with the same endpoint and baseline route.' }
        if ($Scenario -eq 'Failover' -and (-not $ConfirmUplinkFlap -or -not $AdapterName)) { throw 'Failover requires -ConfirmUplinkFlap and exact -AdapterName; the script will restore it in finally.' }
        for ($rep=1; $rep -le $Repetitions; $rep++) {
            $sample=Invoke-EchoRun $rep -DoFailover:($Scenario -eq 'Failover')
            if ($Scenario -eq 'UdpEcho') {
                $hasEcho=$null -ne $sample.Latency -and $sample.Received -gt 0
                if ($hasEcho) {
                    $added=$sample.Latency.Median-$NoAppMedianRttMs
                    Add-Measurement 'median_added_rtt_ms' $rep $added 'ms' '<=' 3.0 "Observed RTT median=$($sample.Latency.Median); separately measured no-app baseline=$NoAppMedianRttMs ms."
                } else { Add-MissingMeasurement 'median_added_rtt_ms' $rep 'ms' '<=' 3.0 'No valid echo response; latency is not measurable.' }
                Add-Measurement 'delivered_reorder_pct' $rep $sample.ReorderPct '%' '<=' 0.1 "outOfOrder=$($sample.OutOfOrder); received=$($sample.Received); sent=$($sample.Sent); loss=$($sample.LossPct)%"
            } else {
                if ($null -ne $sample.FailoverConvergenceSeconds) {
                    Add-Measurement 'failover_convergence_seconds' $rep $sample.FailoverConvergenceSeconds 's' '<=' 2.0 "remote echo first-success gap after interface-down; adapter=$AdapterName; configured down=$FlapDownSeconds s."
                } else { Add-MissingMeasurement 'failover_convergence_seconds' $rep 's' '<=' 2.0 'No successful remote echo after link flap; failover convergence was not observed.' }
            }
            Add-Observation 'packet_loss_pct' $rep $sample.LossPct '%' "sent=$($sample.Sent); received=$($sample.Received); ledger defines no packet-loss acceptance threshold."
        }
        Add-Case "network-$Scenario" (@($script:Rows | Where-Object Result -eq 'FAIL').Count -eq 0) "Send numbered UDP echo probes to $UdpEchoServer`:$UdpEchoPort repeated $Repetitions times." 'Raw CSV, echo endpoint logs, and independently captured route/driver/relay details retained.' ("Rows=$($script:Rows.Count); CSV=$csvPath")
    } elseif ($Scenario -eq 'CleanBefore' -or $Scenario -eq 'CleanAfter') {
        $snapshot=Get-EnvironmentSnapshot
        $snapshotPath=Join-Path $OutputDirectory ("clean-machine-{0}.json" -f $Scenario.ToLowerInvariant())
        $snapshot | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $snapshotPath -Encoding UTF8
        $present=@($snapshot.BalancifyAdapters).Count + @($snapshot.BalancifyServices).Count + @($snapshot.BalancifyDrivers).Count + @($snapshot.BalancifyDevices).Count + @($snapshot.BalancifyRoutes).Count
        if ($Scenario -eq 'CleanBefore') {
            Add-Case 'clean-machine-before-install' ($present -eq 0) 'Capture clean pre-install machine snapshot.' 'No BalanciFy service/driver/device/adapter/route present.' "residueObjects=$present; snapshot=$snapshotPath"
        } else {
            if (-not $BaselineSnapshot -or -not (Test-Path $BaselineSnapshot)) { throw 'CleanAfter requires -BaselineSnapshot pointing to clean-machine-cleanbefore.json.' }
            $base=Get-Content -LiteralPath $BaselineSnapshot -Raw | ConvertFrom-Json
            $baseCount=@($base.BalancifyAdapters).Count+@($base.BalancifyServices).Count+@($base.BalancifyDrivers).Count+@($base.BalancifyDevices).Count+@($base.BalancifyRoutes).Count
            $passed=($present -eq $baseCount -and $present -eq 0)
            Add-Case 'clean-machine-after-uninstall' $passed 'Compare clean pre-install snapshot with post-uninstall snapshot.' 'No BalanciFy service/driver/device/adapter/route residue.' "before=$baseCount; after=$present; snapshot=$snapshotPath"
        }
    } elseif ($Scenario -eq 'CompatibilityTemplate') {
        $path=Join-Path $OutputDirectory 'compatibility-checklist.csv'
        if (-not (Test-Path $path)) {
            @(
                [pscustomobject]@{Component='VPN clients';Configuration='Installed and active';Expected='Tunnel routes remain correct; no conflict';Observed='';Result='NOT RUN';Evidence=''}
                [pscustomobject]@{Component='Endpoint security suites';Configuration='Each supported suite';Expected='No block/crash; document exclusions';Observed='';Result='NOT RUN';Evidence=''}
                [pscustomobject]@{Component='Hyper-V';Configuration='Enabled';Expected='No adapter/route conflict';Observed='';Result='NOT RUN';Evidence=''}
                [pscustomobject]@{Component='WSL2';Configuration='Installed/enabled';Expected='No conflict; WSL is not a local build prerequisite';Observed='';Result='NOT RUN';Evidence=''}
                [pscustomobject]@{Component='HVCI';Configuration='Off';Expected='Install, traffic, teardown succeed';Observed='';Result='NOT RUN';Evidence=''}
                [pscustomobject]@{Component='HVCI';Configuration='On';Expected='Install, traffic, teardown succeed';Observed='';Result='NOT RUN';Evidence=''}
                [pscustomobject]@{Component='Sleep/resume';Configuration='100 cycles';Expected='No leaked route/filter/handle; recovery works';Observed='';Result='NOT RUN';Evidence=''}
                [pscustomobject]@{Component='Clean install/uninstall';Configuration='Offline clean Windows VM';Expected='No residue after uninstall';Observed='';Result='NOT RUN';Evidence=''}
            ) | Export-Csv -LiteralPath $path -NoTypeInformation
        }
        Add-Case 'compatibility-template' $true 'Create compatibility-checklist.csv.' 'Every row initialized NOT RUN; operator supplies evidence before changing result.' $path
    } elseif ($Scenario -eq 'SoakMonitor') {
        $start=[DateTime]::Now
        $deadline=$start.AddHours($SoakHours)
        $rows=New-Object System.Collections.ArrayList
        Add-Case 'soak-monitor-started' $true "Observe for $SoakHours hours; manually perform $RequiredResumeCycles sleep/resume cycles." 'No automatic suspend or disruptive action is issued; operator performs cycles in target VM/rig.' "start=$($start.ToString('o')); expectedResumeCycles=$RequiredResumeCycles"
        while ([DateTime]::Now -lt $deadline) {
            $adapters=@(Get-NetAdapter -ErrorAction SilentlyContinue | Where-Object { $_.Name -match 'Balancify' -or $_.InterfaceDescription -match 'Balancify' })
            $svc=@(Get-CimInstance Win32_Service -ErrorAction SilentlyContinue | Where-Object { $_.Name -match 'Balancify' })
            $resume=@(Get-WinEvent -FilterHashtable @{LogName='System';ProviderName='Microsoft-Windows-Power-Troubleshooter';Id=1;StartTime=$start} -ErrorAction SilentlyContinue)
            [void]$rows.Add([pscustomobject]@{TimestampUtc=[DateTime]::UtcNow.ToString('o'); BalanciFyAdapters=$adapters.Count; BalanciFyServices=$svc.Count; ResumeEvents=$resume.Count; Status='OBSERVED'})
            $rows | Export-Csv -LiteralPath $csvPath -NoTypeInformation
            Write-Progress -Activity 'BalanciFy soak monitor' -Status "$([Math]::Round(($deadline-[DateTime]::Now).TotalHours,1)) hours remain; resume events=$($resume.Count)" -PercentComplete ([Math]::Min(100,100*($SoakHours-($deadline-[DateTime]::Now).TotalHours)/$SoakHours))
            Start-Sleep -Seconds 60
        }
        $resumeFinal=@(Get-WinEvent -FilterHashtable @{LogName='System';ProviderName='Microsoft-Windows-Power-Troubleshooter';Id=1;StartTime=$start} -ErrorAction SilentlyContinue)
        $pass=($resumeFinal.Count -ge $RequiredResumeCycles)
        Add-Case 'soak-duration-and-resume-count' $pass "Run monitor for $SoakHours hours; collect Windows resume event 1." "At least $RequiredResumeCycles resumes and full duration; no crash/route/filter leaks must be manually reviewed." "durationHours=$SoakHours; resumeEvents=$($resumeFinal.Count); samplesCsv=$csvPath"
    }
} catch {
    Add-Case "scenario-$Scenario" 'FAIL' $MyInvocation.Line 'Prerequisites met and measurement completes without unsafe fallback.' $_.Exception.Message
}

# Keep observations and target thresholds as separate fields/objects.
Save-Rows
$metrics=@()
foreach($group in @($script:Rows | Group-Object Metric)) {
    $numericRows=@($group.Group | Where-Object { $null -ne $_.MeasuredValue })
    $stats=if($numericRows.Count){Get-Stats ([double[]]@($numericRows | ForEach-Object {[double]$_.MeasuredValue}))}else{$null}
    $target=$group.Group[0]
    $metrics += [pscustomobject]@{
        metric=$group.Name; observed=$stats; target=[pscustomobject]@{ operator=$target.TargetOperator; value=$target.TargetValue; isAcceptanceThreshold=[bool]$target.TargetIsThreshold }
        perRepetitionResults=@($group.Group | Select-Object Repetition,MeasuredValue,Unit,Result)
        aggregateResult=if(@($group.Group | Where-Object Result -eq 'FAIL').Count){'FAIL'}elseif(@($group.Group | Where-Object Result -eq 'NOT ASSESSED').Count){'NOT ASSESSED'}else{'PASS'}
    }
}
$evidence=[pscustomobject]@{
    evidenceType='Rig B / clean-machine measurement evidence; measured data is distinct from target thresholds'
    runId=$script:RunId; scenario=$Scenario; timestampUtc=[DateTime]::UtcNow.ToString('o')
    environment=[pscustomobject]@{computer=$env:COMPUTERNAME; windowsCaption=$os.Caption; windowsVersion=$os.Version; windowsBuild=$os.BuildNumber; architecture=$env:PROCESSOR_ARCHITECTURE; powershellVersion=$PSVersionTable.PSVersion.ToString(); account=[Security.Principal.WindowsIdentity]::GetCurrent().Name; elevatedAdmin=(Test-Admin); adapters=@(Get-NetAdapter -ErrorAction SilentlyContinue | Select-Object Name,InterfaceDescription,InterfaceIndex,Status,LinkSpeed); sourceAddress=$SourceAddress; uplinkBaselineMbps=@($UplinkBaselineMbps); bestUplinkMbps=$BestUplinkMbps; noAppMedianRttMs=$NoAppMedianRttMs}
    repetitionRequirement=[pscustomobject]@{minimumN=10; configuredN=$Repetitions; durationSeconds=$DurationSeconds}
    targetThresholds=[pscustomobject]@{aggregateMbps='>= 0.90 * sum(uplink baselines)'; matchedSingleMbps='>= 0.70 * sum(matched-link baselines)'; mismatchedSingleMbps='>= best individual uplink baseline'; wiredMedianAddedRttMs='<= 3'; deliveredOutOfOrderPercent='<= 0.1'; failoverConvergenceSeconds='<= 2'}
    measurementsCsv=$csvPath; metrics=$metrics; cases=@($script:Cases); transcriptLog=@($script:Log)
    compatibilityChecklist=(Join-Path $OutputDirectory 'compatibility-checklist.csv')
    note='Do not treat a software-only benchmark, incomplete run, or target threshold as measured evidence. CleanMachine snapshots only detect named BalanciFy residue; review all installer/runtime evidence separately.'
}
$evidence | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $summaryPath -Encoding UTF8
Write-Host "Measurements CSV: $csvPath"
Write-Host "Summary JSON: $summaryPath"
if (@($script:Cases | Where-Object result -eq 'FAIL').Count -gt 0 -or @($script:Rows | Where-Object Result -eq 'FAIL').Count -gt 0) { exit 1 }
