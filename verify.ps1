# Post-install verification. Run ELEVATED.
# Confirms frames are actually flowing, then measures a clean stop.

$ErrorActionPreference = 'Stop'
$out = Join-Path $env:TEMP 'mystique-verify.log'
Start-Transcript -Path $out -Force | Out-Null

$log = Join-Path $env:ProgramData 'Mystique\logs\mystique.log'

Write-Host '1. Waiting for the first "updates sent" summary (300 frames, about 5 minutes)...'
$deadline = (Get-Date).AddMinutes(8)
$summary = $null
while ((Get-Date) -lt $deadline -and -not $summary) {
    $summary = Select-String -Path $log -Pattern 'updates sent' | Select-Object -Last 1
    if (-not $summary) { Start-Sleep -Seconds 15 }
}

if ($summary) {
    Write-Host "   PASS - frames are flowing: $($summary.Line)" -ForegroundColor Green
} else {
    Write-Host '   FAIL - no summary line appeared; the service is not streaming.' -ForegroundColor Red
}

Write-Host ''
Write-Host '2. Measuring a clean stop (budget is 5s before Windows force-kills)...'
$sw = [System.Diagnostics.Stopwatch]::StartNew()
Stop-Service MystiqueService
$sw.Stop()
$secs = [math]::Round($sw.Elapsed.TotalSeconds, 2)
if ($secs -lt 5) {
    Write-Host "   PASS - stopped in ${secs}s, no force-kill" -ForegroundColor Green
} else {
    Write-Host "   FAIL - took ${secs}s, exceeds the kill budget" -ForegroundColor Red
}

Start-Sleep -Seconds 2
Start-Service MystiqueService
Write-Host "   Restarted: $((Get-Service MystiqueService).Status)"

Write-Host ''
Write-Host '3. Scanning the log for errors and identifier leaks...'
$bad = Select-String -Path $log -Pattern 'ERROR'
if ($bad) { Write-Host "   errors found:" -ForegroundColor Red; $bad } else { Write-Host '   PASS - no ERROR lines' -ForegroundColor Green }

# The identifier is not hardcoded here - writing it into a tracked file would
# leak the very value this check exists to catch. Supply it out of band:
#   $env:MYSTIQUE_DEVICE_ID = (& "$env:ProgramFiles\Mystique\..." )  # or paste it
# Get it from: dotnet run --project MystiqueUsbTool -- info --show-device-id
if ($env:MYSTIQUE_DEVICE_ID) {
    $leak = Select-String -Path $log -Pattern ([regex]::Escape($env:MYSTIQUE_DEVICE_ID))
    if ($leak) {
        Write-Host '   FAIL - device identifier present' -ForegroundColor Red; $leak
    } else {
        Write-Host '   PASS - no device identifier in the log' -ForegroundColor Green
    }
} else {
    # Fall back to a shape check: the service never dumps response bodies, so any
    # long uninterrupted hex run outside the interface GUID would be a regression.
    $dumps = Select-String -Path $log -Pattern '(?<![-{0-9a-fA-F])[0-9A-F]{8}(?![-}0-9a-fA-F])'
    if ($dumps) {
        Write-Host '   WARN - raw hex runs found; inspect these lines' -ForegroundColor Yellow; $dumps
    } else {
        Write-Host '   PASS - no raw hex dumps in the log' -ForegroundColor Green
        Write-Host '          (set MYSTIQUE_DEVICE_ID for an exact-match check)' -ForegroundColor DarkGray
    }
}

Write-Host ''
Write-Host 'Done. Reboot and watch the LCD at the login screen for the last check.'
Stop-Transcript | Out-Null
