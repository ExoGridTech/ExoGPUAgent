# Phase 2C-C2 — Local Mini GUI (v0.4.0)

Optional loopback admin UI. **Disabled by default.** No remote / VPN / LAN enablement.

## Defaults

| Surface | Address |
|---------|---------|
| Telemetry | `127.0.0.1:12001` (`/health`, `/gpu`) |
| Admin GUI | `127.0.0.1:12010` (only if `gui.enabled=true` + `admin_auth.toml`) |

## Recommended enable

### Windows (fresh install)

Inno Setup shows an optional Mini GUI page (skipped on upgrades). Choose **yes**, set password + confirmation (min 12). The installer bootstraps via `--password-stdin` (password never on argv), enables the GUI only after success, and keeps the secret out of ProgramData.

A short-lived file under Setup `{tmp}` may be used with a restrictive ACL (current user + SYSTEM). Cleanup on success/error/cancel is **best-effort overwrite + delete**, not a forensic secure erase.

After install (or if you skipped the page):

```text
exogpuagent gui setup
```

(Start Menu → **ExoGPUAgent Mini GUI Setup**)

### Linux (DEB / systemd)

APT/DEB scripts stay non-interactive (headless-safe). After install:

```bash
sudo exogpuagent gui setup
```

The wizard prompts to enable the GUI, asks for a hidden password + confirmation, writes machine credentials, sets `gui.enabled=true`, and restarts the service.

### Scripts / CI (non-interactive)

`--password-stdin` reads **exactly one line** from stdin (no prompt text on stdout):

```bash
printf '%s\n' 'your-long-password' | sudo exogpuagent gui init --password-stdin
sudo exogpuagent config set --machine gui.enabled=true
sudo systemctl restart exogpuagent   # or: exogpuagent service restart
```

Avoid `--password` on the command line (visible in process listings).

### Credentials only

```bash
# Interactive (TTY): hidden password + confirmation; does not enable the listener
sudo exogpuagent gui init
```

## Fresh install vs upgrade

| Path | Behavior |
|------|----------|
| Windows Inno **fresh** | Optional wizard: enable + password + confirm; `gui.enabled=true` only after bootstrap succeeds |
| Windows Inno **upgrade** | GUI page skipped when `config.toml` exists; config/auth never overwritten or re-enabled |
| Linux DEB | Default `gui.enabled=false`; no interactive APT hooks; upgrades preserve `/etc/exogpuagent/*` |
| Portable `--portable` | Explicit flag for per-user credential path |

Upgrades **never** set `gui.enabled=true` automatically and **never** change passwords.

## Security (this build)

- No default password; authentication required before admin APIs
- Argon2id hash in `admin_auth.toml`
  - Linux machine (`/etc/exogpuagent/`): `0640` `root:exogpuagent`
  - Portable / other Unix: `0600`
  - Windows: ProgramData ACLs (`NT AUTHORITY\LOCAL SERVICE` Modify via installer)
- Session cookie: `HttpOnly; SameSite=Strict`; absolute TTL 12h / idle 2h
- CSRF + `X-Requested-With` on mutating admin APIs
- Host / Origin allowlist for loopback GUI port
- Login rate limit (5 / 5 min)
- Admin routes never mounted on the telemetry listener (12001)

## Out of scope (2C-C3+)

Network Discovery UI, remote telemetry bind, admin remote access.
