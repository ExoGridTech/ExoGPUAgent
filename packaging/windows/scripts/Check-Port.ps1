# Fails the installer if 127.0.0.1:<port> is occupied by a non-healthy listener.
# Reads port from ProgramData config when present (default 12001).
$ErrorActionPreference = "Stop"

$cfg = Join-Path $env:ProgramData "ExoGridTech\ExoGPUAgent\config.toml"
$port = 12001
if (Test-Path $cfg) {
    foreach ($line in Get-Content $cfg) {
        if ($line -match '^\s*port\s*=\s*(\d+)\s*$') {
            $port = [int]$Matches[1]
        }
    }
}

$uri = "http://127.0.0.1:$port/health"
try {
    $r = Invoke-RestMethod -Uri $uri -TimeoutSec 2
    if ($r.status -eq "ok") {
        Write-Host "Port $port already serves ExoGPUAgent /health (ok) — continuing."
        exit 0
    }
} catch {
    # Not our healthy agent — check whether anything is listening.
}

$inUse = $false
try {
    $client = New-Object System.Net.Sockets.TcpClient
    $iar = $client.BeginConnect("127.0.0.1", $port, $null, $null)
    $ok = $iar.AsyncWaitHandle.WaitOne(500, $false)
    if ($ok -and $client.Connected) { $inUse = $true }
    $client.Close()
} catch {
    $inUse = $false
}

if ($inUse) {
    Write-Error @"
Localhost port $port is already in use, but ExoGPUAgent /health is not healthy.
Stop the process using 127.0.0.1:$port (or change port in config.toml) and re-run Setup.
This check does not modify other services or SRBMiner.
"@
    exit 1
}

Write-Host "Port $port is free on 127.0.0.1."
exit 0
