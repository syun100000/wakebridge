[CmdletBinding()]
param(
    [Parameter()]
    [string]$ExecutablePath = (Join-Path $PSScriptRoot '..\target\release\wakebridge.exe'),

    [Parameter()]
    [string]$IsccPath
)

$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$scriptPath = Join-Path $PSScriptRoot 'WakeBridge.iss'
$payloadDir = Join-Path $PSScriptRoot 'payload'
$payloadPath = Join-Path $payloadDir 'wakebridge.exe'
$distDir = Join-Path $repoRoot 'dist'

$executable = (Resolve-Path -LiteralPath $ExecutablePath -ErrorAction Stop).Path
if ([IO.Path]::GetFileName($executable) -ne 'wakebridge.exe') {
    throw "ExecutablePath must point to wakebridge.exe: $executable"
}

$cargoToml = Get-Content -LiteralPath (Join-Path $repoRoot 'Cargo.toml') -Raw
$versionMatch = [regex]::Match($cargoToml, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $versionMatch.Success) {
    throw 'Cargo.toml version was not found.'
}
$version = $versionMatch.Groups[1].Value

if (-not $IsccPath) {
    $command = Get-Command iscc.exe -ErrorAction SilentlyContinue
    if ($command) {
        $IsccPath = $command.Source
    }
}
if (-not $IsccPath) {
    $knownPaths = @(
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 7\ISCC.exe'),
        (Join-Path $env:ProgramFiles 'Inno Setup 7\ISCC.exe')
    ) | Where-Object { $_ -and (Test-Path -LiteralPath $_) }
    if ($knownPaths.Count -gt 0) {
        $IsccPath = $knownPaths[0]
    }
}
if (-not $IsccPath -or -not (Test-Path -LiteralPath $IsccPath)) {
    throw 'Inno Setup ISCC.exe was not found. Install Inno Setup on the build machine, then rerun this script.'
}

New-Item -ItemType Directory -Path $payloadDir -Force | Out-Null
New-Item -ItemType Directory -Path $distDir -Force | Out-Null
try {
    Copy-Item -LiteralPath $executable -Destination $payloadPath -Force
    & $IsccPath "/DMyAppVersion=$version" "/DMyAppSource=$payloadPath" $scriptPath
    if ($LASTEXITCODE -ne 0) {
        throw "ISCC.exe failed with exit code $LASTEXITCODE."
    }

    $installerPath = Join-Path $distDir "WakeBridge-Setup-$version-x64.exe"
    if (-not (Test-Path -LiteralPath $installerPath)) {
        throw "Installer output was not found: $installerPath"
    }
    $hash = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -LiteralPath "$installerPath.sha256" -Value "$hash  $(Split-Path -Leaf $installerPath)" -Encoding ascii
    Write-Output "Installer: $installerPath"
    Write-Output "SHA256: $hash"
}
finally {
    Remove-Item -LiteralPath $payloadDir -Recurse -Force -ErrorAction SilentlyContinue
}
