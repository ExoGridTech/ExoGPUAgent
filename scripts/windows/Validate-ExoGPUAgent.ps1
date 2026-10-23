<#
.SYNOPSIS
  Phase 1.1 hardware validation for ExoGPUAgent on Windows (localhost only).

.DESCRIPTION
  Compares ExoGPUAgent (/health, /gpu) against nvidia-smi and SRBMiner API.
  Does not change firewall, Tailscale, or SRBMiner configuration.

.PARAMETER AgentBase
  ExoGPUAgent base URL. Default: http://127.0.0.1:12001

.PARAMETER SrbBase
  SRBMiner API base URL. Default: http://127.0.0.1:21550

.PARAMETER OutDir
  Directory for JSON/MD result files. Default: docs/results under repo root.
#>
[CmdletBinding()]
param(
    [string]$AgentBase = "http://127.0.0.1:12001",
    [string]$SrbBase = "http://127.0.0.1:21550",
    [string]$OutDir = ""
)

$ErrorActionPreference = "Stop"

function Get-RepoRoot {
    $here = $PSScriptRoot
    return (Resolve-Path (Join-Path $here "..\..")).Path
}

function Invoke-JsonGet([string]$Url) {
    try {
        return Invoke-RestMethod -Uri $Url -TimeoutSec 5
    } catch {
        return $null
    }
}

function Get-NvidiaSmiRows {
    $query = "index,name,pci.bus_id,power.draw,temperature.gpu,utilization.gpu,fan.speed"
    $exe = Join-Path $env:SystemRoot "System32\nvidia-smi.exe"
    if (-not (Test-Path $exe)) { $exe = "nvidia-smi" }
    $raw = & $exe "--query-gpu=$query" "--format=csv,noheader,nounits" 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "nvidia-smi failed: $raw"
    }
    $rows = @()
    foreach ($line in ($raw | Out-String).Trim().Split("`n")) {
        $parts = $line.Trim() -split ",\s*"
        if ($parts.Count -lt 7) { continue }
        $na = { param($v) if ($v -in @("N/A", "[N/A]", "")) { $null } else { $v } }
        $pci = & $na $parts[2]
        if ($pci -and ($pci.ToString().Split(":").Count -ge 3)) {
            $pci = ($pci.ToString().Split(":", 2)[1]).ToUpper()
        } elseif ($pci) {
            $pci = $pci.ToString().ToUpper()
        }
        $rows += [pscustomobject]@{
            index            = [int]$parts[0]
            name             = $parts[1].Trim()
            pci_address      = $pci
            power_w          = $(if ($null -eq (& $na $parts[3])) { $null } else { [double]$parts[3] })
            temperature_c    = $(if ($null -eq (& $na $parts[4])) { $null } else { [int]$parts[4] })
            utilization_pct  = $(if ($null -eq (& $na $parts[5])) { $null } else { [int]$parts[5] })
            fan_pct          = $(if ($null -eq (& $na $parts[6])) { $null } else { [int]$parts[6] })
        }
    }
    return $rows
}

function Get-ProcessStats([string]$Name) {
    $procs = Get-Process -Name $Name -ErrorAction SilentlyContinue
    if (-not $procs) { return $null }
    $p = $procs | Select-Object -First 1
    return [pscustomobject]@{
        id      = $p.Id
        cpu     = [math]::Round($p.CPU, 2)
        ws_mb   = [math]::Round($p.WorkingSet64 / 1MB, 2)
        pm_mb   = [math]::Round($p.PrivateMemorySize64 / 1MB, 2)
    }
}

