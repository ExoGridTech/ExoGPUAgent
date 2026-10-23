# Fase 2A — Arquitectura: instaladores y servicios

**Estado:** diseño (sin implementación de código de servicio/instalador).  
**Alcance:** Windows Service nativo + Inno Setup · Linux systemd · config/seguridad · CI · plan de pruebas.  
**Fuera de alcance ahora:** Fase 2B/2C (código), GitHub Release, AtlasGPU, auto-update remoto, Tailscale install.

---

## 1. Auditoría del estado actual (Fase 1)

### Arquitectura Rust

| Pieza | Rol |
|-------|-----|
| `src/main.rs` | Proceso único: carga config → init NVML una vez → `axum` en Tokio `current_thread` |
| `src/config.rs` | TOML `bind` + `port`; default `127.0.0.1:12001`; path vía `directories` |
| `src/gpu/` | Trait `GpuCollector` + `NvmlCollector` + `MockCollector` |
| `src/http/` | Solo `GET /health` y `GET /gpu` (contrato AtlasGPU-compatible) |
| CLI | Solo `--config` y `--version` (clap) |

### Rutas de configuración hoy

| OS | Path actual (`ProjectDirs` io/ExoGridTech/ExoGPUAgent) |
|----|--------------------------------------------------------|
| Windows | `%APPDATA%\ExoGridTech\ExoGPUAgent\config.toml` (por usuario) |
| Linux | `~/.config/exogpuagent/config.toml` (por usuario) |

**Problema para servicios:** un servicio SYSTEM / `exogpuagent` no debe depender del perfil de un usuario interactivo. Fase 2 debe **separar**:

- **Config de máquina (servicio):** ruta fija del sistema.  
- **Config portable / CLI interactivo:** puede seguir usando AppData/XDG del usuario si se ejecuta en foreground.

### Limitaciones a resolver en 2B/2C

1. NVML init-once: si el driver aparece después del arranque, hace falta **reinicio del servicio** (o re-init controlada en 2.x — fuera del MVP 2B).  
2. No hay subcomandos `status` / `doctor` / `config`.  
3. Bind no-loopback solo emite warning; no hay modelo de acceso ni fail-closed.  
4. Logs solo a stderr (adecuado para journald/servicio si se redirige; Windows necesita Event Log o archivo).  
5. Binario = servidor HTTP; no hay modo “admin-only” separado del daemon.

### Reutilización (evitar duplicar Win/Linux)

Un solo crate binario con:

- Lógica compartida: config, doctor, health-check HTTP local, políticas de bind.  
- `#[cfg(windows)]` / `#[cfg(unix)]` solo para SCM / systemd helpers mínimos.  
- Scripts/instaladores delgados que llaman al mismo binario (`exogpuagent install-service`, etc.) en lugar de lógica duplicada en Inno/bash.

---

## 2. Decisiones técnicas propuestas

### 2.1 Windows Service — **nativo vía SCM** (sin NSSM)

| Opción | Veredicto |
|--------|-----------|
| **NSSM / WinSW** | Rechazado para producto: dependencia externa, superficie extra, menos control. |
| **`windows-service` crate** | **Elegido:** registro SCM, SERVICE_WIN32_OWN_PROCESS, arranque Automatic, cuenta Local Service o cuenta dedicada de bajo privilegio. |
| Servicio como wrapper PowerShell | Rechazado: frágil, sesión/perfil, peor para “sin login”. |

**Comportamiento del proceso:**

