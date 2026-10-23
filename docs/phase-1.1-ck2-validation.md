# Phase 1.1 — Hardware validation (CK2 Win)

**Host:** lab Windows x64 · NVIDIA RTX 3090 (example target)  
**Tailscale (admin only):** `<TAILSCALE_CGNAT_IP>` — **do not** bind ExoGPUAgent to Tailscale.  
**SRBMiner:** 3.8.0 · API `127.0.0.1:21550`  
**ExoGPUAgent:** `127.0.0.1:12001` (localhost only)

## Scope check (repo)

| Repo | Expectation |
|------|-------------|
| `ExoGridTech/ExoGPUAgent` | All Phase 1 / 1.1 work stays here |
| `AtlasGPU` | **Do not modify** |

Phase 1.1 delivers procedures + scripts + a results template. No Windows service, installer, or auto-update.

---

## 0. Safety rules

1. Keep `bind = "127.0.0.1"` in `config.toml`.
2. Do **not** open firewall / Tailscale ACLs for port `12001`.
3. Do **not** stop or reconfigure SRBMiner for this test.
4. Run the agent as a foreground console process only.

---

## 1. Build on Windows (no service)

### Prerequisites

- [Rust toolchain](https://rustup.rs/) (stable, x86_64-pc-windows-msvc)
- Git
- NVIDIA Game Ready / Studio driver (NVML: `nvml.dll` next to the driver)

### Clone & build

```powershell
cd $env:USERPROFILE\Documents
git clone https://github.com/ExoGridTech/ExoGPUAgent.git
cd ExoGPUAgent
cargo test
cargo build --release
```

Binary: `target\release\exogpuagent.exe`

### Optional: local config (still localhost)

```powershell
$dir = Join-Path $env:APPDATA "ExoGridTech\ExoGPUAgent"
New-Item -ItemType Directory -Force -Path $dir | Out-Null
@'
bind = "127.0.0.1"
port = 12001
'@ | Set-Content -Encoding utf8 (Join-Path $dir "config.toml")
```

Or:

```powershell
.\target\release\exogpuagent.exe --config C:\path\to\config.toml
```

### Run (console)

```powershell
.\target\release\exogpuagent.exe
```

Expected stderr:

```text
NVML ready (1 GPU(s)); config ...
listening on http://127.0.0.1:12001
```

Leave this window open for the rest of the tests.

---

## 2. Automated validation script

On CK2, in a **second** PowerShell window:

```powershell
cd path\to\ExoGPUAgent
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\windows\Validate-ExoGPUAgent.ps1
```

The script:

- Calls `http://127.0.0.1:12001/health` and `/gpu`
- Runs `nvidia-smi` CSV query (same fields as the original Python agent)
- Calls `http://127.0.0.1:21550/` (SRBMiner JSON)
- Samples process CPU/WorkingSet for `exogpuagent`
- Writes `docs/results/ck2-win-YYYYMMDD-HHMMSS.json` and a short `.md` summary

Manual one-liners (if preferred):

```powershell
Invoke-RestMethod http://127.0.0.1:12001/health | ConvertTo-Json
Invoke-RestMethod http://127.0.0.1:12001/gpu    | ConvertTo-Json -Depth 5

nvidia-smi --query-gpu=index,name,pci.bus_id,power.draw,temperature.gpu,utilization.gpu,fan.speed --format=csv,noheader,nounits

Invoke-RestMethod http://127.0.0.1:21550/ | ConvertTo-Json -Depth 6
```

---

## 3. Pass / fail criteria

| Check | Pass if |
|-------|---------|
| NVML init | Console shows `NVML ready (1 GPU(s))` |
| `/health` | HTTP 200 · `status=ok` · `gpu_count=1` |
| `/gpu` | HTTP 200 · one entry · `name` contains `3090` |
| PCI | Agent `pci_address` = `01:00.0` (domain stripped; matches SRB `0000:01:00.0`) |
| vs `nvidia-smi` temp | \|Δ\| ≤ 3 °C on near-simultaneous samples |
| vs `nvidia-smi` power | \|Δ\| ≤ 10 W (NVML milliwatts → watts) |
| vs `nvidia-smi` util | Informative; mining util can jump (±15 pp OK to note) |
| vs `nvidia-smi` fan | \|Δ\| ≤ 5 pp when both non-N/A |
| vs SRBMiner | Same PCI topology; temp/power/fan in the same ballpark — **not** bit-identical (different sampler / cadence) |
| Resources | Steady Working Set ≲ 30 MB; CPU usually &lt; 1% when idle between polls |
| SRBMiner impact | Hashrate 1m/15m and accepted shares continue; no miner restart; API still responds |
| Exposure | `12001` **not** reachable via Tailscale IP from another host |

### Expected sampling differences (not failures)

- **SRBMiner `asic_power`** vs NVML `power_usage`: miner-reported board/ASIC estimate vs NVML instantaneous draw.
- **Timestamps:** poll agent and `nvidia-smi` within ~1–2 s; under load, util/power swing quickly.
- **Fan 100%:** normal under sustained mining load on many 3090 curves.

---

## 4. SRBMiner non-interference checklist

1. Before agent start: note hashrate `1min` / `15min` and `shares.accepted` from `http://127.0.0.1:21550/`.
2. Start agent; poll `/gpu` every 2 s for 2–3 minutes.
3. After: same SRBMiner fields — hashrate should stay in normal variance; `accepted` only increases; `mining_time` continuous; process still alive.
4. Confirm SRBMiner CLI args / config files unchanged.

---

## 5. Resource measurement

```powershell
Get-Process exogpuagent |
  Select-Object Name, Id, CPU,
    @{n='WS_MB';e={[math]::Round($_.WorkingSet64/1MB,2)}},
    @{n='PM_MB';e={[math]::Round($_.PrivateMemorySize64/1MB,2)}}
```

Sample once at idle (no HTTP polls for 30 s) and once while looping `Invoke-RestMethod` every second for 60 s.

---

## 6. Confirm localhost-only (from another Tailscale host)

```bash
curl -m 3 http://<TAILSCALE_CGNAT_IP>:12001/health
# Expected: connection timeout / refused — NOT 200
```

Agent must remain on `127.0.0.1` only.

---

## 7. Results & go / no-go

Fill `docs/results/ck2-win-RESULTS.md` (template below) or keep the JSON the script emits.

**Go to installers (Phase 2)** only if:

- [ ] `/health` + `/gpu` OK on CK2
- [ ] PCI / name / count correct
- [ ] Sensors align with `nvidia-smi` within thresholds
- [ ] SRBMiner undisturbed
- [ ] Memory/CPU acceptable
- [ ] No Tailscale exposure of `12001`

---

## Example baseline shape (sanitized)

See [`results/srbminer-preagent.example.json`](results/srbminer-preagent.example.json) for the expected SRBMiner JSON fields.
Do **not** commit real Tailscale IPs, wallets, or live sensor dumps — keep those in local `docs/results/ck2-win-*.json` (gitignored).

| Field | Example expectation |
|-------|---------------------|
| Agent via Tailscale `:12001` | Timeout / refused |
| SRBMiner version | `3.8.0` |
| GPU | RTX 3090 class |
| PCI topology | `0000:01:00.0` → agent reports `01:00.0` |

> Full agent vs `nvidia-smi` numbers must be filled **on-box** with the PowerShell script; remote Tailscale cannot see localhost `12001`.
