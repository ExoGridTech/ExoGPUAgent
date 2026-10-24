# Phase 2C-C2 — Local Mini GUI (v0.4.0)

Optional loopback admin UI. **Disabled by default.** No remote / VPN / LAN enablement.

## Defaults

| Surface | Address |
|---------|---------|
| Telemetry | `127.0.0.1:12001` (`/health`, `/gpu`) |
| Admin GUI | `127.0.0.1:12010` (only if `gui.enabled=true` + `admin_auth.toml`) |

## Enable (lab / service)

`gui init` and `gui url` default to the **machine** config path
(`ProgramData` / `/etc/exogpuagent`), not the interactive user profile.

```bash
# 1) Bootstrap credentials (min 12 chars) — does not enable the listener
#    Writes admin_auth.toml beside machine config (0640 root:exogpuagent on Linux).
printf '%s\n' 'your-long-password' | sudo exogpuagent gui init --password-stdin

# 2) Enable GUI in config (restart required)
sudo exogpuagent config set --machine gui.enabled=true

# 3) Restart service / process
# Windows: exogpuagent service restart
# Linux:   sudo systemctl restart exogpuagent

# 4) Open UI
exogpuagent gui url
# → http://127.0.0.1:12010/
```

### Fresh install vs upgrade

| Path | Behavior |
|------|----------|
| Windows Inno **fresh** | Optional wizard page to enable Mini GUI + set password; writes `[gui]` + `admin_auth.toml` only if chosen |
| Windows Inno **upgrade** | GUI page skipped when `config.toml` already exists; config/auth never overwritten |
| Linux DEB | Default `gui.enabled=false`; upgrades preserve `/etc/exogpuagent/*` including `admin_auth.toml` |
| Portable `--portable` | Explicit flag required for per-user credential path |

Upgrades **never** set `gui.enabled=true` automatically.

## Security (this build)

- Argon2id password hash in `admin_auth.toml`
  - Linux machine (`/etc/exogpuagent/`): `0640` `root:exogpuagent` (service can read; not world-readable)
  - Portable / other Unix paths: `0600`
  - Windows: ProgramData ACLs (`NT AUTHORITY\LOCAL SERVICE` Modify via installer)
- Session cookie: `HttpOnly; SameSite=Strict`; absolute TTL 12h / idle 2h
- CSRF + `X-Requested-With` on mutating admin APIs
- Host / Origin allowlist for loopback GUI port
- Login rate limit (5 / 5 min)
- Admin routes never mounted on the telemetry listener (12001)

## Out of scope (2C-C3+)

Network Discovery UI, remote telemetry bind, admin remote access.
