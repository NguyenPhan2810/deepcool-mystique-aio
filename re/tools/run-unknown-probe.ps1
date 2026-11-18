# Probe the command bytes the vendor app never sends.
#
# One packet at a time, banner before each, long pause after, so the owner can
# watch the panel and attribute any effect. Nothing here is known to be
# destructive; every payload is empty or a single byte, and the firmware has
# accepted arbitrary payloads in every prior sweep.
#
# Run elevated: it stops MystiqueService to free the device and restarts it after.

$ErrorActionPreference = 'Continue'
$repo = 'C:\Users\nguyen\dev\Deepcool-AIO-LCD'
$exe  = Join-Path $repo 'rust\target\debug\mystique.exe'
$log  = Join-Path $repo ('re\reports\unknown-probe-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')

function Say([string]$m) {
    $line = "[{0}] {1}" -f (Get-Date -Format 'HH:mm:ss'), $m
    Write-Host $line
    $line | Out-File -FilePath $log -Append -Encoding utf8
}

if (-not (Test-Path $exe)) { Say "missing binary: $exe"; exit 2 }

# cmd, payload hex, pipe, what we are asking
$probes = @(
    @('0x06', '',    'init', 'init step 0x06 — effect never observed'),
    @('0x0B', '',    'init', 'init step 0x0B — effect never observed'),
    @('0x09', '00',  'init', '0x09 payload 00 seen once in an upload capture'),
    @('0x09', '01',  'init', '0x09 payload 01 seen once'),
    @('0x0C', '',    'init', 'recognised in sweep, never sent by vendor'),
    @('0x0D', '',    'init', 'recognised in sweep, never sent by vendor'),
    @('0x0E', '',    'init', 'recognised in sweep, never sent by vendor'),
    @('0x13', '',    'init', 'recognised in sweep, never sent by vendor'),
    @('0xA1', '',    'init', 'recognised in sweep, never sent by vendor'),
    @('0x17', '',    'init', 'third label slot — empty'),
    @('0x17', '414243444546', 'init', 'third label slot — ASCII ABCDEF'),
    @('0x08', '00',  'init', 'media/upload toggle, value 00'),
    @('0x08', '01',  'init', 'media/upload toggle, value 01'),
    @('0x04', '00000102', 'init', '0x04 index 2 = 01 (main 0, style 0, aux 2)'),
    @('0x04', '00000202', 'init', '0x04 index 2 = 02 (main 0, style 0, aux 2)'),
    @('0x0A', 'ea070b120a2e20', 'init', '0x0A payload with altered trailing byte'),
    @('0x0A', 'ea070202022721', 'init', '0x0A 240-style machine-info payload')
)

Say 'unknown-command probe start'
Get-CimInstance Win32_Process -Filter "Name='DeepCool.exe'" -ErrorAction SilentlyContinue |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Get-Process mystique-ui -ErrorAction SilentlyContinue |
    ForEach-Object { Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 3

try { Stop-Service MystiqueService -Force -ErrorAction Stop }
catch { Say "could not stop service: $($_.Exception.Message)"; exit 2 }
Start-Sleep -Seconds 3

try {
    $n = 0
    foreach ($p in $probes) {
        $n++
        $cmd = $p[0]; $payload = $p[1]; $pipe = $p[2]; $why = $p[3]
        Write-Host ''
        Write-Host ('=' * 64) -ForegroundColor Cyan
        Write-Host ("  {0}/{1}   {2}   payload [{3}]   {4}" -f $n, $probes.Count, $cmd, $payload, $pipe) -ForegroundColor Cyan
        Write-Host ("  {0}" -f $why) -ForegroundColor DarkGray
        Write-Host ('=' * 64) -ForegroundColor Cyan
        Say ("{0}/{1} {2} [{3}] {4} — {5}" -f $n, $probes.Count, $cmd, $payload, $pipe, $why)

        $try = if ($payload) { "$cmd`:$payload`:$pipe" } else { "$cmd`:`:$pipe" }
        & $exe probe --send --try $try --timeout-ms 750 --out (Join-Path $repo 're\reports') *>&1 | Out-Null
        Say ("exit={0}" -f $LASTEXITCODE)
        Start-Sleep -Seconds 6
    }
}
finally {
    try {
        Start-Service MystiqueService -ErrorAction Stop
        Start-Sleep -Seconds 8
        Say "service status: $((Get-Service MystiqueService).Status)"
    }
    catch { Say "SERVICE RESTART FAILED: $($_.Exception.Message)" }
    Say 'unknown-command probe end'
}

Write-Host ''
Write-Host 'Done. Report which steps changed the panel.' -ForegroundColor Green
