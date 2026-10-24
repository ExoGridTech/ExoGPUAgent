# Fase 2C-C1 — Arquitectura: Mini GUI & Network Discovery

**Estado:** diseño (sin implementación de código de producto).  
**Versión objetivo de implementación:** `0.4.0` (secuencia 2C-C2 → 2C-C3; crate permanece en `0.3.0` hasta arrancar código).  
**Alcance:** auditoría · Mini GUI web local · Network Discovery · Network Settings (preparación) · seguridad admin · config/migración · empaquetado · pruebas · riesgos.  
**Fuera de alcance ahora:** 2C-C2/2C-C3 (código), acceso remoto real, port forwarding/UPnP, GUI remota, AtlasGPU, GitHub Release, egui/Electron/Node runtime.

Este documento **actualiza** decisiones de `phase-2c-architecture.md` §2 donde hay conflicto (p. ej. puerto admin: **12010**, no 12002).

---

## 1. Auditoría del núcleo actual (reutilización)

### 1.1 Piezas existentes a reutilizar

| Pieza | Ubicación | Rol en 2C-C |
|-------|-----------|-------------|
| Telemetría `/health`, `/gpu` | `src/http/`, `src/server.rs` | **Sin cambios de contrato**; Dashboard las consume vía loopback interno o API admin de solo lectura |
| `GpuCollector` / NVML / mock | `src/gpu/` | Única fuente de métricas; GUI no abre segunda sesión NVML |
| `Config` + paths máquina/portable | `src/config.rs` | Ampliar con secciones nuevas + defaults serde; migración soft |
| `net_policy` fail-closed | `src/net_policy.rs` | Extender: rechazar remoto real en 2C-C; validar binds GUI/telemetría |
| `doctor` / `status` / `local_http` | `src/*` | Diagnostics GUI reutiliza la misma lógica |
| CLI `config` / `service` | `src/main.rs`, `config_cmd.rs` | Ampliar para GUI enable/port y headless |
| Windows SCM / Linux systemd | `src/service/` | Mismo proceso; listener admin opcional en Tokio |
| Packaging DEB / Inno / CI | `packaging/`, `.github/workflows/` | GUI opt-in; core headless intacto |

### 1.2 Limitaciones actuales a resolver en diseño (no en este documento como código)

1. `AccessMode::{Tailscale,Lan}` existen pero **cualquier** modo no-localhost hace fallar el arranque (`RemoteModeDisabled`).  
2. No hay superficie HTTP admin ni assets UI.  
3. No hay descubrimiento de interfaces.  
4. `auth_token` está reservado y vacío; no hay auth admin.  
5. Un solo listener Axum en el puerto de telemetría.

### 1.3 Principios

- **Local-first / privacy-first / provider-agnostic.**  
- Un núcleo de telemetría; GUI = presentación + control tipado.  
- Localhost ≠ autenticación.  
- Detectar ≠ verificar ≠ autorizar acceso remoto.  
- En 2C-C: **preparar** remoto; **no activarlo**.

---

## 2. Decisiones de arquitectura (resumen ejecutivo)

| Decisión | Elección |
|----------|----------|
| UI | Web local estática (HTML/CSS/JS) embebida o servida desde disco; **no** egui/Electron/Node/Python runtime |
| Telemetría | `127.0.0.1:12001` (configurable) |
| Admin GUI | `127.0.0.1:12010` (configurable); **deshabilitada por defecto** |
| Proceso | Mismo daemon Tokio; **segundo listener** admin (no segundo servicio) |
| Empaquetado | Feature Cargo `gui` **on by default in release builds** *o* assets siempre embebidos pero listener off; DEB opcional `exogpuagent-gui` solo si se quiere separar peso — **recomendación §8** |
| Acceso remoto telemetría | Schema + UI + discovery; **backend rechaza enable** en 2C-C |
| Admin remoto | **Prohibido** en 2C-C (y no planificado sin diseño aparte) |
| NVML | Solo lectura; poll rate limitado/caché compartida |

---

## 3. Mini GUI — forma concreta

### 3.1 Superficie HTTP

```text
Telemetría (producto, collectors):
  bind telemetría → GET /health, GET /gpu
  (sin cookies, sin CSRF; remoto futuro = auth distinta — no en 2C-C)

Administración (opt-in, solo loopback en 2C-C):
  127.0.0.1:12010
    GET  /                 → SPA (index.html)
    GET  /assets/*         → CSS/JS estáticos
    POST /admin/api/login
    POST /admin/api/logout
    GET  /admin/api/session
    GET  /admin/api/dashboard
    GET  /admin/api/diagnostics
    GET  /admin/api/network/interfaces
    GET  /admin/api/config
    PUT  /admin/api/config          (subset permitido)
    POST /admin/api/network/prepare (valida + guarda “intent”; no activa remoto)
```

