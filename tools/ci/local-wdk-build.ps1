[CmdletBinding()]
param(
    [ValidateSet('x64', 'ARM64', 'all')]
    [string]$Architecture = 'all'
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $repoRoot

if (-not (Get-Command msbuild -ErrorAction SilentlyContinue)) {
    throw 'MSBuild 17.x was not found. Install Visual Studio 2022 / Build Tools with the C++ workload and WDK integration, then open a Developer PowerShell.'
}
if (-not (Get-Command py -ErrorAction SilentlyContinue)) {
    throw 'The Python launcher (py -3) is required to run tools/ci/verify_driver_pe.py.'
}

$platforms = if ($Architecture -eq 'all') { @('x64', 'ARM64') } else { @($Architecture) }
$artifactRoot = Join-Path $repoRoot 'target\local-wdk'
New-Item -ItemType Directory -Force -Path $artifactRoot | Out-Null

foreach ($platform in $platforms) {
    $archOutput = Join-Path $artifactRoot $platform
    $archIntermediate = Join-Path $artifactRoot "obj\$platform"
    New-Item -ItemType Directory -Force -Path $archOutput, $archIntermediate | Out-Null
    $buildLog = Join-Path $artifactRoot "wdk-$platform.log"
    $driver = Join-Path $archOutput 'balancify.sys'

    $arguments = @(
        'driver\balancify.vcxproj',
        '/m', '/nologo', '/v:minimal',
        '/p:Configuration=Release',
        "/p:Platform=$platform",
        '/p:SignMode=Off',
        "/p:OutDir=$archOutput\",
        "/p:IntDir=$archIntermediate\"
    )

    Write-Host "Building unsigned $platform driver; log: $buildLog"
    & msbuild @arguments 2>&1 | Tee-Object -FilePath $buildLog
    $buildExitCode = $LASTEXITCODE
    if ($buildExitCode -ne 0) {
        throw "MSBuild failed for $platform (exit $buildExitCode). See $buildLog"
    }
    if (-not (Test-Path -LiteralPath $driver -PathType Leaf)) {
        throw "Build succeeded but the expected driver was not created: $driver"
    }

    & py -3 tools/ci/verify_driver_pe.py $driver $platform
    if ($LASTEXITCODE -ne 0) {
        throw "PE validation failed for $platform. See $buildLog"
    }
}

Write-Host "Local WDK build and PE checks passed. Artifacts are unsigned and are stored under $artifactRoot. This does not validate driver installation, runtime behavior, Driver Verifier, HVCI, or hardware acceptance."
