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
| `exogpuagent-v0.3.0-linux-x64-<sha>` | Portable `.tar.gz` only (bin + unit + default config + validate script) |
| `exogpuagent-deb-v0.3.0-amd64-<sha>` | `exogpuagent_0.3.0_amd64.deb`, `build-meta.txt`, `glibc-baseline.txt`, docs, `SHA256SUMS.txt` |

### Linux: where is the `.deb`?

Artifacts are **separate** on purpose:

1. **`exogpuagent-deb-v0.3.0-amd64-<sha>`** — download this for `apt install ./exogpuagent_0.3.0_amd64.deb`.  
2. **`exogpuagent-v0.3.0-linux-x64-<sha>`** — portable tarball only; **does not** include the `.deb`.

The workflow **fails** (`::error::`) if:

- `dpkg-deb` does not produce a non-empty `.deb`
- `dpkg-deb -I` / contents check fails
- `build-meta.txt` is missing
- staging or pre-upload verification fails
- smoke lifecycle cannot find the package

Linux jobs run on **`ubuntu-22.04`** so the glibc baseline stays at 2.35-class for Ubuntu 22.04 / Debian 12 testing. Compatibility is **not claimed** until lab validation.

### Verify

```powershell
Get-FileHash -Algorithm SHA256 .\ExoGPUAgent-Setup-v0.3.0-win64.exe
Get-Content .\SHA256SUMS.txt
```

```bash
# From the dedicated DEB artifact:
sha256sum -c SHA256SUMS.txt
cat build-meta.txt
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
3. Portable `.tar.gz` staging (no `.deb`)  
4. `packaging/linux/build-deb.sh` → `.deb` + `build-meta.txt` + SHA-256 (**required**)  
5. Integrity checks (`dpkg-deb -I` / contents) + `GITHUB_STEP_SUMMARY`  
6. Disposable DEB lifecycle smoke  
7. Pre-upload verification (DEB present; portable must **not** contain `.deb`)  
8. Upload portable + dedicated DEB artifacts (`if-no-files-found: error`)