- Rutas `/admin/*` **nunca** se montan en el listener de telemetría.  
- El listener admin **no** expone `/health` ni `/gpu` (evita confusión y CSRF cruzado).  
- Dashboard obtiene GPU vía llamada interna al collector (mismo proceso) o `local_http` a `127.0.0.1:telemetry_port` — preferir **collector compartido en memoria** para no doblar NVML.

### 3.2 Frontend

| Requisito | Diseño |
|-----------|--------|
| Stack | HTML + CSS + JS vanilla (o build estático precommit); **cero CDN** |
| Embebido | `rust-embed` / `include_dir` sobre `gui/dist/` |
| Responsive | Layout móvil-primero simple; sin framework SPA pesado |
| Refresh | Poll configurable (default **5 s**, min 2 s, max 60 s); backoff si pestaña oculta (`document.visibilityState`) |
| i18n | Inglés en MVP; strings centralizados para futuro ES |

### 3.3 Páginas UI

1. **Login** — bootstrap de credencial admin si falta; luego login.  
2. **Dashboard** — estado agente, versión, OS, hostname, uptime, NVML, GPUs (modelo, temp, W, util, fan, PCI), health de endpoints.  
3. **Settings** — puertos telemetría/GUI, hostname, poll interval, enable GUI; banner “reinicio requerido”.  
4. **Network** — modos Localhost / VPN / LAN (§5); estados de discovery; **sin toggle “activar remoto” efectivo**.  
5. **Diagnostics** — doctor/status, logs acotados (solo lectura), conflictos de puerto.

### 3.4 CLI headless (paridad)

```text
exogpuagent gui [--no-open]          # imprime URL; opcional abrir browser si hay display
exogpuagent config set gui.enabled=true|false
exogpuagent config set gui.port=12010
exogpuagent doctor                   # incluye checks admin listener si enabled
```

El servicio **nunca** abre un navegador automáticamente.

---

## 4. Network Discovery (módulo independiente)

### 4.1 Crate/módulo propuesto

```text
src/net_discovery/
  mod.rs           # API pública: list_interfaces(), refresh()
  types.rs         # InterfaceInfo, ProviderHint, Capability, Confidence
  enumerate.rs     # OS: Windows IP Helper / Linux netlink|sysfs|/proc
  classify.rs      # heurísticas provider-agnostic
  providers/       # plugins de reconocimiento (Tailscale, WireGuard, …)
    mod.rs
    tailscale.rs
    wireguard.rs
    openvpn.rs
    zerotier.rs
    generic_vpn.rs
```

**Sin** invocar CLIs de proveedores (`tailscale status`, etc.) en MVP si implica dependencia o privilegios; preferir:

- Nombre/descripción de interfaz (p. ej. `tailscale0`, `Tailscale Tunnel`).  
- Rangos CGNAT típicos (100.64.0.0/10) solo como **hint**, no prueba.  
- Flags de interfaz (point-to-point, virtual).  
- En Linux: `/sys/class/net/*/tun_flags`, tipo, operstate.  
- En Windows: `IfType`, description strings vía IP Helper (crate `windows` / `ipconfig` / `network-interface` evaluados en 2C-C3).

### 4.2 Modelo de datos

```rust
struct InterfaceInfo {
    id: InterfaceId,              // estable preferido: GUID (Win) / ifindex+name hash (Linux)
    name: String,
    description: Option<String>,
    ipv4: Vec<Ipv4Addr>,
    ipv6: Vec<Ipv6Addr>,
    oper_state: OperState,        // Up | Down | Unknown
    if_kind: IfKind,              // Loopback | Ethernet | Wifi | Tunnel | Other
    is_virtual: bool,
    provider_hint: ProviderHint,  // None | Tailscale | WireGuard | OpenVPN | ZeroTier | CommercialVpn | CorporateVpn | UnknownVpn
    provider_confidence: Confidence, // None | Low | Medium | High
    listen_candidate: ListenCandidate, // NotSuitable | LoopbackOk | LikelyPrivate | LikelyVpnOverlay | Unknown
    warnings: Vec<String>,
}
```

### 4.3 Capas de verdad (obligatorias en UI y API)

