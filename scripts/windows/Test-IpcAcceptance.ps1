[CmdletBinding()]
param(
    [ValidateSet('Standard', 'Admin', 'NonInteractiveProbe')]
    [string]$Role = 'Standard',
    [string]$PipeName = 'BalanciFy.Service.v1',
    [ValidateRange(65, 300)]
    [int]$TimeoutSeconds = 75,
    [switch]$AllowPolicyMutation,
    [switch]$IncludeNonInteractiveProbe,
    [switch]$SkipTransportCases,
    [string]$OutputDirectory,
    [string]$ProbeResultPath
)

# Native Windows PowerShell 5.1; does not require WSL. This script only speaks
# the local IPC protocol. It does not install or control the driver.
$ErrorActionPreference = 'Stop'
$script:Cases = New-Object System.Collections.ArrayList
$script:Transcript = New-Object System.Collections.ArrayList
$script:RunId = [Guid]::NewGuid().ToString('N')
$script:Started = [DateTime]::UtcNow
$script:RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Add-Log([string]$Message) {
    $line = '{0:o} {1}' -f [DateTime]::UtcNow, $Message
    [void]$script:Transcript.Add($line)
    Write-Host $Message
}

function Add-Case([string]$Name, [bool]$Passed, [string]$Command, [string]$Expected, [string]$Observed) {
    $status = if ($Passed) { 'PASS' } else { 'FAIL' }
    [void]$script:Cases.Add([pscustomobject]@{
        case = $Name; result = $status; command = $Command
        expected = $Expected; observed = $Observed
    })
    Add-Log ("{0} [{1}] expected={2}; observed={3}" -f $status, $Name, $Expected, $Observed)
}

function New-IpcFrame([byte[]]$Payload) {
    $frame = New-Object byte[] (6 + $Payload.Length)
    $frame[0] = 1; $frame[1] = 1
    $length = [BitConverter]::GetBytes([UInt32]$Payload.Length)
    if ([BitConverter]::IsLittleEndian) { [Array]::Reverse($length) }
    [Array]::Copy($length, 0, $frame, 2, 4)
    [Array]::Copy($Payload, 0, $frame, 6, $Payload.Length)
    return ,$frame
}

function Connect-Ipc {
    $pipe = [System.IO.Pipes.NamedPipeClientStream]::new(
        '.', $PipeName, [System.IO.Pipes.PipeDirection]::InOut,
        [System.IO.Pipes.PipeOptions]::None,
        [System.Security.Principal.TokenImpersonationLevel]::Impersonation)
    $pipe.Connect(5000)
    $pipe.ReadTimeout = [Math]::Max(5000, $TimeoutSeconds * 1000)
    $pipe.WriteTimeout = 5000
    return $pipe
}

function Read-Exact([System.IO.Stream]$Stream, [int]$Count) {
    $buffer = New-Object byte[] $Count
    $offset = 0
    while ($offset -lt $Count) {
        $read = $Stream.Read($buffer, $offset, $Count - $offset)
        if ($read -le 0) { throw 'IPC peer closed before the full frame arrived.' }
        $offset += $read
    }
    return ,$buffer
}

function Read-IpcResponse([System.IO.Stream]$Stream) {
    $header = Read-Exact $Stream 6
    if ($header[0] -ne 1 -or $header[1] -ne 2) { throw "Unexpected response frame version/kind: $($header[0])/$($header[1])." }
    $lengthBytes = $header[2..5]
    if ([BitConverter]::IsLittleEndian) { [Array]::Reverse($lengthBytes) }
    $length = [BitConverter]::ToUInt32($lengthBytes, 0)
    if ($length -gt 1048576) { throw "IPC response too large: $length bytes." }
    $payload = Read-Exact $Stream ([int]$length)
    $status = $null
    if ($payload.Length -ge 2 -and $payload[0] -eq 7) { $status = [int]$payload[1] }
    return [pscustomobject]@{ Tag = [int]$payload[0]; Status = $status; Payload = $payload }
}