- Misma lógica HTTP que hoy.  
- Entrypoint: si `exogpuagent run` / servicio → daemon; subcomandos admin no abren el puerto (salvo `doctor` que solo hace GET local).  
- Stop: graceful shutdown (señal de servicio → mismo path que Ctrl+C).  
- Working directory: dir de instalación.  
- Config: `C:\ProgramData\ExoGridTech\ExoGPUAgent\config.toml` (máquina).  
- Logs: archivo rotativo bajo `ProgramData\...\logs\` + opcional Event Log (fase posterior si hace falta).

**Cuenta de servicio (MVP 2B):** `NT AUTHORITY\LOCAL SERVICE` con ACL de lectura en ProgramData y ejecución del binario. Evitar SYSTEM salvo que NVML lo exija en algún edge case (validar en CK2).

### 2.2 Windows installer — **Inno Setup 6**

| Opción | Veredicto |
|--------|-----------|
| **Inno Setup** | **Elegido:** maduro, silent install, uninstall registry, custom Pascal script para puerto/NVML check, conserva `config.toml` en upgrade. |
| MSI (WiX) | Más pesado para v0.2; reconsiderar si enterprise lo exige. |
| Solo zip portable | Se mantiene como artifact paralelo; no sustituye el Setup. |

**Flujo Setup:**

1. Detectar x64 / Win10+.  
2. Probe NVML (`nvml.dll` / init ligero vía `exogpuagent doctor --preinstall` o check de archivo). **No** instalar drivers.  
3. Elegir puerto (default 12001) y modo acceso (default localhost).  
4. Escribir config en ProgramData **solo si no existe** (upgrade preserva).  
5. Copiar `exogpuagent.exe` a `Program Files\ExoGPUAgent\`.  
6. `sc create` / API vía binario `exogpuagent service install`.  
7. Start service + `GET http://127.0.0.1:PORT/health`.  
8. Uninstaller: stop → delete service → remove files; checkbox “eliminar configuración y logs”.

### 2.3 Linux — **systemd user dedicado**

| Pieza | Propuesta |
|-------|-----------|
| Binario | `/usr/local/bin/exogpuagent` (o `/usr/bin` si empaquetamos .deb más adelante) |
| Unit | `/etc/systemd/system/exogpuagent.service` |
| Usuario | `exogpuagent` (system user, nologin, sin shell) |
| Config | `/etc/exogpuagent/config.toml` |
| State/logs | journald (`StandardOutput=journal`) + opcional `/var/lib/exogpuagent/` |
| Installer | Script `install.sh` en el `.tar.gz` (MVP 2C); `.deb` opcional más tarde |

**Unit (conceptos):** `Type=simple`, `Restart=on-failure`, `NoNewPrivileges=true`, `ProtectSystem=strict`, `ProtectHome=true`, `ReadWritePaths=/etc/exogpuagent` (o solo config path), `PrivateTmp=true`. **No** tocar drivers/CUDA.

### 2.4 CLI unificado (compartido)

```text
exogpuagent run                 # foreground / usado por servicio
exogpuagent service install|uninstall|start|stop|restart   # OS-specific
exogpuagent status              # servicio + /health local
exogpuagent doctor              # NVML, puerto, bind, Tailscale detect, sin detener servicio
exogpuagent config show|set|path
exogpuagent version
```

- Admin **solo CLI**, nunca endpoints HTTP de shell/control.  
- API pública permanece `/health` + `/gpu`.

### 2.5 Versionado

- Crate / productos Fase 2: **`0.2.0`**.  
- Artifacts: `ExoGPUAgent-Setup-v0.2.0-win64.exe`, `ExoGPUAgent-v0.2.0-linux-x64.tar.gz`.

---

## 3. Modelo de configuración y seguridad de red

### 3.1 Schema propuesto (`config.toml`)

```toml
# Identidad (opcional; no se expone en /gpu todavía)
host_name = ""

# Red
port = 12001
# access_mode: "localhost" | "tailscale" | "lan"
access_mode = "localhost"
# bind se deriva de access_mode; no se recomienda editar a mano
# bind = "127.0.0.1"

# Solo si access_mode = "lan" (CIDR allowlist obligatoria)
# lan_bind = "192.168.1.10"   # IP de interfaz concreta, NUNCA 0.0.0.0
# lan_allow_cidrs = ["192.168.1.0/24"]

# Solo si access_mode = "tailscale"
# tailscale_bind = "100.x.y.z"  # resuelto al arrancar; si desaparece → fail closed a error de servicio

# Servicio
autostart = true

# Auth remota (obligatoria antes de habilitar tailscale/lan en “producción”)
# auth_token = ""   # ver §3.3
```

**Reglas fail-closed:**

| Regla | Comportamiento |
|-------|----------------|
| Default | Solo `127.0.0.1` |
| `0.0.0.0` / `::` / `*` | **Rechazar** al cargar config |
| Tailscale | Resolver interfaz/IP Tailscale al start; si no hay → log claro + **no** caer a LAN; permanecer en localhost **o** fallar el start según flag `require_bind` |
| LAN | Exige IP unicast de interfaz local + `lan_allow_cidrs` no vacío; sin allowlist → rechazo |
| Interfaz caída | No rebind silencioso a wildcard; error + restart policy |

