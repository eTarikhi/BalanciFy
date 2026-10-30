[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Preflight','EnableTestSigning','Install','PacketPath','UnloadUnderLoad','VerifierConfigure','VerifierCollect','HVCIState','Uninstall','Rollback')]
    [string]$Stage,
    [switch]$ConfirmDisposableVM,
    [switch]$EnableTestSigning,
    [switch]$ApplyVerifier,
    [switch]$ConfirmUnloadUnderLoad,
    [string]$PackageInf,
    [string]$PublishedInf,
    [string]$PnpInstanceId,
    [string]$ProbeExe,
    [string[]]$ProbeArguments = @(),
    [string]$ExpectedMarker,
    [string]$OutputDirectory
)

# Run only inside a disposable Hyper-V guest. No stage runs automatically.
# Driver operations require -ConfirmDisposableVM; test signing, Verifier, and
# unload-under-load additionally require their dedicated opt-in switch.
$ErrorActionPreference = 'Stop'
$script:Cases = New-Object System.Collections.ArrayList
$script:Log = New-Object System.Collections.ArrayList
$system = Get-CimInstance Win32_ComputerSystem
$os = Get-CimInstance Win32_OperatingSystem
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$admin = (New-Object Security.Principal.WindowsPrincipal($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$hyperVGuest = ($system.Manufacturer -match 'Microsoft' -and $system.Model -match 'Virtual Machine')

function Add-Log([string]$Message) {
    $line = '{0:o} {1}' -f [DateTime]::UtcNow, $Message
    [void]$script:Log.Add($line); Write-Host $Message
}
function Add-Case([string]$Id,[bool]$Passed,[string]$Command,[string]$Expected,[string]$Observed) {
    $result = if ($Passed) { 'PASS' } else { 'FAIL' }
    [void]$script:Cases.Add([pscustomobject]@{ id=$Id; result=$result; command=$Command; expected=$Expected; observed=$Observed })
    Add-Log ("{0} [{1}] expected={2}; observed={3}" -f $result,$Id,$Expected,$Observed)
}
function Run-Native([string]$File,[string[]]$Arguments) {
    $output = @(& $File @Arguments 2>&1 | ForEach-Object { "$_" })
    $exitCode = $LASTEXITCODE
    return [pscustomobject]@{ ExitCode=$exitCode; Output=($output -join "`n") }
}
function Require-DestructiveSafety([string]$Operation) {
    if (-not $ConfirmDisposableVM) { throw "$Operation refused: pass -ConfirmDisposableVM only inside a disposable isolated Hyper-V guest." }
    if (-not $hyperVGuest) { throw "$Operation refused: this system does not identify as a Microsoft Hyper-V Virtual Machine (manufacturer='$($system.Manufacturer)', model='$($system.Model)')." }
    if (-not $admin) { throw "$Operation requires elevated Administrator PowerShell inside the guest." }
}

try {
    Add-Case 'vm-identity' $hyperVGuest 'Get-CimInstance Win32_ComputerSystem' 'Manufacturer Microsoft; Model Virtual Machine.' "Manufacturer=$($system.Manufacturer); Model=$($system.Model)"
    Add-Case 'elevated-context' $admin 'WindowsPrincipal.IsInRole(Administrator)' 'Elevated Administrator inside disposable guest.' "account=$($identity.Name); admin=$admin"

    switch ($Stage) {
        'Preflight' {
            Add-Case 'preflight-only' $true 'Read OS, VM, test-signing state, driver package state, Verifier state, HVCI state.' 'No changes made.' 'Read-only preflight; use other explicit stages to make changes.'
        }
        'EnableTestSigning' {
            Require-DestructiveSafety 'Test-signing setup'
            if (-not $EnableTestSigning) { throw 'Pass -EnableTestSigning to confirm this deliberate boot-configuration change.' }
            $before = (& bcdedit.exe /enum '{current}' 2>&1 | Out-String)
            $command = 'bcdedit.exe /set testsigning on'
            $result = Run-Native 'bcdedit.exe' @('/set','testsigning','on')
            Add-Case 'test-signing-enable' ($result.ExitCode -eq 0) $command 'Exit 0; test-signing is enabled in the guest boot configuration; reboot required.' $result.Output
            Add-Log ("Previous boot entry captured: {0}" -f ($before -replace '[\r\n]+',' '))
        }
        'Install' {
            Require-DestructiveSafety 'Driver package install'
            if (-not $PackageInf -or -not (Test-Path -LiteralPath $PackageInf -PathType Leaf)) { throw 'Supply an existing, test-signed package INF through -PackageInf. This script does not build or sign packages.' }
            $packageDir = Split-Path -Parent (Resolve-Path -LiteralPath $PackageInf).Path
            foreach ($leaf in @('balancify.sys','balancify.cat')) {
                if (-not (Test-Path -LiteralPath (Join-Path $packageDir $leaf) -PathType Leaf)) { throw "Package missing $leaf; use a complete signed package." }
            }
            $sig = Get-AuthenticodeSignature -LiteralPath (Join-Path $packageDir 'balancify.cat')
            if ($sig.Status -ne 'Valid') { throw "Catalog signature is not Valid ($($sig.Status)); do not bypass package verification." }
            $cmd = "pnputil.exe /add-driver `"$PackageInf`" /install"
            $result = Run-Native 'pnputil.exe' @('/add-driver', $PackageInf, '/install')
            Add-Case 'driver-package-install' ($result.ExitCode -eq 0) $cmd 'PNPUtil exit 0 and package staged/installed; then verify device/driver status.' $result.Output
            $svc = Run-Native 'sc.exe' @('query','Balancify')
            Add-Case 'driver-service-query' ($svc.ExitCode -eq 0) 'sc.exe query Balancify' 'Driver service exists; RUNNING/STOPPED state captured for review.' $svc.Output
        }
        'PacketPath' {
            Require-DestructiveSafety 'Packet-path probe'
            if (-not $ProbeExe -or -not (Test-Path -LiteralPath $ProbeExe -PathType Leaf)) { throw 'Supply a trusted local probe executable using -ProbeExe.' }
            if (-not $ExpectedMarker) { throw 'Supply -ExpectedMarker so a zero exit alone cannot be mistaken for packet delivery.' }
            $result = Run-Native $ProbeExe $ProbeArguments
            $observed = "exit=$($result.ExitCode); output=$($result.Output)"
            $passed = ($result.ExitCode -eq 0 -and $result.Output.Contains($ExpectedMarker))
            Add-Case 'real-packet-path' $passed ("& `"$ProbeExe`" " + ($ProbeArguments -join ' ')) "Exit 0 and output contains marker '$ExpectedMarker'; retain remote peer/capture evidence." $observed
        }
        'UnloadUnderLoad' {
            Require-DestructiveSafety 'Unload-under-load test'
            if (-not $ConfirmUnloadUnderLoad) { throw 'Pass -ConfirmUnloadUnderLoad only after a VM checkpoint and with a disposable guest.' }
            if (-not $PnpInstanceId) { throw 'Supply the exact -PnpInstanceId of the BalanciFy virtual adapter.' }
            if (-not $ProbeExe -or -not (Test-Path -LiteralPath $ProbeExe -PathType Leaf)) { throw 'Supply a trusted packet-load probe executable using -ProbeExe.' }
            $stdout = Join-Path $env:TEMP 'balancify-load.stdout.txt'; $stderr = Join-Path $env:TEMP 'balancify-load.stderr.txt'
            $args = if ($ProbeArguments.Count) { $ProbeArguments } else { @() }
            $proc = Start-Process -FilePath $ProbeExe -ArgumentList $args -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
            Start-Sleep -Seconds 5
            $cmd = "pnputil.exe /remove-device `"$PnpInstanceId`""
            $remove = Run-Native 'pnputil.exe' @('/remove-device', $PnpInstanceId)
            $procExited = $proc.WaitForExit(120000)
            if (-not $procExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
            $finalDevice = Get-PnpDevice -InstanceId $PnpInstanceId -ErrorAction SilentlyContinue
            $observed = "removeExit=$($remove.ExitCode); probeExited=$procExited; probeExit=$($proc.ExitCode); devicePresent=$($null -ne $finalDevice); output=$(Get-Content $stdout,$stderr -ErrorAction SilentlyContinue | Out-String)"
            $passed = ($remove.ExitCode -eq 0 -and $procExited -and $null -eq $finalDevice)
            Add-Case 'teardown-unload-under-load' $passed $cmd 'Device removal completes; load process terminates within 120s; device no longer enumerated; inspect event logs and VM after reboot for bugcheck/leaks.' $observed
        }
        'VerifierConfigure' {
            Require-DestructiveSafety 'Driver Verifier setup'
            if (-not $ApplyVerifier) { throw 'Pass -ApplyVerifier to opt in. Verifier changes the guest boot configuration and requires reboot.' }
            $cmd = 'verifier.exe /flags 0x209BB /driver balancify.sys'
            $result = Run-Native 'verifier.exe' @('/flags','0x209BB','/driver','balancify.sys')
            Add-Case 'verifier-configure' ($result.ExitCode -eq 0) $cmd 'Exit 0; settings apply on reboot. Keep the pre-test Hyper-V checkpoint.' $result.Output
            $query = Run-Native 'verifier.exe' @('/querysettings')
            Add-Case 'verifier-querysettings' ($query.ExitCode -eq 0) 'verifier.exe /querysettings' 'Output includes balancify.sys and requested verifier checks.' $query.Output
        }
        'VerifierCollect' {
            $result = Run-Native 'verifier.exe' @('/querysettings')
            Add-Case 'verifier-settings-captured' ($result.ExitCode -eq 0) 'verifier.exe /querysettings' 'Verifier settings captured; expected 0x209BB for balancify.sys when configured.' $result.Output
            $crashes = Get-WinEvent -FilterHashtable @{ LogName='System'; Id=1001 } -MaxEvents 50 -ErrorAction SilentlyContinue | Where-Object { $_.ProviderName -match 'BugCheck|WER' }
            Add-Case 'bugcheck-event-review' ($crashes.Count -eq 0) 'Get-WinEvent System Id 1001 (last 50)' 'No new bugcheck events during the recorded interval; correlate timestamps before accepting.' ("matchingEvents=$($crashes.Count)")
        }
        'HVCIState' {
            $dg = Get-CimInstance -Namespace 'root\Microsoft\Windows\DeviceGuard' -ClassName Win32_DeviceGuard -ErrorAction SilentlyContinue
            $registry = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity' -Name Enabled -ErrorAction SilentlyContinue
            $hvci = if ($null -ne $dg) { [bool](@($dg.SecurityServicesRunning) -contains 2) } else { $null }
            $state = [pscustomobject]@{ VbsStatus=$dg.VirtualizationBasedSecurityStatus; SecurityServicesConfigured=@($dg.SecurityServicesConfigured); SecurityServicesRunning=@($dg.SecurityServicesRunning); HvciRegistryEnabled=$registry.Enabled; HvciReportedRunning=$hvci }
            Add-Case 'hvci-state-captured' ($null -ne $dg) 'Get-CimInstance Win32_DeviceGuard + HVCI registry read' 'Capture raw values separately for HVCI-off and HVCI-on boots; expected running service ID 2 when on.' ($state | ConvertTo-Json -Compress -Depth 5)
        }
        'Uninstall' {
            Require-DestructiveSafety 'Driver package uninstall'
            if (-not $PublishedInf -or $PublishedInf -notmatch '^oem\d+\.inf$') { throw 'Supply the exact published name such as oem42.inf using -PublishedInf; never use a wildcard.' }
            if ($PnpInstanceId) {
                $removeDevice = Run-Native 'pnputil.exe' @('/remove-device', $PnpInstanceId)
                Add-Case 'device-remove' ($removeDevice.ExitCode -eq 0) "pnputil.exe /remove-device `"$PnpInstanceId`"" 'Target VM device removed.' $removeDevice.Output
            }
            $result = Run-Native 'pnputil.exe' @('/delete-driver', $PublishedInf, '/uninstall')
            Add-Case 'driver-package-uninstall' ($result.ExitCode -eq 0) "pnputil.exe /delete-driver $PublishedInf /uninstall" 'Package removed without /force; no active device/package remains.' $result.Output
        }
        'Rollback' {
            Require-DestructiveSafety 'Test rollback'
            $verifier = Run-Native 'verifier.exe' @('/reset')
            Add-Case 'verifier-reset' ($verifier.ExitCode -eq 0) 'verifier.exe /reset' 'Verifier configuration cleared; reboot required.' $verifier.Output
            $testSign = Run-Native 'bcdedit.exe' @('/set','testsigning','off')
            Add-Case 'test-signing-disabled' ($testSign.ExitCode -eq 0) 'bcdedit.exe /set testsigning off' 'Test-signing disabled in guest; reboot required.' $testSign.Output
            if ($PnpInstanceId) {
                $device = Run-Native 'pnputil.exe' @('/remove-device',$PnpInstanceId)
                Add-Case 'rollback-device-remove' ($device.ExitCode -eq 0) "pnputil.exe /remove-device `"$PnpInstanceId`"" 'No BalanciFy device remains.' $device.Output
            } else { Add-Case 'rollback-device-remove' $false 'pnputil.exe /remove-device <exact instance ID>' 'Exact device removed.' 'NOT RUN: provide -PnpInstanceId; no wildcard removal is performed.' }
            if ($PublishedInf) {
                if ($PublishedInf -notmatch '^oem\d+\.inf$') { throw 'Published INF must be the exact oemNN.inf name.' }
                $del = Run-Native 'pnputil.exe' @('/delete-driver',$PublishedInf,'/uninstall')
                Add-Case 'rollback-package-remove' ($del.ExitCode -eq 0) "pnputil.exe /delete-driver $PublishedInf /uninstall" 'Package removed without /force.' $del.Output
            } else { Add-Case 'rollback-package-remove' $false 'pnputil.exe /delete-driver <exact oemNN.inf> /uninstall' 'Driver package removed.' 'NOT RUN: provide -PublishedInf.' }
            Add-Log 'After reboot: verify testsigning off, verifier reset, no adapter/driver package, and restore the host checkpoint if any residue or instability remains.'
        }
    }
} catch {
    Add-Case "stage-$Stage" $false "$Stage stage" 'Stage completes only inside a disposable Hyper-V guest and with explicit opt-ins for risky changes.' $_.Exception.Message
}

if (-not $OutputDirectory) { $OutputDirectory = Join-Path $PSScriptRoot '..\..\target\acceptance-evidence\driver-vm' }
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$file = Join-Path $OutputDirectory ("driver-{0}-{1}.json" -f $Stage.ToLowerInvariant(), (Get-Date -Format 'yyyyMMdd-HHmmss'))
$evidence = [pscustomobject]@{
    evidenceType = 'isolated Hyper-V driver test; no claim of driver runtime acceptance until review'
    stage = $Stage; timestampUtc = [DateTime]::UtcNow.ToString('o')
    machine = [pscustomobject]@{ computer=$env:COMPUTERNAME; caption=$os.Caption; version=$os.Version; build=$os.BuildNumber; manufacturer=$system.Manufacturer; model=$system.Model; hyperVGuest=$hyperVGuest; account=$identity.Name; elevatedAdmin=$admin }
    command = $MyInvocation.Line; cases = @($script:Cases); transcriptLog = @($script:Log)
}
$evidence | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $file -Encoding UTF8
Write-Host "Evidence JSON (cases + transcript): $file"
if (@($script:Cases | Where-Object result -eq 'FAIL').Count -gt 0) { exit 1 }
