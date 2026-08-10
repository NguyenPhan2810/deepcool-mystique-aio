# Installs MystiqueService so the LCD is driven from boot, before any logon.
#
# Run from an ELEVATED PowerShell:
#   powershell -ExecutionPolicy Bypass -File .\install.ps1
#
# Uninstall with:  & "$env:ProgramFiles\Mystique\MystiqueService.exe" uninstall

#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'

# Transcript so the outcome survives the window being closed.
$transcript = Join-Path $env:TEMP 'mystique-install.log'
Start-Transcript -Path $transcript -Force | Out-Null
trap {
    Write-Host "INSTALL FAILED: $_" -ForegroundColor Red
    Stop-Transcript | Out-Null
    Write-Host "Press Enter to close."
    [void][Console]::ReadLine()
    exit 1
}

$source = Join-Path $PSScriptRoot 'artifacts\publish'
$target = Join-Path $env:ProgramFiles 'Mystique'

if (-not (Test-Path (Join-Path $source 'MystiqueService.exe'))) {
    throw "Build output not found in $source. Run: dotnet publish dev/MystiqueService -c Release -o artifacts/publish"
}

# The tool and the service cannot both own the WinUSB handle. Only one process
# may hold it, so clear any manual instance before the service starts.
Get-Process MystiqueUsbTool, MystiqueService -ErrorAction SilentlyContinue | Stop-Process -Force

# Windows locks a running image, so stop the service before overwriting it.
$existing = Get-Service MystiqueService -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host 'Existing service found; stopping and removing it first.'
    & (Join-Path $target 'MystiqueService.exe') uninstall
    Start-Sleep -Seconds 2
}

Write-Host "Copying to $target"
New-Item -ItemType Directory -Force -Path $target | Out-Null
Copy-Item -Path (Join-Path $source '*') -Destination $target -Recurse -Force

Write-Host 'Registering the service'
& (Join-Path $target 'MystiqueService.exe') install
if ($LASTEXITCODE -ne 0) { throw "install failed with exit code $LASTEXITCODE" }

Write-Host ''
Write-Host 'Configuration:' -ForegroundColor Cyan
sc.exe qc MystiqueService | Select-String 'START_TYPE|SERVICE_START_NAME|BINARY_PATH_NAME|DEPENDENCIES'
sc.exe qsidtype MystiqueService | Select-String 'SERVICE_SID_TYPE'

Write-Host ''
Write-Host 'Log: ' -NoNewline
Write-Host (Join-Path $env:ProgramData 'Mystique\logs\mystique.log') -ForegroundColor Yellow
Write-Host 'The LCD should be updating now, and again at the login screen after a reboot.'
Stop-Transcript | Out-Null
