# Fase 2C-A — Arquitectura: Linux DEB + Mini GUI

**Estado:** diseño (sin implementación de código de empaquetado/GUI).  
**Versión objetivo:** `0.3.0`.  
**Alcance:** auditoría de reutilización · elección de Mini GUI · paquetes `.deb` · systemd · headless · seguridad · CLI · CI · plan de pruebas.  
**Fuera de alcance ahora:** Fase 2C-B/2C-C (código), GitHub Release, AtlasGPU, Tailscale remoto, auto-update, firma de código, overclocking / power limits / fan control, telemetría de minería.

---

## 1. Auditoría del estado actual (post–Fase 2B)

### 1.1 Qué ya existe y se reutiliza

| Pieza | Ubicación | Reutilización en 2C |
|-------|-----------|---------------------|
| Servidor HTTP Axum (`/health`, `/gpu`) | `src/server.rs`, `src/http/` | Sin cambios de contrato; misma telemetría |
| Trait `GpuCollector` + NVML + mock | `src/gpu/` | Dashboard/diagnóstico consumen la misma API |
| Config TOML + paths máquina/portable | `src/config.rs` | **Ya define** `/etc/exogpuagent/config.toml` en no-Windows |
| Política fail-closed localhost | `src/net_policy.rs` | Mantener; sin LAN/Tailscale en 0.3.0 |
| `doctor` / `status` / HTTP local | `src/doctor.rs`, `src/status.rs`, `src/local_http.rs` | Ampliar checks Linux (systemd, libnvidia-ml) |
| Runtime compartido + graceful shutdown | `server::run_*`, `AgentWorker`, SIGTERM | Ideal para `Type=simple` + `KillSignal=SIGTERM` |
| CLI clap (`run`, `status`, `doctor`, `version`, `service`) | `src/main.rs` | Completar `config`; `service` → helpers systemd en Linux |
| Windows SCM + Inno | `src/service/windows.rs`, `packaging/windows/` | No tocar salvo versionado coherente |
| CI portable Win/Linux + Setup | `.github/workflows/build.yml` | Extender: `.deb` + checksums + GUI opcional |
| Docs 2A/2B | `docs/phase-2a-*.md`, `phase-2b-windows.md` | Este documento supersede el “Linux = install.sh MVP” de 2A |

### 1.2 Huecos respecto a 2C

1. **Sin empaquetado Linux real:** no hay `packaging/linux/`, unit file, ni scripts maintainer. El portable CI es solo binario + README.  
2. **`service/*` en Linux son stubs** que devuelven error; `service_state_string()` indica “Phase 2C”.  
3. **Falta subcomando `config`** (`show` / `set` / `path`) prometido en 2A.  
4. **Sin Mini GUI** ni superficie admin separada de telemetría.  
5. **`doctor` no consulta systemd** ni distingue “servicio activo vs puerto libre”.  
6. **Versionado:** crate sigue en `0.2.0`; 2C sube a **`0.3.0`** de forma coherente en Cargo, Inno y DEB.  
7. **Nota 2A desactualizada:** 2A proponía `install.sh` como MVP y `.deb` “después”. **2C eleva `.deb` a entregable principal**; el `.tar.gz` portable se mantiene como artifact paralelo (sin sustituir al DEB).

### 1.3 Principio de no duplicación

Un solo crate binario `exogpuagent`:

- Telemetría, config, doctor, status, net policy → **compartidos**.  
- `#[cfg(unix)]` solo para systemd helpers (systemctl / D-Bus mínimo vía CLI externa, no reimplementar journal).  
- Packaging delgado: unit + `debian/` scripts llaman al binario; **no** re-codificar health checks en bash más allá de wrappers de validación.  
- La GUI **no** reimplementa NVML: lee `/health` + `/gpu` (y, si hace falta, endpoints admin locales).

---

## 2. Decisión Mini GUI — comparación y recomendación

### 2.1 Alternativas evaluadas