$repo = Get-RepoRoot
if (-not $OutDir) {
    $OutDir = Join-Path $repo "docs\results"
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$jsonPath = Join-Path $OutDir "ck2-win-$stamp.json"
$mdPath = Join-Path $OutDir "ck2-win-$stamp.md"

Write-Host "== ExoGPUAgent Phase 1.1 validation ==" -ForegroundColor Cyan
Write-Host "Agent: $AgentBase"
Write-Host "SRBMiner: $SrbBase"

# --- SRBMiner before ---
$srbBefore = Invoke-JsonGet "$SrbBase/"
if (-not $srbBefore) {
    Write-Warning "SRBMiner API not reachable at $SrbBase (continue; mark as skipped)."
}

$health = Invoke-JsonGet "$AgentBase/health"
$gpu = Invoke-JsonGet "$AgentBase/gpu"
if (-not $health -or -not $gpu) {
    throw "ExoGPUAgent not reachable at $AgentBase. Start .\target\release\exogpuagent.exe first (localhost)."
}

$smi = Get-NvidiaSmiRows
$proc = Get-ProcessStats "exogpuagent"

Start-Sleep -Seconds 2
$srbAfter = Invoke-JsonGet "$SrbBase/"

# --- Comparisons ---
$agentGpu = $gpu.gpus[0]
$smiGpu = $smi | Where-Object { $_.index -eq $agentGpu.index } | Select-Object -First 1
if (-not $smiGpu) { $smiGpu = $smi | Select-Object -First 1 }

function DiffNum($a, $b) {
    if ($null -eq $a -or $null -eq $b) { return $null }
    return [math]::Round(([double]$a - [double]$b), 2)
}

$vsSmi = [pscustomobject]@{
    d_temp_c   = DiffNum $agentGpu.temperature_c $smiGpu.temperature_c
    d_power_w  = DiffNum $agentGpu.power_w $smiGpu.power_w
    d_util_pp  = DiffNum $agentGpu.utilization_pct $smiGpu.utilization_pct
    d_fan_pp   = DiffNum $agentGpu.fan_pct $smiGpu.fan_pct
    pci_agent  = $agentGpu.pci_address
    pci_smi    = $smiGpu.pci_address
}

$srbGpu = $null
$vsSrb = $null
if ($srbBefore -and $srbBefore.gpu_devices) {
    $srbGpu = $srbBefore.gpu_devices | Select-Object -First 1
    $vsSrb = [pscustomobject]@{
        d_temp_c  = DiffNum $agentGpu.temperature_c $srbGpu.temperature
        d_power_w = DiffNum $agentGpu.power_w $srbGpu.asic_power
        d_fan_pp  = DiffNum $agentGpu.fan_pct $srbGpu.fan_speed_percent
        pci_srb   = $srbGpu.topology_id
    }
}

$pass = @{
    health_ok        = ($health.status -eq "ok" -and $health.gpu_count -ge 1)
    gpu_count_match  = ($gpu.gpu_count -eq $smi.Count)
    name_has_3090    = ($agentGpu.name -match "3090")
    pci_normalized   = ($agentGpu.pci_address -match '^[0-9A-F]{2}:[0-9A-F]{2}\.[0-9A-F]$')
    pci_vs_smi       = ($agentGpu.pci_address -eq $smiGpu.pci_address)
    temp_vs_smi_ok   = ($null -eq $vsSmi.d_temp_c -or [math]::Abs($vsSmi.d_temp_c) -le 3)
    power_vs_smi_ok  = ($null -eq $vsSmi.d_power_w -or [math]::Abs($vsSmi.d_power_w) -le 10)
    fan_vs_smi_ok    = ($null -eq $vsSmi.d_fan_pp -or [math]::Abs($vsSmi.d_fan_pp) -le 5)
    mem_ok           = (-not $proc -or $proc.ws_mb -le 30)
    srb_alive        = [bool]$srbAfter
}

$allCritical = $pass.health_ok -and $pass.gpu_count_match -and $pass.pci_normalized -and `
    $pass.pci_vs_smi -and $pass.temp_vs_smi_ok -and $pass.power_vs_smi_ok -and $pass.fan_vs_smi_ok

$result = [ordered]@{
    timestamp_utc = (Get-Date).ToUniversalTime().ToString("o")
    host          = $env:COMPUTERNAME
    agent_base    = $AgentBase
    health        = $health
    gpu           = $gpu
    nvidia_smi    = $smi
    srbminer_before = $srbBefore
    srbminer_after  = $srbAfter
    process       = $proc
    vs_nvidia_smi = $vsSmi
    vs_srbminer   = $vsSrb
    pass          = $pass
    verdict       = $(if ($allCritical) { "PASS" } else { "REVIEW" })
}

$result | ConvertTo-Json -Depth 10 | Set-Content -Encoding utf8 $jsonPath

$md = @"
# CK2 Win validation — $stamp

**Verdict:** $($result.verdict)

## Agent
- health: ``$($health | ConvertTo-Json -Compress)``
- gpu_count: $($gpu.gpu_count)
- name: $($agentGpu.name)
- pci: $($agentGpu.pci_address)
- power_w / temp_c / util / fan: $($agentGpu.power_w) / $($agentGpu.temperature_c) / $($agentGpu.utilization_pct) / $($agentGpu.fan_pct)

## vs nvidia-smi
- Δtemp: $($vsSmi.d_temp_c) °C (≤3)
- Δpower: $($vsSmi.d_power_w) W (≤10)
- Δutil: $($vsSmi.d_util_pp) pp
- Δfan: $($vsSmi.d_fan_pp) pp (≤5)
- PCI agent/smi: $($vsSmi.pci_agent) / $($vsSmi.pci_smi)

## vs SRBMiner (informational)
$(if ($vsSrb) { "- Δtemp: $($vsSrb.d_temp_c) °C`n- Δpower: $($vsSrb.d_power_w) W (asic_power vs NVML)`n- Δfan: $($vsSrb.d_fan_pp) pp`n- PCI SRB: $($vsSrb.pci_srb)" } else { "- skipped (API unreachable on localhost)" })

## Process
$(if ($proc) { "- pid $($proc.id) · WS $($proc.ws_mb) MB · PM $($proc.pm_mb) MB · CPU(s) $($proc.cpu)" } else { "- exogpuagent process not found" })

## Pass flags
``````
$($pass | ConvertTo-Json)
``````

Raw JSON: ``$jsonPath``
"@
$md | Set-Content -Encoding utf8 $mdPath

Write-Host ""
Write-Host "Verdict: $($result.verdict)" -ForegroundColor $(if ($allCritical) { "Green" } else { "Yellow" })
Write-Host "Wrote $jsonPath"
Write-Host "Wrote $mdPath"
$pass.GetEnumerator() | ForEach-Object {
    $color = if ($_.Value) { "Green" } else { "Red" }
    Write-Host ("  {0}: {1}" -f $_.Key, $_.Value) -ForegroundColor $color
}

if (-not $allCritical) { exit 2 }
exit 0
