<#
.SYNOPSIS
  Post-install checks for ExoGPUAgent on Windows (localhost only).
  Does not stop SRBMiner or change GPU/miner settings.
#>
[CmdletBinding()]
param(
    [string]$HealthUrl = "http://127.0.0.1:12001/health",
    [string]$GpuUrl = "http://127.0.0.1:12001/gpu",
    [string]$SrbUrl = "http://127.0.0.1:21550/"
)

$ErrorActionPreference = "Continue"
Write-Host "== ExoGPUAgent service validation ==" -ForegroundColor Cyan

$exe = Get-Command exogpuagent.exe -ErrorAction SilentlyContinue
if (-not $exe) {
    $guess = Join-Path ${env:ProgramFiles} "ExoGPUAgent\exogpuagent.exe"
    if (Test-Path $guess) { $exePath = $guess } else { throw "exogpuagent.exe not found in PATH or Program Files" }
} else {
    $exePath = $exe.Source
}

Write-Host "Binary: $exePath"
& $exePath version
& $exePath doctor
& $exePath status

try {
    $h = Invoke-RestMethod -Uri $HealthUrl -TimeoutSec 5
    Write-Host ("/health => " + ($h | ConvertTo-Json -Compress)) -ForegroundColor Green
} catch {
    Write-Host "/health FAILED: $_" -ForegroundColor Red
}

try {
    $g = Invoke-RestMethod -Uri $GpuUrl -TimeoutSec 5
    Write-Host ("/gpu gpu_count=" + $g.gpu_count + " total_power=" + $g.total_gpu_power_w) -ForegroundColor Green
} catch {
    Write-Host "/gpu FAILED: $_" -ForegroundColor Red
}

try {
    $s = Invoke-RestMethod -Uri $SrbUrl -TimeoutSec 5
    Write-Host ("SRBMiner still up version=" + $s.miner_version + " workers=" + $s.total_gpu_workers) -ForegroundColor Green
} catch {
    Write-Host "SRBMiner check skipped/failed: $_" -ForegroundColor Yellow
}

Get-Service -Name ExoGPUAgent -ErrorAction SilentlyContinue | Format-List Name, Status, StartType