| Criterio | A) Rust **egui/eframe** | B) **UI web local** servida por el agente |
|----------|-------------------------|-------------------------------------------|
| RAM en idle (aprox.) | Mayor (winit/wgpu o glow + runtime GUI): decenas–~100+ MB típicos | Bajo: HTML/JS estático + listener admin; overhead del proceso ya existente o crate ligero |
| CPU | Picos de render / vsync si ventana abierta | Negligible en idle; poll periódico opcional |
| Dependencias runtime | Display server (X11/Wayland), drivers GL/Vulkan según backend | Navegador **solo en el cliente admin**; servidor headless no necesita GUI |
| Seguridad | Sin CSRF web; superficie = proceso local con FS | CSRF / “localhost ≠ auth”; requiere diseño explícito (§6) |
| Distribución | Binario pesado o crate feature; empaquetar libs gráficas es frágil en distros | Assets estáticos en `/usr/share/...` o embebidos; DEB GUI opcional limpio |
| Windows + Linux | Un código egui; CI más complejo (GPU CI runners no necesarios pero linking sí) | Mismo HTML; Win/Linux abren `http://127.0.0.1:admin` |
| Headless / multipropósito | **No usable** sin display; riesgo de arrancar UI por error | Core sin UI; GUI opt-in; **no** auto-start en servidores |
| Mantenimiento | Dos stacks UI nativos a largo plazo | Una SPA mínima + endpoints admin tipados en Rust |
| Interferencia con workloads | Ventana/proceso extra en GPU hosts | Solo loopback; cero CUDA/NVML write |

### 2.2 Recomendación (aprobación requerida antes de 2C-C)

**Elegir B — Mini GUI web local (admin-only), empaquetada de forma opcional.**

Razones prioritarias para ExoGPUAgent en servidores de producción:

1. El DEB principal debe ser **headless-first** (requisito explícito). egui contradice ese objetivo o fuerza un segundo binario casi igual de complejo.  
2. Reutiliza HTTP + JSON ya existentes; la GUI es una capa de presentación, no un segundo colector.  
3. Empaquetado natural en dos paquetes (`exogpuagent` / `exogpuagent-gui`).  
4. Misma experiencia admin en Windows (abrir navegador local) sin duplicar egui en el instalador Inno.  
5. Menor riesgo de arrastrar dependencias gráficas a hosts de minería/IA.

**Rechazar egui/eframe para el MVP 0.3.0.** Reconsiderar solo si en el futuro se exige UI sin navegador en desktops air-gapped *y* se acepta un paquete GUI nativo separado.

### 2.3 Forma concreta de la UI web (diseño)

```text
Telemetría (siempre, producto):     127.0.0.1:12001  →  GET /health, GET /gpu
Admin UI (opt-in, no telemetría):   127.0.0.1:12002  →  static + /admin/api/*
```

| Decisión | Detalle |
|----------|---------|
| Puerto admin default | `12002` (configurable); **deshabilitado** si el paquete GUI no está instalado o `admin_ui.enabled = false` |
| Binding | Solo loopback; misma política fail-closed que telemetría (rechazar `0.0.0.0`) |
| Assets | SPA mínima (HTML/CSS/JS vanilla o build estático sin runtime Node en el host) en `/usr/share/exogpuagent/gui/` |
| Proceso | Mismo daemon: listener admin opcional en el runtime Tokio (feature/config), **no** segundo servicio systemd |
| Autostart UI | **Nunca** abrir navegador desde el servicio; el usuario ejecuta `exogpuagent gui` (imprime URL + abre browser si hay DISPLAY) o navega manualmente |
| Alcance funcional | Dashboard / Settings / Diagnostics únicamente (§3); sin shell remoto, sin OC, sin minería |

Páginas:

1. **Dashboard** — estado servicio, versión, hostname, GPU count, modelo, temp, W, util%, fan%, estado NVML (vía `/gpu` + status local).  
2. **Settings** — puerto telemetría, localhost, `host_name`, path de config; aplicar vía API admin autenticada; banner “reinicio de servicio requerido”.  
3. **Diagnostics** — driver/NVML, HTTP API, systemd, logs recientes (journal vía CLI helper o endpoint que ejecuta solo lecturas acotadas), versión/OS.

---

