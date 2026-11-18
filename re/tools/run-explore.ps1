# Elevated runner for the protocol explorer.
#
# Must run elevated: it stops MystiqueService so the explorer can own the device,
# and it restarts the service afterwards regardless of outcome.
#
#   Start-Process pwsh -Verb RunAs -ArgumentList '-NoProfile','-File',<this>,'-Full'
#
# -Full adds a second pass with --unsafe-all over every command byte. The
# allowlist pass always runs first, so a transport problem is caught before any
# unknown byte is touched.

param(
    [switch]$Full,
    [int]$TimeoutMs = 750,
    [string]$LogPath = ''
)

$ErrorActionPreference = 'Continue'
$repo = 'C:\Users\nguyen\dev\Deepcool-AIO-LCD'
$rust = Join-Path $repo 'rust'
$outDir = Join-Path $repo 're\reports'
$svc = 'MystiqueService'

if ([string]::IsNullOrWhiteSpace($LogPath)) {
    $LogPath = Join-Path $repo ('re\reports\explore-run-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')
}
New-Item -ItemType Directory -Force $outDir | Out-Null

function Log([string]$msg) {
    $line = "[{0}] {1}" -f (Get-Date -Format 'HH:mm:ss'), $msg
    Write-Host $line
    $line | Out-File -FilePath $LogPath -Append -Encoding utf8
}

Log "explorer run start (elevated=$([bool](([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))) full=$Full)"

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
    Log "aborting — the explorer must own the device"
    exit 2
}

try {
    Push-Location $rust

    Log "pass 1: allowlist presence sweep"
    & cargo run -q -p mystique-cli -- explore --send --phase presence --timeout-ms $TimeoutMs --out $outDir *>&1 |
        Tee-Object -FilePath $LogPath -Append
    $rc1 = $LASTEXITCODE
    Log "pass 1 exit=$rc1"

    if ($Full -and $rc1 -eq 0) {
        Start-Sleep -Seconds 2
        Log "pass 2: full sweep on auto pipe (--unsafe-all)"
        & cargo run -q -p mystique-cli -- explore --send --unsafe-all --phase presence --timeout-ms $TimeoutMs --out $outDir *>&1 |
            Tee-Object -FilePath $LogPath -Append
        $rc2 = $LASTEXITCODE
        Log "pass 2 exit=$rc2"

        if ($rc2 -eq 0) {
            # Coverage gap: unknown bytes were probed on the data pipe. A command
            # implemented only on the init pipe would look like a no-op there.
            Start-Sleep -Seconds 2
            Log "pass 3: full sweep on init pipe (--pipe init)"
            & cargo run -q -p mystique-cli -- explore --send --unsafe-all --phase presence --pipe init --timeout-ms $TimeoutMs --out $outDir *>&1 |
                Tee-Object -FilePath $LogPath -Append
            Log "pass 3 exit=$LASTEXITCODE"
        }
        else {
            Log "skipping init-pipe sweep because the auto-pipe sweep failed (rc=$rc2)"
        }
    }
    elseif ($Full) {
        Log "skipping full sweep because the allowlist pass failed (rc=$rc1)"
    }

    Pop-Location
}
catch {
    Log "explorer threw: $($_.Exception.Message)"
}
finally {
    if ($stopped) {
        Log "restarting $svc"
        try {
            Start-Service $svc -ErrorAction Stop
            Start-Sleep -Seconds 8
            Log "service restarted; status=$((Get-Service $svc).Status)"
        }
        catch {
            Log "SERVICE RESTART FAILED: $($_.Exception.Message)"
        }
    }
    Log "explorer run end"
}
