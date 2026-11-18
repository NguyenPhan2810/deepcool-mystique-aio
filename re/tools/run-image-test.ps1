# First real image-upload test against the MYSTIQUE 360.
#
# Sequence, each step announced on the console so the owner can watch the panel:
#   1. stop the telemetry service so the CLI can own the device
#   2. upload a distinctive 480x640 JPEG
#   3. hold for a look
#   4. attempt to return to Machine Info with the normal init handshake
#   5. read status, restart the service
#
# If step 4 fails to bring back telemetry, the panel stays on the uploaded image
# until the USB device is power-cycled. That is the known risk.

$ErrorActionPreference = 'Continue'
$repo = 'C:\Users\nguyen\dev\Deepcool-AIO-LCD'
$exe  = Join-Path $repo 'rust\target\debug\mystique.exe'
$img  = Join-Path $repo 're\captures\test-upload.png'
$log  = Join-Path $repo ('re\reports\image-test-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')

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

if (-not (Test-Path $exe)) { Say "missing binary"; exit 2 }
if (-not (Test-Path $img)) { Say "missing image"; exit 2 }

Say "image test start; binary=$exe"

Banner '1/5  CLOSING VENDOR APP AND STOPPING SERVICE'
Get-CimInstance Win32_Process -Filter "Name='DeepCool.exe'" -ErrorAction SilentlyContinue |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 3
try { Stop-Service MystiqueService -Force -ErrorAction Stop } catch { Say "stop failed: $($_.Exception.Message)"; exit 2 }
Start-Sleep -Seconds 3

try {
    Banner '2/5  UPLOADING IMAGE  (watch the panel)'
    & $exe image $img --send --rotation 0 *>&1 | Tee-Object -FilePath $log -Append
    Say "image exit=$LASTEXITCODE"

    Banner '3/5  HOLDING 12 SECONDS  — does the panel show RE / CAPTURE ?'
    Start-Sleep -Seconds 12

    Banner '4/5  ATTEMPTING RETURN TO MACHINE INFO'
    & $exe init --rotation 0 *>&1 | Tee-Object -FilePath $log -Append
    Say "init exit=$LASTEXITCODE"
    Start-Sleep -Seconds 6

    Banner '5/5  STATUS + SERVICE RESTORE'
    & $exe status *>&1 | Tee-Object -FilePath $log -Append
    Say "status exit=$LASTEXITCODE"
}
finally {
    try {
        Start-Service MystiqueService -ErrorAction Stop
        Start-Sleep -Seconds 10
        Say "service status: $((Get-Service MystiqueService).Status)"
    }
    catch { Say "SERVICE RESTART FAILED: $($_.Exception.Message)" }
    Say 'image test end'
}

Write-Host ''
Write-Host 'Done. Please report what the panel showed at each stage.' -ForegroundColor Green
