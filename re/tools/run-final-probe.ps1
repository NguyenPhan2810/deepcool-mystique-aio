# Final isolated probes: 0x17, 0x08, 0x09.
#
# The service stays stopped for the whole run, so between candidates the device
# receives nothing. A traffic-sustained pattern therefore stops on its own and
# cannot contaminate the next candidate.

$ErrorActionPreference = 'Continue'
$repo = 'C:\Users\nguyen\dev\Deepcool-AIO-LCD'
$exe  = Join-Path $repo 'rust\target\debug\mystique.exe'
$log  = Join-Path $repo ('re\reports\final-probe-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')

function Say([string]$m) {
    $line = "[{0}] {1}" -f (Get-Date -Format 'HH:mm:ss'), $m
    Write-Host $line
    $line | Out-File -FilePath $log -Append -Encoding utf8
}

if (-not (Test-Path $exe)) { Say 'missing binary'; exit 2 }

$probes = @(
    @('0x17', '',             'third label slot, empty'),
    @('0x17', '414243444546', 'third label slot, ASCII ABCDEF'),
    @('0x08', '01',           'media/upload toggle, value 01'),
    @('0x09', '01',           '0x09 payload 01')
)

Say 'final probe start'
Get-CimInstance Win32_Process -Filter "Name='DeepCool.exe'" -ErrorAction SilentlyContinue |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Get-Process mystique-ui -ErrorAction SilentlyContinue |
    ForEach-Object { Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 2
try { Stop-Service MystiqueService -Force -ErrorAction Stop } catch { Say 'stop failed'; exit 2 }
Start-Sleep -Seconds 3

try {
    $n = 0
    foreach ($p in $probes) {
        $n++
        Write-Host ''
        Write-Host ('=' * 64) -ForegroundColor Cyan
        Write-Host ("  {0}/{1}   {2}   payload [{3}]" -f $n, $probes.Count, $p[0], $p[1]) -ForegroundColor Cyan
        Write-Host ("  {0}" -f $p[2]) -ForegroundColor DarkGray
        Write-Host ('=' * 64) -ForegroundColor Cyan
        Say ("{0}/{1} {2} [{3}] — {4}" -f $n, $probes.Count, $p[0], $p[1], $p[2])

        $try = if ($p[1]) { "$($p[0])`:$($p[1])`:`:init" } else { "$($p[0])`:`:`:init" }
        & $exe probe --send --try $try --timeout-ms 750 --out (Join-Path $repo 're\reports') *>&1 | Out-Null
        Say ("exit={0}" -f $LASTEXITCODE)
        Start-Sleep -Seconds 10
    }
}
finally {
    try {
        Start-Service MystiqueService -ErrorAction Stop
        Start-Sleep -Seconds 10
        Say "service status: $((Get-Service MystiqueService).Status)"
    }
    catch { Say 'SERVICE RESTART FAILED' }
    Say 'final probe end'
}

Write-Host ''
Write-Host 'Done. Report which numbers did anything.' -ForegroundColor Green