## 3. Paquetes Debian

### 3.1 Esquema de paquetes

| Paquete | Contenido | Depende de GUI |
|---------|-----------|----------------|
| **`exogpuagent`** | Binario, unit systemd, config default, man/README breve, CLI completa | **No** |
| **`exogpuagent-gui`** | Assets estáticos + drop-in/conf que habilita admin UI | Sí (`Depends: exogpuagent (= ${binary:Version})`) |

Artifact principal: **`exogpuagent_0.3.0_amd64.deb`**.  
Opcional: **`exogpuagent-gui_0.3.0_amd64.deb`**.

Instalación típica headless:

```bash
sudo apt install ./exogpuagent_0.3.0_amd64.deb
# sin X11, sin navegador, sin paquete -gui
```

### 3.2 Layout de archivos (paquete core)

```text
/usr/bin/exogpuagent
/lib/systemd/system/exogpuagent.service
/etc/exogpuagent/config.toml          # conffile
/usr/share/doc/exogpuagent/copyright
/usr/share/doc/exogpuagent/changelog.Debian.gz
/usr/share/doc/exogpuagent/README.Debian
```

Opcional GUI:

```text
/usr/share/exogpuagent/gui/...        # index.html, app.js, styles.css
/etc/exogpuagent/admin.toml           # o sección [admin_ui] en config — conffile si se añade
```

**No** instalar bajo `/usr/local` en el DEB (FHS Debian: `/usr/bin`).

### 3.3 Dependencias del paquete

| Campo | Propuesta |
|-------|-----------|
| `Architecture` | `amd64` |
| `Depends` | `${shlibs:Depends}`, `${misc:Depends}` — típico `libc6 (>= 2.35)` si se construye en Ubuntu 22.04 |
| `Recommends` | vacío o documentación |
| `Suggests` | `nvidia-driver` / `libnvidia-ml1` (distro-specific); **nunca** Depends duro de NVIDIA |
| `Conflicts` / `Replaces` | ninguno en 0.3.0 (primera versión DEB) |
| `Section` | `utils` |
| `Priority` | `optional` |

**No** Depends de: Rust, Cargo, Python, X11, Wayland, navegadores, CUDA toolkit.

### 3.4 glibc / distros soportadas

| Decisión | Detalle |
|----------|---------|
| Build CI | `ubuntu-22.04` (glibc 2.35) para el job DEB — mayor compatibilidad que `ubuntu-latest` 24.04 |
| Objetivo | Ubuntu 22.04/24.04 LTS y Debian 12 (bookworm) amd64 |
| Portable `.tar.gz` | Puede seguir en runner actual; documentar glibc mínima en README del artifact |
| Musl/static | **Fuera de 0.3.0** (complejidad NVML dlopen); reevaluar si aparece demanda Alpine |

### 3.5 Usuario y permisos

| Ítem | Valor |
|------|--------|
| Usuario sistema | `exogpuagent` (`useradd --system --home /var/lib/exogpuagent --shell /usr/sbin/nologin`) |
| Grupo | `exogpuagent` |
| Home / state | `/var/lib/exogpuagent/` (crear vacío; no borrar en `remove`) |
| Config | `/etc/exogpuagent/` mode `0750`, owner `root:exogpuagent`; `config.toml` `0640` |
| Binario | `0755` root:root |
| Capabilidades | Ninguna extra; NVML vía permisos de dispositivo NVIDIA ya presentes en el host |

### 3.6 Unit systemd (`exogpuagent.service`)

Conceptos obligatorios:

```ini
[Unit]
Description=ExoGPUAgent NVIDIA GPU telemetry (localhost)
Documentation=https://github.com/ExoGridTech/ExoGPUAgent
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=exogpuagent
Group=exogpuagent
ExecStart=/usr/bin/exogpuagent run --machine
Restart=on-failure
RestartSec=5
TimeoutStopSec=20
KillSignal=SIGTERM

# Hardening (ajustar ReadOnlyPaths según necesidad real de NVML/dev)
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ProtectKernelTunables=true
ProtectControlGroups=true
RestrictSUIDSGID=true
LockPersonality=true
MemoryDenyWriteExecute=false
ReadWritePaths=/etc/exogpuagent /var/lib/exogpuagent
# Acceso a dispositivos NVIDIA: validar en lab; puede requerir
# DeviceAllow o SupplementalGroups=video/render según distro
SupplementaryGroups=

StandardOutput=journal
StandardError=journal
SyslogIdentifier=exogpuagent

[Install]
WantedBy=multi-user.target
```

