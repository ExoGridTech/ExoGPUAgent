# ExoGPUAgent

Lightweight, open-source NVIDIA GPU telemetry agent for Windows and Linux.
Privacy-first, local by default. No cloud dependencies or telemetry collection.

## Status

Phase 1 (Rust core): local HTTP agent exposing `/health` and `/gpu` via NVML.

## Requirements

- Rust 1.75+ (edition 2021)
- NVIDIA driver with NVML (`libnvidia-ml` / `nvml.dll`) for live telemetry
- Tests run without NVIDIA hardware (mock collector)

## Quick start

```bash
cargo build --release
./target/release/exogpuagent
# optional: ./target/release/exogpuagent --config /path/to/config.toml
```

Default listen address: `http://127.0.0.1:12001`

Config is created on first run under the platform config directory
(XDG on Linux/macOS, AppData on Windows), unless `--config` is set.

```toml
bind = "127.0.0.1"
port = 12001
```

## API (JSON contract)

- `GET /health` → `{"status":"ok","gpu_count":N}` or `503` `{"detail":"..."}`
- `GET /gpu` → `gpu_count`, `total_gpu_power_w`, `gpus[]` with per-sensor `null`s

## Hardware validation (Phase 1.1)

Windows lab procedure (RTX 3090 example): [docs/phase-1.1-ck2-validation.md](docs/phase-1.1-ck2-validation.md)

```powershell
cargo build --release
.\target\release\exogpuagent.exe
# second terminal:
.\scripts\windows\Validate-ExoGPUAgent.ps1
```

Keep `bind = "127.0.0.1"`. Do not expose port `12001` on Tailscale.

## CI binaries (Phase 1.2)

Manual GitHub Actions builds for Windows/Linux: [docs/ci-build.md](docs/ci-build.md)

Actions → **Build** → **Run workflow** → download the OS artifact (no GitHub Release yet).

## Develop

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

## License

MIT — see [LICENSE](LICENSE).