**Detección Tailscale (solo lectura):**

- Windows: presencia de adapter “Tailscale” / `tailscale.exe status --json` si existe.  
- Linux: `tailscale0` o CLI.  
- **Nunca** instalar, `up`, login ni modificar ACLs.

### 3.2 Auth para modos remotos (decisión 2A)

| Enfoque | MVP |
|---------|-----|
| Sin auth en remoto | **No** permitido en builds “release” de instalador |
| Token estático Bearer / header `X-ExoGPUAgent-Token` | **MVP recomendado** para Tailscale/LAN |
| mTLS / OAuth | Post-2C |

Localhost: sin auth (compatible con collectors actuales en loopback).  
Remoto: token generado en install (`doctor` / setup); `/health` y `/gpu` exigen token si `access_mode != localhost`.

> **Compatibilidad AtlasGPU:** mientras el collector use solo localhost, no hay cambio de contrato. Remoto es opt-in explícito.

### 3.3 Preservación en updates

| Artefacto | Upgrade | Uninstall |
|-----------|---------|-----------|
| `config.toml` | Conservar si existe | Preguntar |
| Logs | Conservar | Preguntar |
| Binario / unit / servicio | Reemplazar | Eliminar siempre |
| Identidad (`host_name`, token) | Conservar | Según config |

---

## 4. Estructura de archivos propuesta (repo)

```text
ExoGPUAgent/
├── Cargo.toml                          # version 0.2.0 cuando arranque 2B
├── src/
│   ├── main.rs                         # clap subcommands → run | service | status | doctor | config
│   ├── lib.rs
│   ├── config.rs                       # schema ampliado + validación fail-closed
│   ├── net_policy.rs                   # NEW: access_mode, anti-0.0.0.0, Tailscale/LAN resolve
│   ├── doctor.rs                       # NEW: checks compartidos
│   ├── gpu/ …
│   ├── http/ …                         # + middleware auth opcional
│   └── service/                        # NEW
│       ├── mod.rs
│       ├── windows.rs                  # SCM via windows-service
│       └── linux.rs                    # helpers systemctl (CLI); unit file en packaging/
├── packaging/
│   ├── windows/
│   │   ├── exogpuagent.iss             # Inno Setup
│   │   └── ...
│   └── linux/
│       ├── exogpuagent.service
│       ├── install.sh
│       └── uninstall.sh
├── scripts/windows/Validate-*.ps1      # existente + ampliación post-install
├── docs/
│   ├── phase-2a-architecture.md        # este documento
│   ├── phase-2b-windows.md             # (2B)
│   └── phase-2c-linux.md               # (2C)
└── .github/workflows/build.yml         # ampliar en 2B/2C
```

**Dependencias nuevas (previstas, no añadidas aún):**

| Crate / tool | Uso |
|--------------|-----|
| `clap` (subcommands) | Ya existe; ampliar |
| `windows-service` | Solo target Windows |
| `tracing` + `tracing-subscriber` | Logs estructurados (reemplazar eprintln gradual) |
| Inno Setup 6 | CI Windows job |
| (opcional) `axum` middleware auth | Token remoto |

Sin AtlasGPU, sin cloud SDKs, sin auto-updater.

---

## 5. Flujo de vida (UX)

```text
Descargar artifact CI
    → Instalar (Setup.exe / install.sh)
        → Escribir config máquina (si no existe)
        → Registrar servicio
        → Start
        → Verificar GET /health en 127.0.0.1:12001
            → OK: listo
            → NVML ausente: servicio up, /gpu 503, doctor explica driver
```

Actualización manual:

```text
Stop servicio → reemplazar binario (hash) → Start → /health
    fallo → rollback binario anterior + Start
```

---

## 6. GitHub Actions (diseño; implementar en 2B/2C)

Seguir `workflow_dispatch` + `contents: read`.

| Artifact | Contenido |
|----------|-----------|
| `ExoGPUAgent-Setup-v0.2.0-win64.exe` | Inno + SHA256 |
| `exogpuagent-v0.2.0-windows-x64.zip` | Portable opcional |
| `ExoGPUAgent-v0.2.0-linux-x64.tar.gz` | bin + unit + install/uninstall + SHA256 |