Notas de diseño:

- **`run --machine`** ya resuelve `/etc/exogpuagent/config.toml` en el código actual.  
- Logs → **journald** (`journalctl -u exogpuagent`).  
- Restart controlado: `on-failure`, no `always` (evita bucles agresivos si el puerto está ocupado).  
- Shutdown limpio: SIGTERM → path graceful ya presente en `server.rs`.  
- **No** `ExecStartPre` que instale drivers.  
- Validar en lab si `ProtectSystem=strict` + acceso a `/dev/nvidia*` requiere `DeviceAllow=` o grupo `video`/`render`; documentar el mínimo privilegio que funcione.

### 3.7 Scripts maintainer

| Script | Comportamiento |
|--------|----------------|
| `preinst` | Reservado (vacío o mensaje) |
| `postinst` | Crear usuario/grupo si faltan; `systemd-tmpfiles` si aplica; `daemon-reload`; en `configure`: `enable` + `start` (o `try-restart` en upgrade) |
| `prerm` | `stop` el servicio si está activo |
| `postrm` (`remove`) | `disable` + `daemon-reload`; **conservar** `/etc/exogpuagent`, `/var/lib/exogpuagent`, usuario |
| `postrm` (`purge`) | Además: borrar `/etc/exogpuagent` y opcionalmente `/var/lib/exogpuagent`; **no** borrar el usuario del sistema de forma agresiva sin documentarlo (preferencia: dejar usuario huérfano o `deluser --system` solo en purge documentado) |

### 3.8 `conffiles` y upgrades

- Marcar `/etc/exogpuagent/config.toml` como **conffile** (o instalar solo un `config.toml.default` y copiar en postinst con `cp -n` para no pelear con dpkg conffile prompts — **recomendación:** `config.toml.default` + postinst `save_if_absent` semántica alineada con Windows).  
- Decisión preferida alineada con 2B Inno:  
  - Empaquetar `config.toml.default` en `/usr/share/exogpuagent/`.  
  - `postinst`: si no existe `/etc/exogpuagent/config.toml`, copiar default.  
  - Así **upgrades nunca pisan** config de usuario (evita prompts interactivos en CI/apt).  
- Binario y unit se reemplazan siempre.  
- Reinicio del servicio en upgrade vía `try-restart`.

### 3.9 Uninstall vs purge

| Acción | Servicio | Binario/unit | Config `/etc` | State `/var/lib` |
|--------|----------|--------------|---------------|------------------|
| `apt remove` | stop + disable | eliminados | **conservar** | **conservar** |
| `apt purge` | igual | eliminados | **eliminar** | **eliminar** |

### 3.10 Estructura propuesta en el repo

```text
packaging/linux/
├── exogpuagent.service
├── config.toml.default
├── debian/                    # o cargo-deb / nfpm config
│   ├── control
│   ├── changelog
│   ├── copyright
│   ├── postinst
│   ├── prerm
│   ├── postrm
│   ├── conffiles              # si se usa conffile clásico
│   └── exogpuagent-gui.*      # si paquete separado
├── build-deb.sh               # invocado por CI
└── README.md                  # instrucciones apt install ./
scripts/linux/
└── Validate-LinuxInstall.sh
```

**Herramienta de build DEB (decisión de implementación 2C-B):** preferir **`nfpm`** o script `dpkg-deb` reproducible en CI sin instalar el full `devscripts` stack; alternativa `cargo-deb` si se integra limpio con el crate. Criterio: un solo comando CI, checksum reproducible, sin red fuera de apt del runner.

---

## 4. CLI (paridad y mejoras 0.3.0)