| Capa | Significado | Quién la afirma en 2C-C |
|------|-------------|-------------------------|
| 1. Interfaz detectada | Existe en el OS | Discovery |
| 2. VPN identificada | Heurística de proveedor con confidence | Classifier |
| 3. Conectividad verificada | Reachability real (ping/HTTP a peer) | **No en 2C-C** (o probe opcional localhost-only) |
| 4. Acceso remoto autorizado | Usuario + policy + bind activo | **Rechazado** por net_policy en 2C-C |

UI debe mostrar badges explícitos: `detected` / `identified` / `verified` / `authorized` — solo las dos primeras pobladas en esta fase.

### 4.4 Extensibilidad de proveedores

```rust
trait ProviderRecognizer: Send + Sync {
    fn name(&self) -> &'static str;
    fn classify(&self, iface: &RawInterface) -> Option<(ProviderHint, Confidence, Vec<String>)>;
}
```

Registro estático en `providers/mod.rs`. Nuevos proveedores = nuevo archivo + test de fixtures, sin tocar telemetría.

### 4.5 Dependencias candidatas (evaluar en 2C-C3; no añadir aún)

| Crate | Uso | Riesgo |
|-------|-----|--------|
| `network-interface` / `if-addrs` | Enumeración cross-platform | Madurez / IPv6 |
| `windows` (Win) | IP Helper detallado | Peso compile |
| (evitar) invocar `ip`/`powershell` | Fragilidad + privilegios | Preferir APIs |

Elegir la opción más pequeña que cubra Win+Linux en CI sin GPU.

---

## 5. Network Settings (UX + backend)

### 5.1 Tres modos

#### Localhost (default)

- Telemetría: `127.0.0.1:12001`  
- GUI: `127.0.0.1:12010`  
- Sin acceso remoto.  
- `access_mode = "localhost"`.

#### VPN (preparación)

- Lista interfaces con `provider_hint` VPN/tunnel y `listen_candidate`.  
- Usuario selecciona interfaz/`InterfaceId` + IP candidata.  
- Acciones: ver IP, proveedor probable, warnings, **“Prepare remote config”** (persiste intent).  
- **No** “Enable remote” efectivo.  
- Backend: si `remote_access.enabled = true` → **error fail-closed** en 2C-C.

#### LAN (preparación)

- Ethernet/Wi-Fi con IPs unicast privadas.  
- Selección explícita de una IP (nunca `0.0.0.0` / `::`).  
- Warnings: LAN ≠ cifrado; Bearer sobre HTTP insuficiente.  
- Mismo rechazo de enable real.

### 5.2 Comportamiento dinámico

| Evento | Comportamiento |
|--------|----------------|
| Interfaz desaparece | Marcar `unavailable`; invalidar selección en UI |
| IP cambia | No rebind automático; conservar localhost; log sin secretos |
| Interfaz down | No fallback a otra NIC/LAN/Internet |
| Intent remoto guardado pero interfaz lost | `prepared_bind_status = stale` |

Preferir `InterfaceId` estable + `preferred_ipv4` opcional; al aplicar en el futuro (post-2C-C), resolver de nuevo o fallar cerrado.

---

## 6. Seguridad de administración

### 6.1 Credenciales

| Ítem | Diseño |
|------|--------|
| Bootstrap | Primera visita / `exogpuagent gui init`: generar password aleatorio **o** forzar set interactivo; guardar hash (Argon2id) en archivo `0600` |
| Almacenamiento | `/etc/exogpuagent/admin_auth.toml` (Linux) / `%ProgramData%\...\admin_auth.toml` (Windows); **fuera** del TOML de telemetría si es posible, o sección omitida en logs |
| Telemetría token | Distinto (`auth_token` / futuro); no reutilizar cookie admin |
| Logs | Nunca password, session id crudo, ni hash completo |

### 6.2 Sesión

- Cookie `HttpOnly; Secure` solo si HTTPS (en localhost HTTP: `HttpOnly; SameSite=Strict; Path=/admin`)  
- Session store en memoria + opcional archivo rotativo; TTL corto (p. ej. 12 h) + idle timeout  
- CSRF: token synchronizer en cookie/header `X-CSRF-Token` o double-submit  
- `Origin` / `Host` allowlist: solo `http://127.0.0.1:12010` y `http://localhost:12010`  
- Rate limit login (p. ej. 5 / 5 min por IP loopback peer)  
- Separar GET (lectura) vs PUT/POST (escritura) con CSRF en mutaciones  

### 6.3 Autorización de operaciones

Toda mutación validada en Rust:

- Claves de config allowlist.  
- Puertos en rango seguro; sin overlap telemetría↔GUI.  
- Rechazo de `0.0.0.0`, remoto enable, paths de shell.  
- Sin endpoints de “run command” / journal arbitrario (solo `journalctl -u exogpuagent -n N` acotado vía helper tipado).

