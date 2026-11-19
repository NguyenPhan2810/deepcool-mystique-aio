#Requires -Version 7.0
<#
.SYNOPSIS
    Build the MYSTIQUE Control NSIS installer.

.DESCRIPTION
    The UI launches the daemon as a sibling binary, so the installer must contain both
    executables side by side. The order is load-bearing: the daemon is built and staged
    in `installer/stage` first, then Tauri bundles the UI and copies that staged file in.
    Running `tauri build` on its own is still safe because `build.beforeBuildCommand`
    runs the same staging script, but this wrapper is the supported entry point and also
    verifies the finished installer.

    The Tauri CLI is optional. If `cargo tauri` is installed it is used, otherwise the
    pinned major version is fetched with `pnpm dlx` (no global install).
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$rustRoot = Split-Path -Parent $PSScriptRoot
$appDir = Join-Path $rustRoot 'apps/mystique-ui'
$prepare = Join-Path $appDir 'installer/prepare-daemon.ps1'
$staged = Join-Path $appDir 'installer/stage/mystique-daemon.exe'
$bundleDir = Join-Path $rustRoot 'target/release/bundle/nsis'

function Invoke-Checked {
    param(
        [Parameter(Mandatory)]
        [scriptblock]$Command,
        [Parameter(Mandatory)]
        [string]$What
    )

    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "$What failed with exit code $LASTEXITCODE."
    }
}

Write-Host '==> 1/2  Building and staging mystique-daemon (release)'
& $prepare
if (-not (Test-Path -LiteralPath $staged -PathType Leaf)) {
    throw "The daemon was not staged at $staged. Refusing to bundle without it."
}
$stagedFile = Get-Item -LiteralPath $staged
Write-Host ("    {0} ({1:N1} MB, {2:u})" -f $stagedFile.FullName, ($stagedFile.Length / 1MB), $stagedFile.LastWriteTimeUtc)

Write-Host '==> 2/2  Bundling MYSTIQUE Control (NSIS)'
Push-Location -LiteralPath $appDir
try {
    $cargoTauri = $false
    & cargo tauri --version *> $null
    if ($LASTEXITCODE -eq 0) {
        $cargoTauri = $true
    }

    if ($cargoTauri) {
        Write-Host '    using cargo tauri'
        Invoke-Checked -What 'cargo tauri build' -Command {
            cargo tauri build --bundles nsis
        }
    }
    else {
        if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
            throw 'Neither `cargo tauri` nor pnpm is available. Install one: cargo install tauri-cli --version "^2"'
        }
        Write-Host '    cargo tauri not installed; using pnpm dlx @tauri-apps/cli@2'
        Invoke-Checked -What 'pnpm dlx @tauri-apps/cli@2 build' -Command {
            pnpm dlx '@tauri-apps/cli@2' build --bundles nsis
        }
    }
}
finally {
    Pop-Location
}

$installer = Get-ChildItem -LiteralPath $bundleDir -Filter '*.exe' -File -ErrorAction SilentlyContinue |
    Sort-Object -Property LastWriteTimeUtc -Descending |
    Select-Object -First 1
if (-not $installer) {
    throw "No installer was produced under $bundleDir."
}

$hash = (Get-FileHash -LiteralPath $installer.FullName -Algorithm SHA256).Hash
Write-Host ''
Write-Host ("Installer: {0}" -f $installer.FullName)
Write-Host ("Size:      {0:N1} MB ({1:N0} bytes)" -f ($installer.Length / 1MB), $installer.Length)
Write-Host ("SHA256:    {0}" -f $hash)

$sevenZipPath = $null
$sevenZip = Get-Command 7z, 7za -ErrorAction SilentlyContinue | Select-Object -First 1
if ($sevenZip) {
    $sevenZipPath = $sevenZip.Source
}
else {
    $fallback = Join-Path $env:ProgramFiles '7-Zip\7z.exe'
    if (Test-Path -LiteralPath $fallback -PathType Leaf) {
        $sevenZipPath = $fallback
    }
}

if ($sevenZipPath) {
    $listing = & $sevenZipPath l -ba $installer.FullName 2>$null
    $hasUi = $listing -match 'mystique-ui\.exe'
    $hasDaemon = $listing -match 'mystique-daemon\.exe'
    if (-not ($hasUi -and $hasDaemon)) {
        throw 'The installer does not contain both mystique-ui.exe and mystique-daemon.exe.'
    }
    Write-Host 'Verified:  mystique-ui.exe and mystique-daemon.exe are both inside.'
}
else {
    Write-Warning '7-Zip was not found; skipping the archive content check.'
}