```text
exogpuagent run [--config PATH] [--machine]
exogpuagent status [--config PATH] [--portable]
exogpuagent doctor [--config PATH] [--portable]
exogpuagent version
exogpuagent config show|set|path [--machine|--portable]
exogpuagent service install|uninstall|start|stop|restart   # Win: SCM; Linux: systemctl wrappers / documentar que el DEB ya registra la unit
exogpuagent gui [--no-open]   # solo si admin UI disponible: imprime URL; abre browser si hay display
```

### 4.1 Comportamiento Linux de `service`

| Acción | Implementación |
|--------|----------------|
| `install` / `uninstall` | En hosts con DEB: **no-op documentado** o mensaje “gestionado por apt/systemd unit”; en portable tar.gz: copiar unit a `/etc/systemd/system` (requiere root) — scope 2C-B |
| `start` / `stop` / `restart` | `systemctl start|stop|restart exogpuagent` |
| `status` (subcommand service) | opcional; el `exogpuagent status` ya agrega estado |

### 4.2 `doctor` — con y sin NVIDIA

| Check | Sin NVIDIA | Con NVIDIA |
|-------|------------|------------|
| Config + net policy | sí | sí |
| Puerto / `/health` | sí | sí |
| NVML init | `unavailable` + nota clara; **`ok` del doctor puede ser true** si el daemon debe poder correr con 503 (alinear política: **warning no fatal** vs fail — **propuesta:** `ok=false` solo ante config/bind inválidos; NVML ausente → `ok=true` con `nvml=unavailable` y exit code **0** en “soft”, o exit **0** siempre que el servicio pueda operar; exit **1** solo fallos duros) |
| systemd ActiveState | sí (Linux) | sí |
| Driver files (`libnvidia-ml.so*`) | reportar ausencia | reportar path |

**Códigos de salida (automatización):**

| Código | Significado |
|--------|-------------|
| `0` | Sin fallos duros |
| `1` | Fallos de config/política/servicio crítico |
| `2` | Uso CLI inválido |

### 4.3 `config`

- `path` — imprime ruta resuelta.  
- `show` — TOML o JSON **sin** `auth_token` / secretos admin.  
- `set key=value` — escribe de forma segura; indica si hace falta `systemctl restart`.  
- Escritura a `/etc/...` requiere privilegios adecuados (root o grupo); la GUI admin usa el mismo módulo Rust, no un parser bash.

---

## 5. Servidores headless

| Requisito | Cumplimiento |
|-----------|--------------|
| Sin X11/Wayland | Paquete `exogpuagent` sin Depends gráficos |
| Sin navegador | Operación completa vía CLI + HTTP localhost |
| Sin sesión interactiva | systemd + journald |
| GUI no auto-start | `admin_ui.enabled` default `false` en core; `exogpuagent-gui` lo pone `true` vía drop-in **sin** ExecStart de browser |
| No interferir CUDA/minería | Solo lectura NVML; sin cambiar clocks/power/fan; bind localhost |

---

## 6. Seguridad

### 6.1 Superficie de red (sin cambios de postura 2B)

- Telemetría default: `127.0.0.1:12001`.  
- Admin UI default: `127.0.0.1:12002`, deshabilitada sin paquete/opt-in.  
- **No** reglas de firewall.  
- **No** Tailscale/LAN en 0.3.0.  
- **No** cloud, analytics ni telemetría saliente.  
- **No** modificar AtlasGPU.

### 6.2 Localhost ≠ autenticación (GUI web)

Amenazas: otra app local, CSRF desde página maliciosa en el navegador del admin, DNS rebinding.

Controles MVP:

1. **Token admin** generado en install/postinst (o primer enable), guardado en archivo `0600` root:exogpuagent (p. ej. `/etc/exogpuagent/admin_token`) — **nunca** en logs ni en `/gpu`.  
2. Todas las rutas `/admin/api/*` exigen header `Authorization: Bearer …` o `X-ExoGPUAgent-Admin-Token`.  
3. **Rechazar** requests admin cuyo `Origin`/`Host` no sea loopback (allowlist `http://127.0.0.1:12002`, `http://localhost:12002`).  
4. Mutaciones: método `POST`/`PUT` + header custom `X-Requested-With: ExoGPUAgentAdmin` (mitiga CSRF clásico de forms).  
5. Tokens de telemetría remota (`auth_token` en config) siguen reservados e inactivos.  
6. La GUI **no** expone ejecución de comandos arbitrarios; solo operaciones tipadas: leer status, leer logs acotados (`journalctl -u exogpuagent -n 100`), escribir claves de config permitidas, solicitar restart del unit propio.  
7. Restart del servicio: vía `systemctl restart` solo si el proceso tiene privilegio (polkit/helper) **o** documentar que Settings escribe config y el usuario reinicia con sudo — **preferencia 0.3.0:** escritura de config por API admin (daemon ya corre como `exogpuagent` con Write a `/etc/exogpuagent`) + endpoint `POST /admin/api/service/restart` que envía señal/sd_notify o ejecuta `systemctl restart` **únicamente** si se añade un helper setuid/polkit mínimo; si eso excede scope, Settings solo marca “reinicio requerido” y CLI `exogpuagent service restart` permanece el camino privilegiado. **Decisión de implementación 2C-C:** empezar con *write config + flag restart_required*; añadir restart mediado solo si el lab lo exige.

### 6.3 Privilegios del daemon

- Usuario dedicado, hardening systemd (§3.6).  
- Sin `CAP_SYS_ADMIN`.  
- Config legible por el servicio, no world-writable.

---

## 7. GitHub Actions (extensión, sin Release)

Mantener `workflow_dispatch` + `permissions: contents: read`.

| Artifact | Notas |
|----------|-------|
| `ExoGPUAgent-Setup-v0.3.0-win64.exe` | Inno; bump version |
| `exogpuagent-v0.3.0-windows-x64-*.zip` | Portable |
| `exogpuagent-v0.3.0-linux-x64-*.tar.gz` | Portable + unit + default config (sin sustituir DEB) |
| `exogpuagent_0.3.0_amd64.deb` | Core |
| `exogpuagent-gui_0.3.0_amd64.deb` | Opcional (job o mismo job Linux) |
| `SHA256SUMS.txt` | Por artifact directory |

Jobs sugeridos:

1. **build-windows** — como hoy + version `0.3.0`.  
2. **build-linux-portable** — ubuntu-22.04 o latest documentado.  
3. **build-deb** — ubuntu-22.04, produce `.deb` + checksums.  
4. (2C-C) embebido o paquete GUI.

**No** publicar GitHub Release en 2C.

---

## 8. Plan de validación Linux

### 8.1 Script `scripts/linux/Validate-LinuxInstall.sh`

Genera reporte local (markdown/txt) **sin** seriales de GPU, UUIDs, IPs públicas, hostnames sensibles opcionales (flag `--redact`):

- Versión paquete / binario / glibc (`ldd --version`).  
- `systemctl is-active`, `is-enabled`.  
- `ss`/`ss -ltnp` filtrado a `12001`/`12002` → solo `127.0.0.1`.  
- `exogpuagent doctor` / `status` (JSON).  
- `/health` y `/gpu` (resumen: counts y sensores null-safe, sin UUID).  
- Lista Depends del DEB; verificación de ausencia de libs X11 en `ldd` del binario core.  
- Timestamps de install/upgrade.

### 8.2 Matriz de pruebas

| ID | Caso |
|----|------|
| L1 | Install limpio `apt install ./exogpuagent_*.deb` |
| L2 | Ubuntu 22.04 / 24.04 / Debian 12 (glibc) |
| L3 | 0 GPUs / 1 GPU / multi-GPU |
| L4 | Sensores `null` (fan/power ausentes) |
| L5 | NVML presente vs ausente (503 + doctor) |
| L6 | Workload CUDA/miner concurrente (no regresión de hashrate/job) |
| L7 | RSS/CPU idle del servicio (baseline documentado) |
| L8 | Autostart post-reboot |
| L9 | `systemctl restart` + `/health` |
| L10 | Persistencia config tras upgrade DEB |
| L11 | `remove` conserva config; `purge` la elimina |
| L12 | Sin listen fuera de localhost |
| L13 | Core sin Depends gráficas; headless OK |
| L14 | GUI opcional no instalada ⇒ admin port cerrado |
| L15 | Instalación `-gui` ⇒ dashboard usable solo en loopback + token |

