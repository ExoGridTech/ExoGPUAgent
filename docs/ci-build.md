# CI build (GitHub Actions)

Manual workflow: **Windows x64** + **Linux x64** on GitHub-hosted runners. No GPU required at compile time.

## Trigger

1. https://github.com/ExoGridTech/ExoGPUAgent/actions/workflows/build.yml  
2. **Run workflow** → `main`  
3. Wait for green jobs

Permissions: `contents: read` only. No secrets. No GitHub Releases.

## Artifacts (v0.2.0+)

| Artifact name pattern | Contents |
|----------------------|----------|
| `exogpuagent-v0.2.0-windows-x64-<sha>` | Portable `exogpuagent.exe`, README, docs, `SHA256SUMS.txt` |
| `ExoGPUAgent-Setup-v0.2.0-win64-<sha>` | `ExoGPUAgent-Setup-v0.2.0-win64.exe`, README, docs, `SHA256SUMS.txt` |
| `exogpuagent-v0.2.0-linux-x64-<sha>` | `.tar.gz` (preserves `+x`) + checksum of the archive |

### Verify

```powershell
Get-FileHash -Algorithm SHA256 .\ExoGPUAgent-Setup-v0.2.0-win64.exe
Get-Content .\SHA256SUMS.txt
```

```bash
sha256sum -c SHA256SUMS.txt
tar -xzf exogpuagent-v*-linux-x64-*.tar.gz
```

## CK2 Win

Prefer the **Setup** artifact for service install, or portable `exogpuagent.exe run` for a quick console test. Always `127.0.0.1:12001`. See [phase-2b-windows.md](phase-2b-windows.md).

## Pipeline steps

1. `fmt --check` · `clippy -D warnings` · `test` · `build --release`  
2. Package portable artifact  
3. Windows only: Inno Setup → Setup.exe artifact (`if-no-files-found: error`)
