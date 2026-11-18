# Elevated wrapper for the USB capture harness.
#
# The DeepCool display broker runs as LocalSystem; attaching to it needs
# SeDebugPrivilege, so the harness has to run elevated. Started via:
#   Start-Process pwsh -Verb RunAs -ArgumentList '-NoProfile','-File',<this>,'-TargetPid',<pid>
#
# Writes both stdout and the JSONL beside it so the result can be read from a
# non-elevated session afterwards.

param(
    [Parameter(Mandatory = $true)][int]$TargetPid,
    [int]$Seconds = 900,
    [string]$LogPath = ''
)

$ErrorActionPreference = 'Continue'
$repo = 'C:\Users\nguyen\dev\Deepcool-AIO-LCD'

if ([string]::IsNullOrWhiteSpace($LogPath)) {
    $LogPath = Join-Path $repo ('re\captures\elevated-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')
}

Set-Location $repo

"[$(Get-Date -Format o)] elevated capture start; target pid=$TargetPid seconds=$Seconds" |
    Out-File -FilePath $LogPath -Encoding utf8

try {
    & node 're\tools\frida-usb-capture.mjs' --pid $TargetPid --seconds $Seconds *>&1 |
        Out-File -FilePath $LogPath -Append -Encoding utf8
}
catch {
    "capture failed: $($_.Exception.Message)" | Out-File -FilePath $LogPath -Append -Encoding utf8
}

"[$(Get-Date -Format o)] elevated capture end" | Out-File -FilePath $LogPath -Append -Encoding utf8
