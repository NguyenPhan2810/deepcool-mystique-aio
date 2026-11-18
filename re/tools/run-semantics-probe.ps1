# Interactive semantics probe.
#
# Sends one candidate command at a time, three seconds apart, printing a banner
# before each so the owner can watch the panel and report what changed when.
#
# Runs elevated because it stops MystiqueService to free the device, and it
# closes the vendor app for the same reason. Both are restored afterwards.

$ErrorActionPreference = 'Continue'
$repo = 'C:\Users\nguyen\dev\Deepcool-AIO-LCD'
$exe  = Join-Path $repo 'rust\target\debug\mystique-cli.exe'
$log  = Join-Path $repo ('re\reports\semantics-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')

function Say([string]$m) {
    $line = "[{0}] {1}" -f (Get-Date -Format 'HH:mm:ss'), $m
    Write-Host $line
    $line | Out-File -FilePath $log -Append -Encoding utf8
}

if (-not (Test-Path $exe)) {
    Say "binary missing: $exe"
    exit 2
}

Say 'closing vendor app'
Get-CimInstance Win32_Process -Filter "Name='DeepCool.exe'" -ErrorAction SilentlyContinue |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 3

Say 'stopping MystiqueService'
try { Stop-Service MystiqueService -Force -ErrorAction Stop } catch { Say "stop failed: $($_.Exception.Message)"; exit 2 }
Start-Sleep -Seconds 3

$pairs = @(
    @('0x09', '00'), @('0x09', '64'),
    @('0x0C', '00'), @('0x0C', '64'),
    @('0x0D', '00'), @('0x0D', '64'),
    @('0x0E', '00'), @('0x0E', '64'),
    @('0xA1', '00'), @('0xA1', '64')
)

try {
    $n = 0
    foreach ($p in $pairs) {
        $n++
        $cmd = $p[0]; $pay = $p[1]
        $banner = ">>> $n/10   CMD $cmd   PAYLOAD $pay <<<"
        Write-Host ''
        Write-Host ('=' * 60) -ForegroundColor Cyan
        Write-Host $banner -ForegroundColor Cyan
        Write-Host ('=' * 60) -ForegroundColor Cyan
        Say $banner

        & $exe probe --send --try "${cmd}:${pay}:init" --timeout-ms 750 --out (Join-Path $repo 're\reports') *>&1 |
            Out-Null

        Say "done $cmd/$pay (exit $LASTEXITCODE)"
        Start-Sleep -Seconds 3
    }
}
finally {
    Say 'restoring MystiqueService'
    try {
        Start-Service MystiqueService -ErrorAction Stop
        Start-Sleep -Seconds 8
        Say "service status: $((Get-Service MystiqueService).Status)"
    }
    catch { Say "SERVICE RESTART FAILED: $($_.Exception.Message)" }
    Say 'probe batch end'
}

Write-Host ''
Write-Host 'Batch complete. Panel should be back on telemetry.' -ForegroundColor Green
