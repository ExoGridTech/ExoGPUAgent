# Post-start health probe using the configured localhost port.
$ErrorActionPreference = "Continue"

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
$ok = $false
$r = $null
for ($i = 0; $i -lt 20; $i++) {
    try {
        $r = Invoke-RestMethod -Uri $uri -TimeoutSec 2
        if ($r.status -eq "ok") {
            $ok = $true
            break
        }
    } catch {
        Start-Sleep -Seconds 1
    }
}

if (-not $ok) {
    Write-Host "WARNING: $uri not ready yet. Run: exogpuagent doctor"
    # Non-fatal: service may still be starting or NVML may return 503 later on /gpu.
    exit 0
}

Write-Host ("health ok on port {0}; gpu_count={1}" -f $port, $r.gpu_count)
exit 0