function Send-IpcRequest([byte[]]$Payload) {
    $pipe = Connect-Ipc
    try {
        $frame = New-IpcFrame $Payload
        $pipe.Write($frame, 0, $frame.Length)
        $pipe.Flush()
        return Read-IpcResponse $pipe
    } finally { $pipe.Dispose() }
}

function Read-U32BE([byte[]]$Bytes, [int]$Offset) {
    if ($Offset -lt 0 -or $Bytes.Length -lt ($Offset + 4)) { throw 'Truncated 32-bit IPC field.' }
    return [UInt32](([UInt64]$Bytes[$Offset] -shl 24) -bor ([UInt64]$Bytes[$Offset + 1] -shl 16) -bor ([UInt64]$Bytes[$Offset + 2] -shl 8) -bor [UInt64]$Bytes[$Offset + 3])
}

function Get-PolicyFlags {
    $response = Send-IpcRequest ([byte[]]@(1)) # GetStatus
    if ($response.Tag -ne 2) { throw "GetStatus did not return Status (response tag $($response.Tag))." }
    $versionLength = [int]$response.Payload[1]
    $flagOffset = 19 + $versionLength
    if ($response.Payload.Length -lt ($flagOffset + 3)) { throw 'Status response was too short to contain machine-policy flags.' }
    return [pscustomobject]@{
        Bond = [bool]($response.Payload[$flagOffset] -ne 0)
        Duplicate = [bool]($response.Payload[$flagOffset + 1] -ne 0)
        AllowMetered = [bool]($response.Payload[$flagOffset + 2] -ne 0)
    }
}

function Get-FlowRuleSnapshot {
    $response = Send-IpcRequest ([byte[]]@(3)) # ListFlowRules
    if ($response.Tag -ne 4) { throw "ListFlowRules did not return FlowRules (response tag $($response.Tag))." }
    $payload = $response.Payload
    $count = [int](Read-U32BE $payload 1)
    if ($count -gt 4096) { throw "Unreasonable flow-rule count: $count." }
    $offset = 5
    $rules = New-Object System.Collections.ArrayList
    for ($index = 0; $index -lt $count; $index++) {
        $id = Read-U32BE $payload $offset; $offset += 4
        if ($offset -ge $payload.Length) { throw 'Truncated flow-rule name.' }
        $nameLength = [int]$payload[$offset]; $offset++
        if ($offset + $nameLength + 12 -gt $payload.Length) { throw 'Truncated flow-rule record.' }
        $name = [Text.Encoding]::UTF8.GetString($payload, $offset, $nameLength); $offset += $nameLength
        $class = [int]$payload[$offset]; $offset++
        $pinned = [int]$payload[$offset]; $offset++
        $priority = Read-U32BE $payload $offset; $offset += 4
        $proto = [int]$payload[$offset]; $offset++
        $portStart = ([UInt16](([UInt16]$payload[$offset] -shl 8) -bor [UInt16]$payload[$offset + 1])); $offset += 2
        $portEnd = ([UInt16](([UInt16]$payload[$offset] -shl 8) -bor [UInt16]$payload[$offset + 1])); $offset += 2
        $duplicate = [bool]($payload[$offset] -ne 0); $offset++
        if ($id -ge 1000) {
            [void]$rules.Add([pscustomobject]@{ Id = [UInt32]$id; Name = $name; Class = $class; Pinned = $pinned; Priority = [UInt32]$priority; Proto = $proto; PortStart = $portStart; PortEnd = $portEnd; Duplicate = $duplicate })
        }
    }
    if ($offset -ne $payload.Length) { throw 'Unexpected trailing bytes in flow-rule response.' }
    return $rules.ToArray()
}

function Add-WireU16([System.Collections.Generic.List[byte]]$Bytes, [UInt16]$Value) {
    $encoded = [BitConverter]::GetBytes($Value)
    if ([BitConverter]::IsLittleEndian) { [Array]::Reverse($encoded) }
    $Bytes.AddRange($encoded)
}

