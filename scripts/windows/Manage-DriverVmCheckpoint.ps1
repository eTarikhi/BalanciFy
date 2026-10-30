[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Before', 'After', 'RestoreBefore')]
    [string]$Action,
    [Parameter(Mandatory = $true)]
    [string]$VMName,
    [Parameter(Mandatory = $true)]
    [switch]$ConfirmDisposableVM,
    [string]$CheckpointName,
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
if (-not $CheckpointName) {
    $CheckpointName = "BalanciFy-$Action-$stamp"
}
$log = New-Object System.Collections.ArrayList
function Note([string]$Text) {
    $line = '{0:o} {1}' -f [DateTime]::UtcNow, $Text
    [void]$script:log.Add($line)
    Write-Host $Text
}

$case = [pscustomobject]@{ test = "Hyper-V checkpoint $Action"; result = 'FAIL'; command = ''; expected = ''; observed = '' }
try {
    if (-not $ConfirmDisposableVM) { throw 'Safety stop: pass -ConfirmDisposableVM only for a disposable isolated Hyper-V VM.' }
    if (-not (Get-Command Get-VM -ErrorAction SilentlyContinue)) { throw 'Hyper-V PowerShell module/cmdlets unavailable. Run on the Hyper-V host, not inside the guest.' }
    $vm = Get-VM -Name $VMName -ErrorAction Stop
    if ($Action -eq 'Before' -or $Action -eq 'After') {
        $cmd = "Checkpoint-VM -Name '$VMName' -SnapshotName '$CheckpointName'"
        if ($PSCmdlet.ShouldProcess("VM '$VMName'", "Create checkpoint '$CheckpointName'")) {
            Checkpoint-VM -Name $VMName -SnapshotName $CheckpointName -ErrorAction Stop | Out-Null
            $case.result = 'PASS'; $case.command = $cmd
            $case.expected = 'Named checkpoint exists before/after the VM runtime test.'
            $case.observed = "Checkpoint '$CheckpointName' created for VM '$VMName'."
        } else {
            $case.result = 'NOT RUN'; $case.command = $cmd
            $case.expected = 'Checkpoint created only after ShouldProcess confirmation.'
            $case.observed = 'WhatIf/confirmation declined.'
        }
    } else {
        $matches = @(Get-VMSnapshot -VMName $VMName -Name $CheckpointName -ErrorAction SilentlyContinue)
        if ($matches.Count -ne 1) { throw "Expected exactly one checkpoint named '$CheckpointName'; found $($matches.Count)." }
        if ($vm.State -eq 'Running') { throw "Stop VM '$VMName' manually before restore; this script will not stop it for you." }
        $cmd = "Restore-VMSnapshot -VMName '$VMName' -Name '$CheckpointName'"
        if ($PSCmdlet.ShouldProcess("VM '$VMName'", "Restore checkpoint '$CheckpointName'")) {
            Restore-VMSnapshot -VMName $VMName -Name $CheckpointName -ErrorAction Stop
            $case.result = 'PASS'; $case.command = $cmd
            $case.expected = 'Disposable VM returns to the selected checkpoint; it is not automatically started.'
            $case.observed = "Restore requested for '$CheckpointName'; start and verify the VM manually."
        } else {
            $case.result = 'NOT RUN'; $case.command = $cmd
            $case.expected = 'Restore only after explicit ShouldProcess confirmation.'
            $case.observed = 'WhatIf/confirmation declined.'
        }
    }
} catch {
    $case.command = "$Action VM '$VMName' checkpoint operation"
    $case.expected = 'Operation runs only on the named disposable Hyper-V VM after confirmation.'
    $case.observed = $_.Exception.Message
    Note ("FAIL: {0}" -f $_.Exception.Message)
}
Note ("{0} [{1}] expected={2}; observed={3}" -f $case.result, $case.test, $case.expected, $case.observed)
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $PSScriptRoot '..\..\target\acceptance-evidence\driver-vm' }
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$evidence = [pscustomobject]@{
    evidenceType = 'Hyper-V checkpoint operation; no driver test execution'
    timestampUtc = [DateTime]::UtcNow.ToString('o'); host = $env:COMPUTERNAME; vmName = $VMName
    action = $Action; checkpointName = $CheckpointName; evidence = $case; transcriptLog = @($log)
}
$path = Join-Path $OutputDirectory ("checkpoint-{0}-{1}.json" -f $Action.ToLowerInvariant(), $stamp)
$evidence | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $path -Encoding UTF8
Write-Host "Evidence JSON: $path"
if ($case.result -eq 'FAIL') { exit 1 }
