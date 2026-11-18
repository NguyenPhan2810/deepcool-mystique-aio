# Does incoming telemetry end the 0xA1 colour test, or does it time out?
#
# Sends 0xA1, holds, then streams telemetry for ten seconds. The owner watches and
# reports the moment it stops cycling. The service is restored at the end.

$ErrorActionPreference = 'Continue'
$repo = 'C:\Users\nguyen\dev\Deepcool-AIO-LCD'
$exe  = Join-Path $repo 'rust\target\debug\mystique.exe'
$log  = Join-Path $repo ('re\reports\selftest-exit-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')

function Say([string]$m) {
    $line = "[{0}] {1}" -f (Get-Date -Format 'HH:mm:ss'), $m
    Write-Host $line
    $line | Out-File -FilePath $log -Append -Encoding utf8
}
function Banner([string]$m) {
    Write-Host ''
    Write-Host ('=' * 64) -ForegroundColor Cyan
    Write-Host "  $m" -ForegroundColor Cyan
    Write-Host ('=' * 64) -ForegroundColor Cyan
    Say $m
}

Say 'self-test exit probe start'
Get-CimInstance Win32_Process -Filter "Name='DeepCool.exe'" -ErrorAction SilentlyContinue |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Get-Process mystique-ui -ErrorAction SilentlyContinue |
    ForEach-Object { Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 2
try { Stop-Service MystiqueService -Force -ErrorAction Stop } catch { Say 'stop failed'; exit 2 }
Start-Sleep -Seconds 3

try {
    Banner '1/3  SENDING 0xA1  (panel should start cycling colours)'
    & $exe probe --send --try '0xA1::init' --timeout-ms 750 --out (Join-Path $repo 're\reports') *>&1 | Out-Null
    Start-Sleep -Seconds 6

    Banner '2/3  HOLDING 10s WITH NO TRAFFIC  (does it stop by itself?)'
    Start-Sleep -Seconds 10

    Banner '3/3  STREAMING TELEMETRY FOR 10s  (does data clear it?)'
    & $exe stream --seconds 10 *>&1 | Select-Object -Last 3 | ForEach-Object { Write-Host "    $_" }
    Start-Sleep -Seconds 4
}
finally {
    try {
        Start-Service MystiqueService -ErrorAction Stop
        Start-Sleep -Seconds 10
        Say "service status: $((Get-Service MystiqueService).Status)"
    }
    catch { Say 'SERVICE RESTART FAILED' }
    Say 'self-test exit probe end'
}

Write-Host ''
Write-Host 'Done. When did the cycling stop: during step 2 (silence), step 3 (data), or only after the service came back?' -ForegroundColor Green