---

## 9. Metodología de implementación

### 2C-A — Diseño (este documento)

Entregable: `docs/phase-2c-architecture.md`. **Sin** código de producto. Aprobación humana antes de 2C-B.

### 2C-B — Implementación Linux (siguiente, tras aprobación)

1. Bump versión `0.3.0`.  
2. `service/linux.rs` + CLI `config` + doctor systemd.  
3. Unit + default config + scripts maintainer.  
4. Pipeline DEB + portable actualizado + SHA-256.  
5. `Validate-LinuxInstall.sh` + pruebas L1–L13 en lab.  
6. **Sin** GUI todavía (admin listener permanece deshabilitado).

### 2C-C — Mini GUI (tras 2C-B estable)

1. Assets + listener admin opt-in + token/CSRF.  
2. Páginas Dashboard / Settings / Diagnostics.  
3. Paquete `exogpuagent-gui` + artifact Windows (abrir URL / carpeta estáticos).  
4. Pruebas L14–L15 + smoke Win.

**No** desarrollar 2C-B y 2C-C en un solo golpe.

---

## 10. Riesgos

| Riesgo | Mitigación |
|--------|------------|
| NVML bajo usuario `exogpuagent` sin acceso a `/dev/nvidia*` | Lab temprano; `SupplementaryGroups=` o udev; documentar ACL mínima |
| `ProtectSystem=strict` rompe dlopen NVML | Ajustar `ReadOnlyPaths`/`DeviceAllow`; tests L5 |
| CSRF / rebinding en admin UI | Token + Origin/Host allowlist + header custom (§6.2) |
| glibc demasiado nueva en CI | Build DEB en ubuntu-22.04 |
| Scope creep restart privilegiado desde GUI | MVP: restart_required + CLI; helper polkit después |
| Duplicar telemetría en GUI | Solo consumir `/gpu`/`/health` |
| egui “por si acaso” | Rechazado en 0.3.0; no mezclar stacks |
| conffile prompts en apt upgrade | Preferir `config.toml.default` + copy-if-absent |
| Interferencia con minería/IA | Solo lectura; localhost; sin OC/power/fan |

---

## 11. Criterios de aceptación 2C-A

- [x] Auditoría de reutilización documentada (sin duplicar NVML/config/HTTP)  
- [x] Comparación egui vs web local con **recomendación explícita** (web admin opt-in)  
- [x] Diseño DEB core + GUI opcional, systemd, usuario, conffiles/purge  
- [x] Headless garantizado en paquete principal  
- [x] Modelo de seguridad admin (localhost ≠ auth)  
- [x] CLI, CI artifacts, plan de pruebas, riesgos, plan 2C-B → 2C-C  
- [ ] **Aprobación humana** para iniciar **solo Fase 2C-B** (Linux, sin GUI)

---

## Resumen ejecutivo de decisiones

1. **Reutilizar** el binario actual: config máquina Linux ya apunta a `/etc/exogpuagent/config.toml`; telemetría y doctor compartidos.  
2. **Mini GUI = web local admin** en puerto loopback separado (`12002`), paquete **`exogpuagent-gui` opcional**; **no** egui en 0.3.0.  
3. **DEB principal headless** `exogpuagent_0.3.0_amd64.deb` + systemd + usuario `exogpuagent` + journald; sin Depends NVIDIA ni gráficos.  
4. Config: **copy-if-absent** desde default; `remove` conserva; `purge` limpia.  
5. Seguridad: telemetría y admin solo loopback; token admin + anti-CSRF; sin firewall/Tailscale/cloud.  
6. CI: Setup Win + portable Win/Linux + `.deb` (+ GUI en 2C-C) + SHA-256; sin GitHub Release.  
7. Implementación **secuencial**: 2C-B Linux → 2C-C GUI tras aprobación.
