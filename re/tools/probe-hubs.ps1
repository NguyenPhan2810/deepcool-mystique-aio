# Identify which USBPcap root hub carries the MYSTIQUE.
#
# Short captures are taken on each \\.\USBPcapN in turn and scanned for the
# literal "HIDC" footer, which only this device's 48-byte packets carry. The
# telemetry service streams at ~1 Hz, so 6 seconds is plenty when the hub is right.

$ErrorActionPreference = 'Continue'
$exe = 'C:\Program Files\USBPcap\USBPcapCMD.exe'
$tmp = Join-Path $env:TEMP 'usbpcap-probe'
New-Item -ItemType Directory -Force $tmp | Out-Null

foreach ($n in 1..4) {
    $dev  = "\\.\USBPcap$n"
    $out  = Join-Path $tmp "hub$n.pcap"
    Remove-Item $out -ErrorAction SilentlyContinue

    Write-Host "--- probing $dev"
    $p = Start-Process -FilePath $exe -ArgumentList @('-d', $dev, '-o', $out, '-A', '-s', '65535') -PassThru -WindowStyle Hidden
    Start-Sleep -Seconds 6
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 400

    if (-not (Test-Path $out)) { Write-Host "    no capture file (device not present)"; continue }

    $bytes = [IO.File]::ReadAllBytes($out)
    $text  = [Text.Encoding]::ASCII.GetString($bytes)
    $hidc  = ([regex]::Matches($text, 'HIDC')).Count
    $aa2e  = 0
    for ($i = 0; $i -lt $bytes.Length - 1; $i++) {
        if ($bytes[$i] -eq 0xAA -and $bytes[$i + 1] -eq 0x2E) { $aa2e++ }
    }

    Write-Host ("    size={0} bytes  HIDC={1}  AA2E={2}" -f $bytes.Length, $hidc, $aa2e)
    if ($hidc -gt 0) { Write-Host "    *** MATCH: use $dev ***" -ForegroundColor Green }
}