Jobs: build Rust (como hoy) → job Inno (Windows) → empaquetado Linux ampliado.  
**Sin** GitHub Release en esta fase.

---

## 7. Plan de pruebas

### 7.1 Automatizables sin hardware (CI)

- Validación config: rechaza `0.0.0.0`, LAN sin CIDR, token vacío en remoto.  
- Auth middleware: localhost sin token OK; remoto sin token 401.  
- `doctor` con MockCollector / NVML fail simulado.  
- Parse unit file / ISS syntax check donde sea viable.  
- Contrato JSON `/health` `/gpu` (ya existe).

### 7.2 Manuales / lab (CK2 Win en 2B; Linux box en 2C)

| ID | Caso |
|----|------|
| I1 | Instalación limpia |
| I2 | Upgrade conservando config |
| I3 | Autostart tras reboot |
| I4 | Servicio sin sesión interactiva |
| I5 | Driver NVIDIA ausente (mensaje + 503) |
| I6 | Puerto ocupado (fail claro en install/start) |
| I7 | Reinicio driver / servicio (documentar need restart) |
| I8 | Uninstall sin huérfanos SCM/systemd |
| I9 | Uninstall conservando vs borrando config |
| I10 | Bind localhost-only; probe Tailscale IP debe fallar |
| I11 | Sin conexiones salientes (firewall/netstat durante idle) |
| I12 | No interferencia SRBMiner (regresión 1.1) |

---

## 8. Riesgos

| Riesgo | Mitigación |
|--------|------------|
| NVML/permisos bajo Local Service | Validar en CK2; si falla, cuenta dedicada con ACL mínima |
| Inno + firma de código ausente | SmartScreen; documentar; signing post-2C |
| Auth rompe collectors remotos futuros | Remoto opt-in; localhost sin auth |
| Re-init NVML no hot | Documentar `restart`; ticket 2.x |
| `install.sh` no es paquete distro | Aceptable MVP; .deb después |
| Scope creep Tailscale | Solo detect/bind; nunca instalar |
| Duplicación Win/Linux | Un binario, packaging delgado |

---

## 9. Plan de implementación por etapas

### Fase 2A (este documento) — **completa al aprobar**

Diseño, decisiones, estructura, riesgos, pruebas.

### Fase 2B — Windows (siguiente)

1. Ampliar `Config` + `net_policy` + fail-closed (shared).  
2. Subcommands CLI + `doctor`/`status`/`config`.  
3. Windows service module + install/uninstall.  
4. Inno script + CI artifact Setup.  
5. Validación CK2 (I1–I12 subset + SRBMiner).  
6. Auth token listo pero **remoto deshabilitado por defecto** en UI del installer (solo localhost en wizard MVP si se prefiere ultraseguro).

### Fase 2C — Linux

1. Unit + `install.sh`/`uninstall.sh`.  
2. Empaquetado tar.gz ampliado + CI.  
3. Validación hardware systemd.  
4. Paridad CLI con Windows.

### Explícitamente **después** de 2C

- GitHub Release público.  
- Auto-update.  
- Firma Authenticode.  
- .deb/.rpm.  
- Auth avanzada / allowlist HTTP por IP en LAN.

---

## 10. Criterios de aceptación 2A

- [x] Auditoría del núcleo actual documentada  
- [x] Elección: Windows Service nativo + Inno; Linux systemd + script  
- [x] Modelo config/seguridad fail-closed (sin `0.0.0.0`)  
- [x] Estructura de repo y dependencias previstas  
- [x] Riesgos y plan 2B → 2C  
- [ ] Aprobación humana para iniciar **solo Fase 2B**

---

## Resumen ejecutivo de decisiones

1. **Un binario** con subcomandos; instaladores delgados.  
2. **Config de máquina** en ProgramData / `/etc/exogpuagent` para el servicio.  
3. **SCM nativo** (no NSSM). **Inno Setup** para Windows. **systemd** + script para Linux.  
4. **Localhost default**; Tailscale/LAN opt-in con bind explícito + token; nunca `0.0.0.0`.  
5. **No** drivers, cloud, AtlasGPU, Tailscale install, ni auto-update en Fase 2.  
6. Implementación **secuencial**: 2B Windows → 2C Linux.