function Add-WireU32([System.Collections.Generic.List[byte]]$Bytes, [UInt32]$Value) {
    $encoded = [BitConverter]::GetBytes($Value)
    if ([BitConverter]::IsLittleEndian) { [Array]::Reverse($encoded) }
    $Bytes.AddRange($encoded)
}

function Add-WireRule([System.Collections.Generic.List[byte]]$Bytes, $Rule) {
    $nameBytes = [Text.Encoding]::UTF8.GetBytes([string]$Rule.Name)
    if ($nameBytes.Length -gt 255) { throw 'Flow-rule name exceeds the IPC string limit.' }
    Add-WireU32 $Bytes ([UInt32]$Rule.Id)
    $Bytes.Add([byte]$nameBytes.Length); $Bytes.AddRange($nameBytes)
    $Bytes.Add([byte]$Rule.Class); $Bytes.Add([byte]$Rule.Pinned)
    Add-WireU32 $Bytes ([UInt32]$Rule.Priority)
    $Bytes.Add([byte]$Rule.Proto)
    Add-WireU16 $Bytes ([UInt16]$Rule.PortStart)
    Add-WireU16 $Bytes ([UInt16]$Rule.PortEnd)
    $Bytes.Add([byte]([int][bool]$Rule.Duplicate))
}

function New-ApplyCurrentPolicyPayload {
    $flags = Get-PolicyFlags
    $rules = @(Get-FlowRuleSnapshot)
    $bytes = New-Object 'System.Collections.Generic.List[byte]'
    $bytes.Add([byte]12); $bytes.Add([byte]([int]$flags.Bond)); $bytes.Add([byte]([int]$flags.Duplicate)); $bytes.Add([byte]([int]$flags.AllowMetered))
    Add-WireU16 $bytes ([UInt16]$rules.Count)
    foreach ($rule in $rules) { Add-WireRule $bytes $rule }
    return ,$bytes.ToArray()
}

function Get-BondState { return (Get-PolicyFlags).Bond }

function Wait-PeerClose([System.IO.Pipes.NamedPipeClientStream]$Pipe, [int]$WaitSeconds) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $Pipe.ReadTimeout = $WaitSeconds * 1000
    try {
        $one = $Pipe.ReadByte()
        if ($one -lt 0) { return [pscustomobject]@{ Closed = $true; Detail = 'EOF from service'; ElapsedSeconds = $clock.Elapsed.TotalSeconds } }
        return [pscustomobject]@{ Closed = $false; Detail = "Received unexpected byte $one from service"; ElapsedSeconds = $clock.Elapsed.TotalSeconds }
    } catch [System.TimeoutException] {
        return [pscustomobject]@{ Closed = $false; Detail = 'Client read timed out before service closed the stream'; ElapsedSeconds = $clock.Elapsed.TotalSeconds }
    } catch [System.IO.IOException] {
        if ($clock.Elapsed.TotalSeconds -lt ($WaitSeconds - 1)) {
            return [pscustomobject]@{ Closed = $true; Detail = $_.Exception.Message; ElapsedSeconds = $clock.Elapsed.TotalSeconds }
        }
        return [pscustomobject]@{ Closed = $false; Detail = 'Read deadline elapsed without confirmed peer close'; ElapsedSeconds = $clock.Elapsed.TotalSeconds }
    }
}

