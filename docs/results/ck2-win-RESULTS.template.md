# CK2 Win — results template (fill on-box)

**Date / operator:**  
**Driver version:** (`nvidia-smi` header)  
**exogpuagent version:** (`exogpuagent.exe --version` or Cargo package 0.1.0)  
**Config path:**  
**bind / port:** `127.0.0.1` / `12001`

## Endpoints

| Endpoint | HTTP | Body summary |
|----------|------|--------------|
| `/health` | | |
| `/gpu` | | |

## Detection

| Field | Agent | nvidia-smi | SRBMiner |
|-------|-------|------------|----------|
| GPU count | | | |
| Name | | | |
| PCI | | `…` → short | `0000:01:00.0` |

## Sensors (near-simultaneous)

| Metric | Agent | nvidia-smi | Δ | SRBMiner | Notes |
|--------|-------|------------|---|----------|-------|
| temperature_c | | | | | |
| power_w | | | | asic_power | |
| utilization_pct | | | | (n/a or HR) | |
| fan_pct | | | | fan_speed_percent | |

## Resources

| Sample | WS MB | Private MB | CPU |
|--------|-------|------------|-----|
| Idle 30s | | | |
| 1 Hz poll 60s | | | |

## SRBMiner interference

| Check | Before | After | OK? |
|-------|--------|-------|-----|
| hashrate 1min | | | |
| hashrate 15min | | | |
| shares.accepted | | | |
| mining_time continuous | | | |
| process / API up | | | |

## Tailscale exposure

| Probe | Result |
|-------|--------|
| `curl http://<TAILSCALE_CGNAT_IP>:12001/health` from another host | Must fail (timeout/refused) |

## Issues / limitations observed

1.  
2.  

## Verdict

- [ ] PASS — proceed toward installers  
- [ ] REVIEW — see notes  
- [ ] FAIL — block Phase 2  

**Recommendation:**
