# Phase 2C-B — Linux systemd & Debian package

Localhost-only Linux x64 packaging for ExoGPUAgent `0.3.0`.

## What you get

| Artifact | Description |
|----------|-------------|
| `exogpuagent-v0.3.0-linux-x64-<sha>` | Portable `.tar.gz` only (bin + unit + scripts) |
| `exogpuagent-deb-v0.3.0-amd64-<sha>` | Dedicated DEB bundle for `apt install` (includes `build-meta.txt`) |
| `exogpuagent_0.3.0_amd64.deb` | Package file inside the **deb** artifact only |

GUI package is **not** included in 2C-B (see Phase 2C-C).

## Defaults

- Bind: `127.0.0.1:12001`
- Config (service): `/etc/exogpuagent/config.toml`
- Config (portable): `~/.config/exogpuagent/config.toml` (XDG via `directories`)
- Unit: `exogpuagent.service` · user/group: `exogpuagent` · start: enabled
- Logs: `journalctl -u exogpuagent`
- **No** firewall rules, Tailscale/LAN bind, NVIDIA/CUDA/Tailscale installers, cloud calls, or GUI

## glibc baseline

CI builds the Linux binary and `.deb` on **Ubuntu 22.04** (`ubuntu-22.04` runner).

| Item | Intent |
|------|--------|
| Target distros (to validate) | Ubuntu 22.04, Ubuntu 24.04, Debian 12 |
| Declared package Depends | `libc6 (>= 2.35)` unless newer GLIBC symbols are detected at pack time |
| Compatibility claim | **Not declared until lab validation** — check `glibc-baseline.txt` in the artifact |

Inspect symbols locally:

```bash
objdump -T /usr/bin/exogpuagent | grep -oE 'GLIBC_[0-9.]+' | sort -u
ldd --version
```

## Install (DEB)

```bash
# Verify checksum from the artifact SHA256SUMS.txt first
sudo apt install ./exogpuagent_0.3.0_amd64.deb
exogpuagent doctor --machine
exogpuagent status --machine
curl -sS http://127.0.0.1:12001/health
curl -sS http://127.0.0.1:12001/gpu
```

The package:

1. Creates system user/group `exogpuagent`
2. Copies `/usr/share/exogpuagent/config.toml.default` → `/etc/exogpuagent/config.toml` **only if absent**
3. Enables and starts `exogpuagent.service`

## Upgrade

```bash
sudo apt install ./exogpuagent_0.3.0_amd64.deb   # or a newer .deb
```

Existing `/etc/exogpuagent/config.toml` is **not** overwritten.  
`prerm` does **not** stop the unit on upgrade; `postinst` runs `systemctl try-restart` when the service was already active (avoids unnecessary downtime).

## Remove vs purge

| Command | Service/unit/binary | `/etc/exogpuagent` | `/var/lib/exogpuagent` |
|---------|---------------------|--------------------|------------------------|
| `sudo apt remove exogpuagent` | removed | **kept** | **kept** |
| `sudo apt purge exogpuagent` | removed | **deleted** | **deleted** |

System user/group is left in place after purge.

## Portable tarball (no apt)

```bash
tar -xzf exogpuagent-v0.3.0-linux-x64-*.tar.gz
sudo ./exogpuagent service install   # writes unit under /etc/systemd/system if no DEB unit
sudo ./exogpuagent service start
```

Prefer the `.deb` on production hosts.

## CLI

```text
exogpuagent run [--config PATH] [--machine]
exogpuagent status [--portable]
exogpuagent doctor [--portable]
exogpuagent version
exogpuagent config path|show|set [--machine|--portable] [key=value ...]
exogpuagent service install|uninstall|start|stop|restart
```

HTTP API unchanged: `GET /health`, `GET /gpu`.

`doctor` treats missing NVIDIA/NVML as a soft note (service may still run; `/gpu` returns 503). Hard failures are invalid config / bind policy.

## Validation script

```bash
sudo ./scripts/linux/Validate-LinuxInstall.sh --redact --out /tmp/ega-report.md
# Keep the report local — do not commit host-specific output to the public repo.
```

## Lab checklist (compatibility not yet claimed)

- [ ] Ubuntu 22.04 clean install
- [ ] Ubuntu 24.04 clean install
- [ ] Debian 12 clean install
- [ ] 0 / 1 / multi-GPU
- [ ] NVML absent
- [ ] Coexistence with CUDA / mining workloads
- [ ] Reboot autostart
- [ ] Upgrade preserves config
- [ ] remove vs purge
- [ ] Listen only on 127.0.0.1
- [ ] No graphical shared libraries in `ldd`

## Limitations / risks

- NVML device node permissions under user `exogpuagent` may need `video`/`render` groups or udev ACLs on some hosts — validate before production.
- `ProtectSystem=strict` is enabled; if NVML dlopen fails only under systemd, relax `DeviceAllow=` / groups carefully (never add NVIDIA installer hooks).
- Unsigned packages may require `apt install ./file.deb` trust confirmation.
- Phase 2C-B rejects non-localhost binds (`0.0.0.0`, LAN, Tailscale modes).
- Mini GUI is deferred to Phase 2C-C.
