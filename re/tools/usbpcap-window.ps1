# Capture a short USBPcap window and report whether it contains MYSTIQUE traffic.
#
# Must run elevated: USBPcapCMD auto-elevates and cannot be stopped from a
# medium-integrity process. Killing it is also how the capture is ended, since it
# otherwise runs until Ctrl+C.

param(
    [int]$Hub = 1,
    [int]$Seconds = 8,
    [string]$OutFile = ''
)

$ErrorActionPreference = 'Continue'
$exe = 'C:\Program Files\USBPcap\USBPcapCMD.exe'
$repo = 'C:\Users\nguyen\dev\Deepcool-AIO-LCD'

if ([string]::IsNullOrWhiteSpace($OutFile)) {
    $OutFile = Join-Path $repo ('re\captures\usbpcap-hub' + $Hub + '-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.pcap')
}
New-Item -ItemType Directory -Force (Split-Path $OutFile) | Out-Null

# Clear any previous capture that is still holding the device.
Get-Process USBPcapCMD -ErrorAction SilentlyContinue | ForEach-Object {
    try { Stop-Process -Id $_.Id -Force -ErrorAction Stop } catch {}
}
Start-Sleep -Seconds 1

Write-Host "capturing $Seconds s on \\.\USBPcap$Hub -> $OutFile"
$p = Start-Process -FilePath $exe `
    -ArgumentList @('-d', "\\.\USBPcap$Hub", '-o', $OutFile, '-A', '-s', '65535', '--inject-descriptors') `
    -PassThru -WindowStyle Hidden

Start-Sleep -Seconds $Seconds

Get-Process USBPcapCMD -ErrorAction SilentlyContinue | ForEach-Object {
    try { Stop-Process -Id $_.Id -Force -ErrorAction Stop } catch { Write-Host "kill failed: $($_.Exception.Message)" }
}
Start-Sleep -Seconds 2

if (-not (Test-Path $OutFile)) { Write-Host 'no capture file'; exit 1 }

$bytes = [IO.File]::ReadAllBytes($OutFile)
$text  = [Text.Encoding]::ASCII.GetString($bytes)
$hidc  = ([regex]::Matches($text, 'HIDC')).Count
$aa2e  = 0
for ($i = 0; $i -lt $bytes.Length - 1; $i++) {
    if ($bytes[$i] -eq 0xAA -and $bytes[$i + 1] -eq 0x2E) { $aa2e++ }
}

Write-Host ("size={0} bytes  HIDC={1}  AA2E={2}" -f $bytes.Length, $hidc, $aa2e)
if ($hidc -gt 0) { Write-Host '*** captures MYSTIQUE traffic ***' -ForegroundColor Green }
else { Write-Host 'no MYSTIQUE traffic in this window' -ForegroundColor Yellow }
