# Elevated runner for `mystique probe`.
#
# Must run elevated: it stops MystiqueService so the probe can own the device,
# and it restarts the service afterwards no matter what happens.
#
#   Start-Process pwsh -Verb RunAs -ArgumentList '-NoProfile','-File',<this>,'-Batch',<file>
#
# Wedge policy (re/findings/09-protocol-mapping-plan.md, rule 5): the probe verb
# stops after the first blocked write and exits 3. A service restart alone is not
# enough; the PnP device is restarted, then the service is started, then this
# script stops. It never retries the batch.

param(
    [Parameter(Mandatory = $true)][string]$Batch,
    [int]$TimeoutMs = 750,
    [string]$LogPath = ''
)

$ErrorActionPreference = 'Continue'
$repo = 'C:\Users\nguyen\dev\Deepcool-AIO-LCD'
$rust = Join-Path $repo 'rust'
$outDir = Join-Path $repo 're\reports'
$svc = 'MystiqueService'
$instance = (Get-PnpDevice -PresentOnly |
    Where-Object { $_.InstanceId -like '*VID_3633&PID_0009*' } |
    Select-Object -First 1).InstanceId
if (-not $instance) { throw 'MYSTIQUE device not present' }

if ([string]::IsNullOrWhiteSpace($LogPath)) {
    $LogPath = Join-Path $repo ('re\reports\probe-run-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')
}
New-Item -ItemType Directory -Force $outDir | Out-Null

function Log([string]$msg) {
    $line = "[{0}] {1}" -f (Get-Date -Format 'HH:mm:ss'), $msg
    Write-Host $line
    $line | Out-File -FilePath $LogPath -Append -Encoding utf8
}

Log "probe run start (elevated=$([bool](([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))) batch=$Batch timeout=$TimeoutMs)"

if (-not (Test-Path $Batch)) {
    Log "batch file not found: $Batch"
    exit 2
}

$stopped = $false
try {
    Log "stopping $svc"
    Stop-Service $svc -Force -ErrorAction Stop
    $stopped = $true
    Start-Sleep -Seconds 3
    Log "service stopped"
}
catch {
    Log "could not stop service: $($_.Exception.Message)"
    Log "aborting — the probe must own the device"
    exit 2
}

$rc = $null
try {
    Push-Location $rust
    Log "probe batch"
    & cargo run -q -p mystique-cli -- probe --send --from-file $Batch --timeout-ms $TimeoutMs --out $outDir *>&1 |
        Tee-Object -FilePath $LogPath -Append
    $rc = $LASTEXITCODE
    Log "probe exit=$rc"
    Pop-Location
}
catch {
    Log "probe threw: $($_.Exception.Message)"
}
finally {
    if ($rc -eq 3) {
        Log "WEDGE: exit 3. Restarting the PnP device (a service restart alone is not enough)."
        try {
            & pnputil.exe /restart-device $instance *>&1 | Tee-Object -FilePath $LogPath -Append
            Log "pnputil exit=$LASTEXITCODE"
        }
        catch {
            Log "pnputil failed: $($_.Exception.Message)"
        }
        Log "waiting 15 s for the device to re-enumerate"
        Start-Sleep -Seconds 15
    }

    if ($stopped) {
        Log "restarting $svc"
        try {
            Start-Service $svc -ErrorAction Stop
            Start-Sleep -Seconds 10
            Log "service restarted; status=$((Get-Service $svc).Status)"
        }
        catch {
            Log "SERVICE RESTART FAILED: $($_.Exception.Message)"
        }
    }
    Log "probe run end"
}
