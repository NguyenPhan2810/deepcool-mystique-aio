#Requires -Version 7.0
<#
.SYNOPSIS
    Build `mystique-daemon` in release and stage it for `bundle.resources`.

.DESCRIPTION
    Tauri copies `bundle.resources` next to the built UI in every profile, including
    debug. Mapping the daemon straight to `mystique-daemon.exe` would therefore try to
    overwrite the debug daemon, which is a running process during development and cannot
    be replaced on Windows. The resource is staged under `installer/stage` instead and
    installed into a `daemon` subdirectory; `hooks.nsh` moves it beside `mystique-ui.exe`
    after the file copy.

    This runs as `build.beforeBuildCommand`, so even a bare `tauri build` cannot bundle
    an old or absent daemon. `rust/scripts/build-installer.ps1` also calls it explicitly
    to make the order visible.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$installerDir = $PSScriptRoot
$appDir = Split-Path -Parent $installerDir
$rustRoot = Split-Path -Parent (Split-Path -Parent $appDir)
$manifest = Join-Path $rustRoot 'Cargo.toml'
$daemon = Join-Path $rustRoot 'target/release/mystique-daemon.exe'
$stage = Join-Path $installerDir 'stage'
$staged = Join-Path $stage 'mystique-daemon.exe'

Write-Host 'prepare-daemon: building mystique-daemon (release)'
& cargo build --release --manifest-path $manifest -p mystique-daemon
if ($LASTEXITCODE -ne 0) {
    throw "cargo build -p mystique-daemon failed with exit code $LASTEXITCODE."
}
if (-not (Test-Path -LiteralPath $daemon -PathType Leaf)) {
    throw "The daemon binary was not produced at $daemon."
}

# A stale copy must never survive a failed build or a shortened script run.
New-Item -ItemType Directory -Path $stage -Force | Out-Null
Get-ChildItem -LiteralPath $stage -File -Force |
    Where-Object { $_.Name -ne '.gitignore' } |
    Remove-Item -Force

Copy-Item -LiteralPath $daemon -Destination $staged -Force

$sourceHash = (Get-FileHash -LiteralPath $daemon -Algorithm SHA256).Hash
$stagedHash = (Get-FileHash -LiteralPath $staged -Algorithm SHA256).Hash
if ($sourceHash -ne $stagedHash) {
    throw "The staged daemon does not match $daemon."
}

$file = Get-Item -LiteralPath $staged
Write-Host ("prepare-daemon: staged {0} ({1:N0} bytes)" -f $file.FullName, $file.Length)
