# CI build (GitHub Actions)

Manual workflow that builds **Windows x64** and **Linux x64** release binaries on GitHub-hosted runners. No GPU/driver is required at compile time (`nvml-wrapper` loads NVML at runtime).

## Trigger

1. Open https://github.com/ExoGridTech/ExoGPUAgent/actions/workflows/build.yml  
2. **Run workflow** → branch `main` → **Run workflow**.  
3. Wait for both jobs (`Windows x64`, `Linux x64`) to finish green.

Only `workflow_dispatch` is enabled (no push/PR triggers yet). Permissions: `contents: read` only. No secrets, no Releases.

## Download artifacts

1. Open the completed workflow run.  
2. Under **Artifacts**, download e.g.  
   - `exogpuagent-v0.1.0-windows-x64-<sha>`  
   - `exogpuagent-v0.1.0-linux-x64-<sha>`  

### Windows artifact (zip from GitHub)

| File | Purpose |
|------|---------|
| `exogpuagent.exe` | Stable name for scripts |
| `exogpuagent-v0.1.0-windows-x64.exe` | Versioned copy |
| `README.md` | Usage |
| `SHA256SUMS.txt` | SHA-256 of the three files above |

```powershell
Get-FileHash -Algorithm SHA256 .\exogpuagent.exe
Get-Content .\SHA256SUMS.txt
```

### Linux artifact (zip from GitHub containing a `.tar.gz`)

| File | Purpose |
|------|---------|
| `exogpuagent-v0.1.0-linux-x64-<sha>.tar.gz` | Redistributable archive (**preserves `+x`**) |
| `SHA256SUMS.txt` | SHA-256 of that `.tar.gz` |

Inside the tarball:

| File | Purpose |
|------|---------|
| `exogpuagent` | Executable (`0755`) |
| `exogpuagent-v0.1.0-linux-x64` | Versioned executable |
| `README.md` | Usage |
| `SHA256SUMS.txt` | SHA-256 of the files inside the archive |

```bash
sha256sum -c SHA256SUMS.txt          # verifies the .tar.gz
tar -xzf exogpuagent-v*-linux-x64-*.tar.gz
cd … && sha256sum -c SHA256SUMS.txt  # verifies extracted files
./exogpuagent
```

## CK2 Win (localhost validation)

```powershell
# after unzipping the Windows artifact
.\exogpuagent.exe
# expect: listening on http://127.0.0.1:12001
```

Keep bind on loopback. Do not expose port `12001` on Tailscale. NVIDIA driver must be installed on the host for live `/gpu` data.

## What the workflow runs

On each OS:

1. `cargo fmt --all -- --check`  
2. `cargo clippy --all-targets -- -D warnings`  
3. `cargo test --all-targets`  
4. `cargo build --release`  
5. Package + `upload-artifact` with `if-no-files-found: error` (14-day retention)

## Out of scope (this phase)

- GitHub Releases / tags  
- macOS builds  
- Installers / Windows services  
- Automatic builds on every push  