function Invoke-TransportCase([string]$Name, [byte[]]$PartialFrame, [string]$Command) {
    $pipe = $null
    try {
        $pipe = Connect-Ipc
        $hello = New-IpcFrame ([byte[]]@(0)) # Ping authenticates the pipe peer
        $pipe.Write($hello, 0, $hello.Length); $pipe.Flush()
        $helloResponse = Read-IpcResponse $pipe
        if ($helloResponse.Tag -ne 1) { throw 'Initial Ping did not return Pong; transport test is invalid.' }
        $pipe.ReadTimeout = $TimeoutSeconds * 1000
        $pipe.Write($PartialFrame, 0, $PartialFrame.Length); $pipe.Flush()
        $outcome = Wait-PeerClose $pipe $TimeoutSeconds
        $timely = ($outcome.Closed -and $outcome.ElapsedSeconds -ge 45 -and $outcome.ElapsedSeconds -le $TimeoutSeconds)
        Add-Case $Name $timely $Command "Service closes the incomplete/stalled frame after the 60-second server deadline (45-second lower tolerance) and within $TimeoutSeconds seconds." ("$($outcome.Detail); elapsedSeconds=$([Math]::Round($outcome.ElapsedSeconds,3))")
    } catch {
        Add-Case $Name $false $Command 'A valid Ping succeeds, then service closes the incomplete frame within the configured deadline.' $_.Exception.Message
    } finally { if ($null -ne $pipe) { $pipe.Dispose() } }
}

function Invoke-IdleCase {
    $pipe = $null
    try {
        $pipe = Connect-Ipc
        $frame = New-IpcFrame ([byte[]]@(1))
        $pipe.Write($frame, 0, $frame.Length); $pipe.Flush()
        $response = Read-IpcResponse $pipe
        if ($response.Tag -ne 2) { throw 'GetStatus did not return a Status response.' }
        $outcome = Wait-PeerClose $pipe $TimeoutSeconds
        $timely = ($outcome.Closed -and $outcome.ElapsedSeconds -ge 45 -and $outcome.ElapsedSeconds -le $TimeoutSeconds)
        Add-Case 'idle-connection-timeout' $timely "Connect named pipe; GetStatus; remain idle for up to $TimeoutSeconds seconds." 'After a valid request with no event subscription, service closes idle connection at the 60-second limit (45-second lower tolerance).' ("$($outcome.Detail); elapsedSeconds=$([Math]::Round($outcome.ElapsedSeconds,3))")
    } catch {
        Add-Case 'idle-connection-timeout' $false 'Connect named pipe; GetStatus; remain idle.' 'Service closes idle connection after the configured idle deadline.' $_.Exception.Message
    } finally { if ($null -ne $pipe) { $pipe.Dispose() } }
}

function Invoke-NonInteractiveProbeChild([string]$ResultPath) {
    $session = [Diagnostics.Process]::GetCurrentProcess().SessionId
    $observation = 'No response received.'
    $rejected = $false
    $pipe = $null
    try {
        $pipe = [System.IO.Pipes.NamedPipeClientStream]::new(
            '.', $PipeName, [System.IO.Pipes.PipeDirection]::InOut,
            [System.IO.Pipes.PipeOptions]::None,
            [System.Security.Principal.TokenImpersonationLevel]::Impersonation)
        $pipe.Connect(5000)
        $pipe.ReadTimeout = 5000
        $pipe.WriteTimeout = 5000
        $ping = New-IpcFrame ([byte[]]@(0))
        $pipe.Write($ping, 0, $ping.Length); $pipe.Flush()
        $reply = Read-IpcResponse $pipe
        $rejected = ($reply.Tag -ne 1)
        $observation = "Received response tag $($reply.Tag) in session $session."
        if (-not $rejected) { $observation = "Unexpected Pong accepted the session-0 peer (session $session)." }
    } catch [System.TimeoutException] {
        $observation = "Timed out waiting for service rejection in session $session."
        $rejected = $false
    } catch [System.UnauthorizedAccessException] {
        $observation = "OS/pipe ACL rejected the client before service dispatch in session $session: $($_.Exception.Message)"
        $rejected = $true
    } catch [System.IO.IOException] {
        $observation = "Pipe closed/rejected the client in session $session: $($_.Exception.Message)"
        $rejected = ($session -eq 0)
    } finally { if ($null -ne $pipe) { $pipe.Dispose() } }
    $result = [pscustomobject]@{ sessionId = $session; rejected = $rejected; observed = $observation }
    $result | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $ResultPath -Encoding UTF8
    if ($rejected) { exit 0 } else { exit 2 }
}

