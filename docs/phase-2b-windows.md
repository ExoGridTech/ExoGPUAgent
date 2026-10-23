# Phase 2B — Windows service & installer

Localhost-only Windows x64 packaging for ExoGPUAgent (`0.3.0` crate; installer version follows `Cargo.toml` / Inno `#define`).

## What you get

| Artifact | Description |
|----------|-------------|
| `exogpuagent.exe` (portable) | Console binary with CLI subcommands |
| `ExoGPUAgent-Setup-v0.3.0-win64.exe` | Inno Setup installer + Windows service |

## Defaults

- Bind: `127.0.0.1:12001`
- Config (service): `%ProgramData%\ExoGridTech\ExoGPUAgent\config.toml`
- Config (portable): `%APPDATA%\ExoGridTech\ExoGPUAgent\config.toml`
- Service name: `ExoGPUAgent` · account: `NT AUTHORITY\LocalService` · start: Automatic
- **No** firewall rules, Tailscale/LAN bind, NVIDIA/CUDA/Tailscale installers, or cloud calls

## CLI

```text
exogpuagent run [--config PATH] [--machine]
exogpuagent status [--config PATH] [--portable]
exogpuagent doctor [--config PATH] [--portable]
exogpuagent version
exogpuagent service install|uninstall|start|stop|restart
```

HTTP API unchanged: `GET /health`, `GET /gpu`.

## Install (CK2 Win) without touching SRBMiner

1. Download the Setup artifact from GitHub Actions (manual workflow).
2. Verify SHA-256 against `SHA256SUMS.txt`.
3. Run Setup **as Administrator**. Leave SRBMiner running.
4. Confirm wizard keeps localhost / port `12001`.
5. After install:

```powershell
exogpuagent doctor
exogpuagent status
Invoke-RestMethod http://127.0.0.1:12001/health
Invoke-RestMethod http://127.0.0.1:12001/gpu
# optional: existing lab script against localhost only
```

6. Confirm SRBMiner API still healthy on `127.0.0.1:21550` (hashrate/shares uninterrupted).
7. Do **not** open port `12001` on Tailscale.

## Upgrade

Re-run Setup. Existing `config.toml` is **not** overwritten. Service is re-registered/restarted.

## Uninstall

Windows Settings → Apps → ExoGPUAgent → Uninstall.  
You will be asked whether to keep ProgramData config/logs. Service is always removed.

## Build installer locally (Windows)

```powershell
cargo build --release
# Inno Setup 6 installed
& "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" packaging\windows\exogpuagent.iss
# output: dist\windows-installer\ExoGPUAgent-Setup-v0.3.0-win64.exe
```

## Service diagnostics

Persistent log (LocalService-writable under ProgramData):

`%ProgramData%\ExoGridTech\ExoGPUAgent\logs\service.log`

The service reports SCM `Running` only after the HTTP listener is bound. If `/health` fails, read this log before changing the service account.

## Limitations / risks

- Unsigned installer may trigger SmartScreen.
- NVML under LocalService should be validated on CK2; escalate ACLs only if needed.
- NVML is initialized once per process; after installing a driver, **restart the service**.
- Phase 2B rejects non-localhost binds (`0.0.0.0`, LAN, Tailscale modes).
- `auth_token` field is reserved and never logged; remote auth not enabled.