### 6.4 Activación GUI

No arrancar listener admin si:

- `gui.enabled = false`, o  
- falta bootstrap de credenciales (`admin_auth` ausente) → CLI/`gui init` obligatorio.

---

## 7. Acceso remoto (solo arquitectura; no activar)

### 7.1 Schema previsto (persistido, no honrado para bind)

```toml
[gui]
enabled = false
port = 12010
bind = "127.0.0.1"
poll_interval_secs = 5

[network]
mode = "localhost"             # localhost | vpn | lan
selected_interface_id = ""
selected_ip = ""
prepared_remote = false        # intent only

[remote_access]                # TELEMETRY remote — future
enabled = false                # MUST remain false in 2C-C builds
# bind_ip = ""
# interface_id = ""
# require_https = true         # LAN
# auth = "bearer"              # distinct from admin

# auth_token = ""              # existing field; telemetry consumers
```

### 7.2 Reglas fail-closed (2C-C)

| Condición | Resultado |
|-----------|-----------|
| `remote_access.enabled = true` | **Rechazar** al cargar config / al arrancar |
| `access_mode ∈ {tailscale, lan}` con bind no-loopback | **Rechazar** (igual que hoy, mensaje actualizado “not enabled in this build”) |
| GUI bind no-loopback | **Rechazar** |
| Prepare API pide enable | HTTP 409/400 con código `remote_not_implemented` |

Documentar por proveedor (Tailscale overlay vs LAN cleartext) en `docs/` en 2C-C3 sin implementar.

---

## 8. Configuración persistente y migración

### 8.1 Compatibilidad

- Campos desconocidos: `serde` ignore o `#[serde(default)]`.  
- Config v0.2/v0.3 sin `[gui]` → defaults (`gui.enabled=false`).  
- Upgrades: **nunca** sobrescribir `config.toml` existente (ya cumplido en DEB/Inno).  
- Nuevos archivos (`admin_auth.toml`) solo se crean en bootstrap explícito.

### 8.2 Reinicio

Cambios que requieren restart del servicio:

- Puertos / binds / `gui.enabled` / (futuro) remote bind.

API responde `{ "restart_required": true, "applied": [...] }`. CLI imprime el mismo aviso.

---

## 9. Empaquetado Windows / Linux

### 9.1 Recomendación

| Enfoque | Veredicto |
|---------|-----------|
| Assets embebidos en el mismo binario; `gui.enabled=false` por defecto | **Elegido para MVP 2C-C2** — un artifact, headless OK, sin segundo paquete obligatorio |
| Paquete `exogpuagent-gui` | **Opcional post-MVP** si el embed supera umbral de tamaño (~añadir drop-in `gui.enabled=true`) |
| Feature Cargo `gui` | Útil para builds mínimos CI; release oficiales con `gui` incluida pero desactivada en runtime |

### 9.2 Windows

- Mismo servicio SCM; listener admin si enabled.  
- Inno: checkbox “Enable local Mini GUI (localhost:12010)” → escribe config + bootstrap password en log del setup **una vez** (no al Event Log).  
- Sin ventana Win32 permanente.

### 9.3 Linux

- systemd sin cambios de Unit salvo env opcional.  
- Headless: sin X11/Wayland.  
- DEB core sin Depends gráficos.  
- Evaluar `exogpuagent-gui` solo si se externalizan assets.

### 9.4 CI (tras 2C-C2 estable)

Mantener Setup / portable Win / portable Linux / DEB / SHA-256.  
No Release. Artifacts GUI no separados mientras embed+disabled sea el default.

---

## 10. Estructura de módulos propuesta (repo)

```text
src/
  admin/                 # NEW — router Axum admin, auth, csrf, static
    mod.rs
    auth.rs
    session.rs
    routes.rs
    static_files.rs
  net_discovery/         # NEW — §4
  net_policy.rs          # EXTEND
  config.rs              # EXTEND + migration helpers
  server.rs              # spawn telemetry + optional admin listeners
  gpu/                   # unchanged contract; shared handle for dashboard cache
gui/                     # NEW — source assets (html/css/js)
  index.html
  app.js
  styles.css
docs/
  phase-2c-gui-network-architecture.md  # este documento
```

### 10.1 Dependencias nuevas previstas (no añadidas aún)

| Crate | Uso |
|-------|-----|
| `rust-embed` o `include_dir` | Assets |
| `argon2` + `rand` | Password admin |
| `tower-http` (set/limit features) | Trace limitado, timeout; sin CORS abierto |
| `axum` features: `form`, `query`, `multipart` solo si hace falta | Cookies vía `axum-extra` / `tower-cookies` |
| Enumeración red | Ver §4.5 |