function Invoke-NonInteractiveCase {
    if (-not (Test-Path (Join-Path $env:WINDIR 'System32\Tasks'))) {
        Add-Case 'non-interactive-client-rejected' $false 'Run the probe as SYSTEM in a scheduled task (session 0).' 'Session-0 client is rejected by the service.' 'Task Scheduler service path not available.'
        return
    }
    $guid = [Guid]::NewGuid().ToString('N')
    $taskName = "BalanciFy-IpcProbe-$guid"
    $tempRoot = Join-Path $env:ProgramData "BalanciFy\IpcProbe-$guid"
    $probeFile = Join-Path $tempRoot 'probe.json'
    $scriptPath = Join-Path $PSScriptRoot 'Test-IpcAcceptance.ps1'
    try {
        New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
        $acl = Get-Acl $tempRoot
        $rule = New-Object System.Security.AccessControl.FileSystemAccessRule(
            'SYSTEM', 'Modify', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
        $acl.SetAccessRule($rule); Set-Acl -LiteralPath $tempRoot -AclObject $acl
        $arguments = "-NoProfile -ExecutionPolicy Bypass -File `"$scriptPath`" -Role NonInteractiveProbe -ProbeResultPath `"$probeFile`" -PipeName `"$PipeName`""
        $action = New-ScheduledTaskAction -Execute (Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe') -Argument $arguments
        $trigger = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(10)
        $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
        Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Principal $principal -Force | Out-Null
        Start-ScheduledTask -TaskName $taskName
        $deadline = [DateTime]::UtcNow.AddSeconds(30)
        while (-not (Test-Path -LiteralPath $probeFile) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 250 }
        if (-not (Test-Path -LiteralPath $probeFile)) { throw 'Session-0 probe task did not produce a result within 30 seconds.' }
        $probe = Get-Content -LiteralPath $probeFile -Raw | ConvertFrom-Json
        $passed = ($probe.sessionId -eq 0 -and [bool]$probe.rejected)
        Add-Case 'non-interactive-client-rejected' $passed "Start transient SYSTEM scheduled task in session 0; send IPC Ping." 'Peer is rejected; no Pong is returned and session ID is 0.' $probe.observed
    } catch {
        Add-Case 'non-interactive-client-rejected' $false 'Start transient SYSTEM scheduled task in session 0; send IPC Ping.' 'Peer is rejected; no Pong is returned and session ID is 0.' $_.Exception.Message
    } finally {
        Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

if ($Role -eq 'NonInteractiveProbe') {
    if ([string]::IsNullOrWhiteSpace($ProbeResultPath)) { throw '-ProbeResultPath is required for the internal non-interactive probe.' }
    Invoke-NonInteractiveProbeChild $ProbeResultPath
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
$isAdmin = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$sessionId = [Diagnostics.Process]::GetCurrentProcess().SessionId
$expectedAdmin = ($Role -eq 'Admin')
$contextOkay = if ($Role -eq 'Admin') { $isAdmin -and $sessionId -ne 0 } else { (-not $isAdmin) -and $sessionId -ne 0 }
$os = Get-CimInstance Win32_OperatingSystem
$runContext = [pscustomobject]@{
    windowsCaption = $os.Caption; windowsVersion = $os.Version; windowsBuild = $os.BuildNumber
    account = $identity.Name; accountSid = $identity.User.Value; roleRequested = $Role
    isElevatedAdministrator = $isAdmin; sessionId = $sessionId; pipeName = $PipeName
}
Add-Case 'execution-context' $contextOkay "Run under requested role '$Role'." ("interactive session; admin=$expectedAdmin") ("account=$($identity.Name); elevatedAdmin=$isAdmin; session=$sessionId")

if ($contextOkay) {
    try {
        $ping = Send-IpcRequest ([byte[]]@(0))
        Add-Case 'permitted-ping' ($ping.Tag -eq 1) 'IPC Ping (opcode 0).' 'Pong response (tag 1).' "responseTag=$($ping.Tag); errorStatus=$($ping.Status)"
    } catch { Add-Case 'permitted-ping' $false 'IPC Ping (opcode 0).' 'Pong response (tag 1).' $_.Exception.Message }

    try {
        $status = Send-IpcRequest ([byte[]]@(1))
        Add-Case 'permitted-get-status' ($status.Tag -eq 2) 'IPC GetStatus (opcode 1).' 'Status response (tag 2).' "responseTag=$($status.Tag); errorStatus=$($status.Status)"
    } catch { Add-Case 'permitted-get-status' $false 'IPC GetStatus (opcode 1).' 'Status response (tag 2).' $_.Exception.Message }

    try {
        $oldFlags = Get-PolicyFlags
        if ($Role -eq 'Standard') {
            $ruleId = [UInt32]4000000000
            $ruleName = 'IPC-authorization-probe'
            $ruleNameBytes = [Text.Encoding]::UTF8.GetBytes($ruleName)
            $setRule = New-Object 'System.Collections.Generic.List[byte]'
            $setRule.Add([byte]4); Add-WireU32 $setRule $ruleId
            $setRule.Add([byte]$ruleNameBytes.Length); $setRule.AddRange($ruleNameBytes)
            $setRule.AddRange([byte[]]@(0, 0)); Add-WireU32 $setRule ([UInt32]0)
            $setRule.Add([byte]0); Add-WireU16 $setRule ([UInt16]0); Add-WireU16 $setRule ([UInt16]0); $setRule.Add([byte]0)
            $requests = @(
                [pscustomobject]@{ Name = 'SetFlowRule'; Opcode = 4; Payload = $setRule.ToArray() },
                [pscustomobject]@{ Name = 'RemoveFlowRule'; Opcode = 5; Payload = [byte[]]@(5, 0, 0, 3, 232) },
                [pscustomobject]@{ Name = 'SetBondMode'; Opcode = 6; Payload = [byte[]]@(6, [byte]([int](-not $oldFlags.Bond))) },
                [pscustomobject]@{ Name = 'SetDuplicateMode'; Opcode = 7; Payload = [byte[]]@(7, [byte]([int](-not $oldFlags.Duplicate))) },
                [pscustomobject]@{ Name = 'SetAllowMetered'; Opcode = 8; Payload = [byte[]]@(8, [byte]([int](-not $oldFlags.AllowMetered))) },
                [pscustomobject]@{ Name = 'ApplyMachinePolicy'; Opcode = 12; Payload = [byte[]]@(12, [byte]([int](-not $oldFlags.Bond)), [byte]([int](-not $oldFlags.Duplicate)), [byte]([int](-not $oldFlags.AllowMetered)), 0, 0) }
            )
            foreach ($request in $requests) {
                $mutation = Send-IpcRequest ([byte[]]$request.Payload)
                $denied = ($mutation.Tag -eq 7 -and $mutation.Status -eq 5)
                Add-Case ("standard-denied-{0}" -f $request.Name) $denied ("{0}, opcode {1}." -f $request.Name, $request.Opcode) 'Error tag 7 with PermissionDenied status 5.' "responseTag=$($mutation.Tag); status=$($mutation.Status)"
            }
            $afterFlags = Get-PolicyFlags
            $sameFlags = ($afterFlags.Bond -eq $oldFlags.Bond -and $afterFlags.Duplicate -eq $oldFlags.Duplicate -and $afterFlags.AllowMetered -eq $oldFlags.AllowMetered)
            Add-Case 'standard-policy-unchanged' $sameFlags 'GetStatus after all denied machine-policy requests.' "Bond=$($oldFlags.Bond), Duplicate=$($oldFlags.Duplicate), AllowMetered=$($oldFlags.AllowMetered)." "Bond=$($afterFlags.Bond), Duplicate=$($afterFlags.Duplicate), AllowMetered=$($afterFlags.AllowMetered)"
        } elseif ($AllowPolicyMutation) {
            $flagTests = @(
                [pscustomobject]@{ Name = 'Bond'; Opcode = 6; Old = $oldFlags.Bond },
                [pscustomobject]@{ Name = 'Duplicate'; Opcode = 7; Old = $oldFlags.Duplicate },
                [pscustomobject]@{ Name = 'AllowMetered'; Opcode = 8; Old = $oldFlags.AllowMetered }
            )
            foreach ($test in $flagTests) {
                $toggle = -not [bool]$test.Old
                $changed = $false
                try {
                    $mutation = Send-IpcRequest ([byte[]]@($test.Opcode, [byte]([int]$toggle)))
                    $changed = ($mutation.Tag -eq 5)
                    Add-Case ("admin-{0}-write-allowed" -f $test.Name) $changed ("{0}($toggle), opcode {1}." -f $test.Name, $test.Opcode) 'Ack response (tag 5).' "responseTag=$($mutation.Tag); status=$($mutation.Status)"
                    $current = Get-PolicyFlags
                    $observed = [bool]$current.PSObject.Properties[$test.Name].Value
                    Add-Case ("admin-{0}-write-applied" -f $test.Name) ($changed -and $observed -eq $toggle) 'GetStatus after allowed policy mutation.' "$($test.Name)=$toggle." "$($test.Name)=$observed"
                } finally {
                    if ($changed) {
                        $restore = Send-IpcRequest ([byte[]]@($test.Opcode, [byte]([int]$test.Old)))
                        $restoredFlags = Get-PolicyFlags
                        $restoredValue = [bool]$restoredFlags.PSObject.Properties[$test.Name].Value
                        Add-Case ("admin-{0}-restored" -f $test.Name) ($restore.Tag -eq 5 -and $restoredValue -eq $test.Old) ("Restore {0}={1}, opcode {2}." -f $test.Name, $test.Old, $test.Opcode) 'Ack and original value restored.' "responseTag=$($restore.Tag); $($test.Name)=$restoredValue"
                    }
                }
            }
            $added = $false; $removed = $false; $temporaryId = [UInt32]0
            try {
                $beforeRules = @(Get-FlowRuleSnapshot)
                $occupied = @($beforeRules | ForEach-Object { [UInt32]$_.Id })
                do { $temporaryId = [UInt32](3000000000 + (Get-Random -Minimum 0 -Maximum 1000000000)) } while ($occupied -contains $temporaryId)
                $temporaryName = "IPC-acceptance-$($script:RunId.Substring(0, 8))"
                $testRule = [pscustomobject]@{ Id = $temporaryId; Name = $temporaryName; Class = 0; Pinned = 0; Priority = 0; Proto = 0; PortStart = 0; PortEnd = 0; Duplicate = $false }
                $setBytes = New-Object 'System.Collections.Generic.List[byte]'
                $setBytes.Add([byte]4); Add-WireRule $setBytes $testRule
                $add = Send-IpcRequest ($setBytes.ToArray())
                $added = ($add.Tag -eq 5)
                Add-Case 'admin-set-flow-rule-allowed' $added "SetFlowRule id $temporaryId (opcode 4)." 'Ack; temporary rule is added.' "responseTag=$($add.Tag); status=$($add.Status)"
                if ($added) {
                    $removeBytes = New-Object 'System.Collections.Generic.List[byte]'
                    $removeBytes.Add([byte]5); Add-WireU32 $removeBytes $temporaryId
                    $remove = Send-IpcRequest ($removeBytes.ToArray())
                    $removed = ($remove.Tag -eq 5)
                    Add-Case 'admin-remove-flow-rule-allowed' $removed "RemoveFlowRule id $temporaryId (opcode 5)." 'Ack; temporary rule is removed.' "responseTag=$($remove.Tag); status=$($remove.Status)"
                }
            } catch { Add-Case 'admin-flow-rule-mutations' $false 'Add then remove a uniquely identified temporary custom rule.' 'Both operations ACK; temporary rule absent at end.' $_.Exception.Message }
            finally {
                if ($added -and -not $removed) {
                    try {
                        $cleanupBytes = New-Object 'System.Collections.Generic.List[byte]'
                        $cleanupBytes.Add([byte]5); Add-WireU32 $cleanupBytes $temporaryId
                        $cleanupResponse = Send-IpcRequest ($cleanupBytes.ToArray())
                        $verifyRules = @(Get-FlowRuleSnapshot)
                        $stillExists = @($verifyRules | Where-Object { $_.Id -eq $temporaryId }).Count -gt 0
                        Add-Case 'admin-temporary-rule-cleanup' (-not $stillExists) 'Retry removal and verify ListFlowRules.' 'Temporary probe rule is absent at end.' "cleanupResponseTag=$($cleanupResponse.Tag); temporaryRuleStillExists=$stillExists"
                    } catch { Add-Case 'admin-temporary-rule-cleanup' $false 'Retry temporary rule removal and verify ListFlowRules.' 'Temporary probe rule is absent at end.' $_.Exception.Message }
                }
            }

            try {
                $currentPolicy = New-ApplyCurrentPolicyPayload
                $apply = Send-IpcRequest $currentPolicy
                Add-Case 'admin-apply-machine-policy-allowed' ($apply.Tag -eq 5) 'ApplyMachinePolicy with the exact current flags and custom rules (no-op policy import).' 'Ack response (tag 5); existing machine policy is preserved.' "responseTag=$($apply.Tag); status=$($apply.Status)"
            } catch { Add-Case 'admin-apply-machine-policy-allowed' $false 'Apply current machine policy without changing its values.' 'Ack response (tag 5); existing policy preserved.' $_.Exception.Message }
        } else {
            Add-Case 'admin-policy-writes-allowed' $false 'Run the guarded mutation probes.' 'Run with -AllowPolicyMutation in a disposable VM; all values/rules are restored.' 'NOT RUN: explicit -AllowPolicyMutation was not supplied.'
        }
    } catch { Add-Case 'machine-policy-write' $false 'Read machine policy; send authorized/unauthorized IPC mutations; verify state.' 'Standard: all machine-policy mutation opcodes denied; Admin: ACK and original policy restored.' $_.Exception.Message }

    if (-not $SkipTransportCases) {
        Invoke-IdleCase
        # Valid Ping frame is 7 bytes. Header-only prefix exercises partial header;
        # a full header declaring one byte without the byte exercises stalled body.
        Invoke-TransportCase 'partial-ipc-header-timeout' ([byte[]]@(1, 1)) 'Send first 2 bytes of a Ping frame and hold the connection.'
        Invoke-TransportCase 'stalled-ipc-payload-timeout' ([byte[]]@(1, 1, 0, 0, 0, 1)) 'Send a request header declaring 1 payload byte, then stall.'
    }

    if ($IncludeNonInteractiveProbe) {
        if (-not $isAdmin) {
            Add-Case 'non-interactive-client-rejected' $false 'Create transient SYSTEM scheduled task.' 'Session-0 peer rejected.' 'Run the parent script elevated to create the SYSTEM task.'
        } else { Invoke-NonInteractiveCase }
    }
}

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $script:RepoRoot 'target\acceptance-evidence\ipc'
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$evidencePath = Join-Path $OutputDirectory ("ipc-{0}-{1}.json" -f $Role.ToLowerInvariant(), $script:RunId)
$evidence = [pscustomobject]@{
    evidenceType = 'IPC Windows VM integration test; not driver runtime acceptance'
    runId = $script:RunId; startedUtc = $script:Started.ToString('o'); finishedUtc = [DateTime]::UtcNow.ToString('o')
    context = $runContext; command = $MyInvocation.Line
    cases = @($script:Cases); transcriptLog = @($script:Transcript)
    summary = [pscustomobject]@{ passed = @($script:Cases | Where-Object result -eq 'PASS').Count; failed = @($script:Cases | Where-Object result -eq 'FAIL').Count; evidenceFile = $evidencePath }
}
$evidence | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $evidencePath -Encoding UTF8
Add-Log "Evidence JSON (structured cases + transcript log): $evidencePath"
if (@($script:Cases | Where-Object result -eq 'FAIL').Count -gt 0) { exit 1 }
exit 0
