# Bisect the unknown bytes, one at a time, with automatic recovery after each.
#
# Each candidate is followed by an erase (0x14) and a full init, so a byte that
# starts an animation is cleared before the next one is tried. The service is
# restored at the end.

$ErrorActionPreference = 'Continue'
$repo = 'C:\Users\nguyen\dev\Deepcool-AIO-LCD'
$exe  = Join-Path $repo 'rust\target\debug\mystique.exe'
$log  = Join-Path $repo ('re\reports\bisect-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')

function Say([string]$m) {
    $line = "[{0}] {1}" -f (Get-Date -Format 'HH:mm:ss'), $m
    Write-Host $line
    $line | Out-File -FilePath $log -Append -Encoding utf8
}

if (-not (Test-Path $exe)) { Say "missing binary"; exit 2 }

$candidates = @('0x0C','0x0D','0x0E','0x13','0xA1','0x17','0x08','0x09')

Say 'bisect start'
Get-CimInstance Win32_Process -Filter "Name='DeepCool.exe'" -ErrorAction SilentlyContinue |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Get-Process mystique-ui -ErrorAction SilentlyContinue |
    ForEach-Object { Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 2
try { Stop-Service MystiqueService -Force -ErrorAction Stop } catch { Say "stop failed"; exit 2 }
Start-Sleep -Seconds 3

try {
    $n = 0
    foreach ($cmd in $candidates) {
        $n++
        Write-Host ''
        Write-Host ('=' * 64) -ForegroundColor Cyan
        Write-Host ("  {0}/{1}   SENDING {2}   (watch the panel)" -f $n, $candidates.Count, $cmd) -ForegroundColor Cyan
        Write-Host ('=' * 64) -ForegroundColor Cyan
        Say ("{0}/{1} {2}" -f $n, $candidates.Count, $cmd)
        & $exe probe --send --try "$cmd`:`:init" --timeout-ms 750 --out (Join-Path $repo 're\reports') *>&1 | Out-Null
        Start-Sleep -Seconds 5

        Write-Host '  clearing with erase + init' -ForegroundColor DarkGray
        Say '  clearing'
        & $exe probe --send --try '0x14::init' --timeout-ms 750 --out (Join-Path $repo 're\reports') *>&1 | Out-Null
        & $exe init --rotation 0 *>&1 | Out-Null
        Start-Sleep -Seconds 4
    }
}
finally {
    try {
        Start-Service MystiqueService -ErrorAction Stop
        Start-Sleep -Seconds 8
        Say "service status: $((Get-Service MystiqueService).Status)"
    }
    catch { Say "SERVICE RESTART FAILED" }
    Say 'bisect end'
}

Write-Host ''
Write-Host 'Done. Report the numbers that did anything.' -ForegroundColor Green
