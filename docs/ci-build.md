# CI build (GitHub Actions)

Manual workflow: **Windows x64** + **Linux x64 (ubuntu-22.04)** on GitHub-hosted runners. No GPU required at compile time.

## Trigger

1. https://github.com/ExoGridTech/ExoGPUAgent/actions/workflows/build.yml  
2. **Run workflow** → `main`  
3. Wait for green jobs

Permissions: `contents: read` only. No secrets. No GitHub Releases.

## Artifacts (v0.3.0+)

| Artifact name pattern | Contents |
|----------------------|----------|
| `exogpuagent-v0.3.0-windows-x64-<sha>` | Portable `exogpuagent.exe`, README, docs, `SHA256SUMS.txt` |
| `ExoGPUAgent-Setup-v0.3.0-win64-<sha>` | `ExoGPUAgent-Setup-v0.3.0-win64.exe`, README, docs, `SHA256SUMS.txt` |
| `exogpuagent-v0.3.0-linux-x64-<sha>` | `.tar.gz` (bin + unit + default config + validate script) + checksum |
| `exogpuagent_0.3.0_amd64-<sha>` | `exogpuagent_0.3.0_amd64.deb`, `glibc-baseline.txt`, docs, `SHA256SUMS.txt` |

Linux jobs run on **`ubuntu-22.04`** so the glibc baseline stays at 2.35-class for Ubuntu 22.04 / Debian 12 testing. Compatibility is **not claimed** until lab validation.

### Verify

```powershell
Get-FileHash -Algorithm SHA256 .\ExoGPUAgent-Setup-v0.3.0-win64.exe
Get-Content .\SHA256SUMS.txt
```

```bash
sha256sum -c SHA256SUMS.txt
sudo apt install ./exogpuagent_0.3.0_amd64.deb
```

## Pipeline steps

### Windows

1. `fmt --check` · `clippy -D warnings` · `test` · `build --release`  
2. Package portable zip-style directory  
3. Inno Setup → Setup.exe artifact

### Linux

1. Same Rust checks on `ubuntu-22.04`  
2. Record `glibc-baseline.txt` (`ldd` + `GLIBC_*` symbols)  
3. Portable `.tar.gz`  
4. `packaging/linux/build-deb.sh` → `.deb` + SHA-256  
5. Disposable DEB lifecycle smoke (`scripts/linux/smoke-deb-lifecycle.sh` on the runner)
