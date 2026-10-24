# ExoGPUAgent

Lightweight, open-source NVIDIA GPU telemetry agent for Windows and Linux.
Privacy-first, local by default. No cloud dependencies or telemetry collection.

## Status

- **Phase 1:** Rust core — `/health` + `/gpu` via NVML  
- **Phase 1.1 / 1.2:** Hardware validation docs + GitHub Actions builds  
- **Phase 2B:** Windows service (SCM) + Inno Setup installer — localhost only  
- **Phase 2C-B:** Linux systemd + Debian package — localhost only  
- **Phase 2C-C2:** Optional local Mini GUI on `127.0.0.1:12010` (`0.4.0`, **off by default**)

## Requirements

- Rust 1.75+ to build from source (end users: use CI artifacts / Setup.exe / `.deb`)
- NVIDIA driver with NVML for live telemetry
- Tests run without NVIDIA hardware (mock collector)

## Quick start (portable)

```bash
cargo build --release
./target/release/exogpuagent run
# Windows: .\target\release\exogpuagent.exe run
```

Default listen address: `http://127.0.0.1:12001`

```toml
host_name = ""
bind = "127.0.0.1"
port = 12001
access_mode = "localhost"
autostart = true
```

- Portable config: platform user config dir (`directories`)  
- Service config (Windows): `%ProgramData%\ExoGridTech\ExoGPUAgent\config.toml`  
- Service config (Linux): `/etc/exogpuagent/config.toml`

## CLI

```text
exogpuagent run [--config PATH] [--machine]
exogpuagent status [--portable]
exogpuagent doctor [--portable]
exogpuagent version
exogpuagent config path|show|set [--machine|--portable] [key=value ...]
exogpuagent service install|uninstall|start|stop|restart
exogpuagent gui init|setup|url [--machine|--portable]
```

Mini GUI (optional, localhost `12010`): run `exogpuagent gui setup` after install, or see [docs/phase-2c-c2-gui.md](docs/phase-2c-c2-gui.md).

## API (JSON contract)

- `GET /health` → `{"status":"ok","gpu_count":N}` or `503` `{"detail":"..."}`
- `GET /gpu` → `gpu_count`, `total_gpu_power_w`, `gpus[]` with per-sensor `null`s

## Windows installer (Phase 2B)

See [docs/phase-2b-windows.md](docs/phase-2b-windows.md).

## Linux package (Phase 2C-B)

See [docs/phase-2c-linux.md](docs/phase-2c-linux.md).

```bash
sudo apt install ./exogpuagent_0.3.0_amd64.deb
```

CI (manual): [docs/ci-build.md](docs/ci-build.md) — Windows Setup/portable + Linux `.tar.gz` + `.deb`.

Keep `bind = "127.0.0.1"`. Do not expose port `12001` on Tailscale.

## Hardware validation

[docs/phase-1.1-ck2-validation.md](docs/phase-1.1-ck2-validation.md)

## Develop

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

## License

MIT — see [LICENSE](LICENSE).