Sin SDKs cloud, sin Tailscale client library, sin analytics.

---

## 11. Dashboard — datos y rendimiento

| Campo | Fuente |
|-------|--------|
| Estado agente / uptime | Proceso + service state |
| Versión / OS | `CARGO_PKG_VERSION` / `std::env::consts::OS` |
| Hostname | `config.host_name` o hostname OS |
| NVML + GPUs | `GpuCollector` compartido |
| Endpoints HTTP | Probe loopback a telemetría |

**Caché NVML:** refresh máximo 1× por `poll_interval_secs` (default 5); Dashboard y `/gpu` externo comparten última snapshot (mutex/watch channel). No writes NVML.

---

## 12. Plan de pruebas

### 12.1 Automatizadas (2C-C2/C3)

- Contrato JSON `/health` `/gpu` (regresión).  
- Admin: login/logout, sesión, CSRF fail, Origin/Host reject.  
- Config migrate: TOML viejo → defaults GUI.  
- Discovery: fixtures de interfaces (VPN unknown, down, IP change).  
- net_policy: reject remote enable, reject `0.0.0.0`, overlap ports.  
- NVML fail: dashboard degradado, agente up.  
- Headless: `gui.enabled=false` → no listen 12010.  
- Idle: sin conexiones salientes (mock net / allowlist test).

### 12.2 Manuales (con autorización)

| Host | Casos |
|------|-------|
| CK2 Win | GUI local, service, SRBMiner coexistence, discovery adapters |
| core-srv Linux | DEB headless, enable GUI via CLI, journal, multi-NIC |

**No** acceder a esos equipos en 2C-C1.

---

## 13. Metodología de implementación

### 2C-C1 — este documento

Diseño completo; aprobación humana antes de código.

### 2C-C2 — GUI local

1. Schema `[gui]` + admin auth bootstrap.  
2. Listener admin + static embed.  
3. Dashboard / Settings / Diagnostics.  
4. Tests auth/CSRF/config.  
5. CLI `gui` / config keys.  
6. **Sin** Network Discovery UI completa (stub opcional).

### 2C-C3 — Network Discovery

1. Módulo discovery + providers heurísticos.  
2. Pantalla Network Settings (prepare only).  
3. Persistencia de intent; rechazo de enable remoto.  
4. Tests fixtures + docs de proveedores.

**No** implementar bind remoto real ni admin remoto en 2C-C.

---

## 14. Riesgos

| Riesgo | Mitigación |
|--------|------------|
| CSRF / browser malware en localhost | Auth + CSRF + Origin/Host (§6) |
| Doble NVML / CPU | Snapshot compartida + rate limit |
| Heurística VPN falsa | Capas detected≠authorized; confidence Low |
| Usuario cree que “Prepare” = remoto activo | Copy UI + API `remote_not_implemented` |
| Binario más pesado por embed | Comprimir assets; medir; paquete -gui después |
| Drift puerto 12002 (doc 2C-A) vs 12010 | **12010 gana**; actualizar docs al implementar |
| Scope creep Tailscale install | Solo classify; nunca instalar/up |
| Cookie Secure en HTTP localhost | SameSite=Strict sin Secure en HTTP local documentado |

---

## 15. Criterios de aceptación 2C-C1

- [x] Auditoría de reutilización documentada  
- [x] Mini GUI web local, puertos 12001 / **12010**, opt-in  
- [x] Separación telemetría vs admin  
- [x] Diseño Network Discovery extensible + 4 capas de verdad  
- [x] Network Settings Localhost/VPN/LAN prepare-only  
- [x] Seguridad admin (auth, sesión, CSRF, secrets)  
- [x] Acceso remoto: schema + rechazo explícito en esta fase  
- [x] Migración config, empaquetado, pruebas, riesgos, plan C2→C3  
- [ ] **Aprobación humana** para iniciar **solo 2C-C2**

---

## Resumen de decisiones

1. GUI = web estática embebida; default **off**; puerto **12010** loopback.  
2. Telemetría intacta en **12001**; admin en listener separado.  
3. Discovery provider-agnostic con confidence; **no** verificar ni autorizar remoto aún.  
4. Prepare configs VPN/LAN en UI; backend **fail-closed** si se intenta enable.  
5. Auth admin ≠ token telemetría; sin comandos arbitrarios.  
6. Implementación secuencial: **C2 GUI local → C3 discovery**; sin Release ni AtlasGPU.
