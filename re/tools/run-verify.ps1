# Verification pass: send each decoded setting from our own driver, one at a time,
# with a banner and a pause so the owner can watch the panel.
#
# Every command here was decoded from a capture of the vendor app doing the same
# thing; this pass proves our encoder produces bytes the panel accepts.

$ErrorActionPreference = 'Continue'
$repo = 'C:\Users\nguyen\dev\Deepcool-AIO-LCD'
$exe  = Join-Path $repo 'rust\target\debug\mystique.exe'
$log  = Join-Path $repo ('re\reports\verify-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')

function Say([string]$m) {
    $line = "[{0}] {1}" -f (Get-Date -Format 'HH:mm:ss'), $m
    Write-Host $line
    $line | Out-File -FilePath $log -Append -Encoding utf8
}
function Step([string]$title, [string[]]$arguments, [int]$hold) {
    Write-Host ''
    Write-Host ('=' * 64) -ForegroundColor Cyan
    Write-Host "  $title" -ForegroundColor Cyan
    Write-Host ('=' * 64) -ForegroundColor Cyan
    Say $title
    & $exe @arguments *>&1 | ForEach-Object { Write-Host "    $_" }
    Say ("exit={0}" -f $LASTEXITCODE)
    Start-Sleep -Seconds $hold
}

if (-not (Test-Path $exe)) { Say "missing binary: $exe"; exit 2 }

Say 'verification start'
Get-CimInstance Win32_Process -Filter "Name='DeepCool.exe'" -ErrorAction SilentlyContinue |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 3
try { Stop-Service MystiqueService -Force -ErrorAction Stop } catch { Say "stop failed: $($_.Exception.Message)"; exit 2 }
Start-Sleep -Seconds 3

try {
    Step '1/6  brightness 20  (expect: dim)'            @('brightness','20','--send') 9
    Step '2/6  brightness 92  (expect: bright)'          @('brightness','92','--send') 9
    Step '3/6  rotate 1  (expect: 90 degrees)'           @('rotate','1','--send')      9
    Step '4/6  rotate 0  (expect: upright again)'        @('rotate','0','--send')      9
    Step '5/6  areas --main 5 --aux 2  (expect: layout changes)' @('areas','--main','5','--aux','2','--send') 9
    Step '6/6  screen off, then on'                      @('screen','off','--send')    6
    & $exe screen on --send *>&1 | ForEach-Object { Write-Host "    $_" }
    Start-Sleep -Seconds 4
}
finally {
    try {
        Start-Service MystiqueService -ErrorAction Stop
        Start-Sleep -Seconds 10
        Say "service status: $((Get-Service MystiqueService).Status)"
    }
    catch { Say "SERVICE RESTART FAILED: $($_.Exception.Message)" }
    Say 'verification end'
}

Write-Host ''
Write-Host 'Done. Report which steps changed the panel.' -ForegroundColor Green
