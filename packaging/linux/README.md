# Linux packaging (Phase 2C-B)

| File | Role |
|------|------|
| `exogpuagent.service` | systemd unit (also embedded for portable `service install`) |
| `config.toml.default` | First-install template → `/etc/exogpuagent/config.toml` if absent |
| `debian/*` | Maintainer scripts + changelog/copyright |
| `build-deb.sh` | Build `exogpuagent_VERSION_amd64.deb` from a release binary |

```bash
cargo build --release
./packaging/linux/build-deb.sh
# → dist/deb/exogpuagent_0.3.0_amd64.deb
```

Build on **Ubuntu 22.04** (or equivalent glibc 2.35+) for the supported baseline. CI uses `ubuntu-22.04`.

Does **not** install NVIDIA drivers, CUDA, Tailscale, or GUI components.
