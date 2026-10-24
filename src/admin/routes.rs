//! Admin HTTP API + static UI (loopback only).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::admin::rate_limit::LoginRateLimiter;
use crate::admin::session::{peer_bucket, SessionStore};
use crate::admin::snapshot::SnapshotCache;
use crate::admin::static_files;
use crate::admin_auth::AdminAuthFile;
use crate::config::Config;
use crate::gpu::GpuCollector;
use crate::models::GpuInfo;

pub const SESSION_COOKIE: &str = "ega_admin_session";
pub const CSRF_HEADER: &str = "x-csrf-token";

#[derive(Clone)]
pub struct AdminState {
    pub config_path: PathBuf,
    pub auth_path: PathBuf,
    pub config: Arc<std::sync::RwLock<Config>>,
    pub collector: Arc<dyn GpuCollector>,
    pub cache: Arc<SnapshotCache>,
    pub sessions: Arc<SessionStore>,
    pub rate_limit: Arc<LoginRateLimiter>,
    pub started_at: Instant,
    pub service_status: fn() -> String,
}

pub fn admin_router(state: AdminState) -> Router {
    let authed = Router::new()
        .route("/admin/api/dashboard", get(dashboard))
        .route("/admin/api/diagnostics", get(diagnostics))
        .route("/admin/api/config", get(get_config).put(put_config))
        .route_layer(from_fn_with_state(state.clone(), require_auth));

    Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route(
            "/app.js",
            get(|| async { static_files::serve_asset("/app.js".parse().unwrap()) }),
        )
        .route(
            "/styles.css",
            get(|| async { static_files::serve_asset("/styles.css".parse().unwrap()) }),
        )
        .route(
            "/favicon.svg",
            get(|| async { static_files::serve_asset("/favicon.svg".parse().unwrap()) }),
        )
        .route("/admin/api/login", post(login))
        .route("/admin/api/logout", post(logout))
        .route("/admin/api/session", get(session_info))
        .merge(authed)
        .layer(from_fn_with_state(state.clone(), guard_origin_host))
        .with_state(state)
}

async fn index() -> Response {
    static_files::serve_asset("/".parse().unwrap())
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    password: String,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    ok: bool,
    csrf_token: String,
}

async fn login(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Response {
    let peer = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("127.0.0.1");
    let bucket = peer_bucket(peer);
    if !state.rate_limit.check_and_record(&bucket) {
        return json_err(StatusCode::TOO_MANY_REQUESTS, "too many login attempts");
    }

    let auth = match AdminAuthFile::load(&state.auth_path) {
        Ok(a) => a,
        Err(crate::admin_auth::AdminAuthError::NotBootstrapped) => {
            return json_err(
                StatusCode::SERVICE_UNAVAILABLE,
                "admin credentials not bootstrapped; run: exogpuagent gui init",
            );
        }
        Err(_) => {
            // Do not leak path/permission details to the browser.
            return json_err(
                StatusCode::SERVICE_UNAVAILABLE,
                "admin credentials unavailable (check service permissions on admin_auth.toml)",
            );
        }
    };

    if !auth.verify(&body.password) {
        return json_err(StatusCode::UNAUTHORIZED, "invalid credentials");
    }

    state.rate_limit.clear(&bucket);
    let (sid, session) = state.sessions.create();
    let mut res = Json(LoginResponse {
        ok: true,
        csrf_token: session.csrf_token,
    })
    .into_response();
    set_session_cookie(res.headers_mut(), &sid);
    res
}

async fn logout(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if let Some(sid) = cookie_value(&headers, SESSION_COOKIE) {
        state.sessions.remove(&sid);
    }
    let mut res = Json(serde_json::json!({"ok": true})).into_response();
    clear_session_cookie(res.headers_mut());
    res
}

#[derive(Serialize)]
struct SessionInfo {
    authenticated: bool,
    csrf_token: Option<String>,
    poll_interval_secs: u64,
}

async fn session_info(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    let cfg = state.config.read().expect("config lock").clone();
    if let Some(sid) = cookie_value(&headers, SESSION_COOKIE) {
        if let Some(session) = state.sessions.get(&sid) {
            return Json(SessionInfo {
                authenticated: true,
                csrf_token: Some(session.csrf_token),
                poll_interval_secs: cfg.gui_poll_interval_secs(),
            })
            .into_response();
        }
    }
    Json(SessionInfo {
        authenticated: false,
        csrf_token: None,
        poll_interval_secs: cfg.gui_poll_interval_secs(),
    })
    .into_response()
}

#[derive(Serialize)]
struct DashboardResponse {
    agent_status: String,
    version: String,
    os: String,
    hostname: String,
    uptime_secs: u64,
    uptime_human: String,
    nvml_status: String,
    gpu_count: usize,
    gpus: Vec<GpuInfo>,
    gpus_note: Option<String>,
    telemetry_http: String,
    service: String,
}

async fn dashboard(State(state): State<AdminState>) -> Response {
    let cfg = state.config.read().expect("config lock").clone();
    let uptime = state.started_at.elapsed().as_secs();
    let (nvml_status, gpus, gpus_note) = match state.cache.get(state.collector.as_ref()) {
        Ok(snaps) => {
            let gpus: Vec<GpuInfo> = snaps.into_iter().map(|s| s.into_public()).collect();
            ("ok".to_string(), gpus, None)
        }
        Err(e) => (
            format!("unavailable: {e}"),
            Vec::new(),
            Some("NVML unavailable or no readable GPUs".into()),
        ),
    };

    let telemetry_http = match crate::local_http::get_json_loopback(cfg.port, "/health") {
        Ok((code, _)) => format!("HTTP {code}"),
        Err(e) => format!("unreachable ({e})"),
    };

    let hostname = if cfg.host_name.is_empty() {
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_default()
    } else {
        cfg.host_name.clone()
    };

    Json(DashboardResponse {
        agent_status: "running".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        os: std::env::consts::OS.into(),
        hostname,
        uptime_secs: uptime,
        uptime_human: format_uptime(uptime),
        nvml_status,
        gpu_count: gpus.len(),
        gpus,
        gpus_note,
        telemetry_http,
        service: (state.service_status)(),
    })
    .into_response()
}

async fn diagnostics(State(state): State<AdminState>) -> Response {
    let cfg = state.config.read().expect("config lock").clone();
    let report = crate::doctor::run_doctor(&state.config_path);
    let mut redacted = serde_json::to_value(&report).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(obj) = redacted.as_object_mut() {
        obj.insert(
            "config_summary".into(),
            serde_json::json!(cfg.safe_summary()),
        );
        obj.remove("auth");
    }
    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "config_path": state.config_path.display().to_string(),
        "auth_bootstrapped": AdminAuthFile::exists(&state.auth_path),
        "gui_enabled": cfg.gui.enabled,
        "gui_port": cfg.gui.port,
        "telemetry_port": cfg.port,
        "service": (state.service_status)(),
        "doctor": redacted,
        "generated_unix": SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        "note": "Redacted diagnostics export — no passwords, tokens, or session cookies",
    }))
    .into_response()
}

#[derive(Serialize)]
struct ConfigView {
    host_name: String,
    bind: String,
    port: u16,
    access_mode: String,
    gui: crate::config::GuiConfig,
    config_path: String,
    auth_path: String,
}

async fn get_config(State(state): State<AdminState>) -> Response {
    let cfg = state.config.read().expect("config lock").clone();
    Json(ConfigView {
        host_name: cfg.host_name.clone(),
        bind: cfg.bind.clone(),
        port: cfg.port,
        access_mode: cfg.access_mode.as_str().into(),
        gui: cfg.gui.clone(),
        config_path: state.config_path.display().to_string(),
        auth_path: state.auth_path.display().to_string(),
    })
    .into_response()
}

#[derive(Debug, Deserialize)]
struct ConfigUpdate {
    host_name: Option<String>,
    gui_poll_interval_secs: Option<u64>,
}

async fn put_config(State(state): State<AdminState>, Json(body): Json<ConfigUpdate>) -> Response {
    let mut cfg = state.config.write().expect("config lock").clone();
    let restart_required = false;

    if let Some(name) = body.host_name {
        if name.len() > 128 {
            return json_err(StatusCode::BAD_REQUEST, "host_name too long");
        }
        cfg.host_name = name;
    }
    if let Some(poll) = body.gui_poll_interval_secs {
        if !(2..=60).contains(&poll) {
            return json_err(StatusCode::BAD_REQUEST, "poll_interval must be 2..=60");
        }
        cfg.gui.poll_interval_secs = poll;
    }

    if let Err(e) = crate::net_policy::validate(&cfg) {
        return json_err(StatusCode::BAD_REQUEST, &e.to_string());
    }
    if let Err(e) = cfg.save(&state.config_path) {
        return json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }
    *state.config.write().expect("config lock") = cfg.clone();

    Json(serde_json::json!({
        "ok": true,
        "restart_required": restart_required,
        "poll_interval_secs": cfg.gui_poll_interval_secs(),
        "summary": cfg.safe_summary(),
    }))
    .into_response()
}

async fn require_auth(State(state): State<AdminState>, req: Request<Body>, next: Next) -> Response {
    let headers = req.headers();
    let Some(sid) = cookie_value(headers, SESSION_COOKIE) else {
        return json_err(StatusCode::UNAUTHORIZED, "authentication required");
    };
    let Some(session) = state.sessions.get(&sid) else {
        return json_err(StatusCode::UNAUTHORIZED, "session expired");
    };

    let method = req.method().clone();
    if method != axum::http::Method::GET && method != axum::http::Method::HEAD {
        let csrf = headers
            .get(CSRF_HEADER)
            .or_else(|| headers.get("X-CSRF-Token"))
            .and_then(|v| v.to_str().ok());
        if csrf != Some(session.csrf_token.as_str()) {
            return json_err(StatusCode::FORBIDDEN, "csrf validation failed");
        }
        let xrw = headers
            .get("x-requested-with")
            .and_then(|v| v.to_str().ok());
        if xrw != Some("ExoGPUAgentAdmin") {
            return json_err(StatusCode::FORBIDDEN, "missing admin request marker");
        }
    }

    next.run(req).await
}

async fn guard_origin_host(
    State(state): State<AdminState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let cfg = state.config.read().expect("config lock").clone();
    let port = cfg.gui.port;
    let allowed_hosts = [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ];
    if let Some(host) = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
    {
        if !allowed_hosts.iter().any(|h| h.eq_ignore_ascii_case(host)) {
            return json_err(StatusCode::FORBIDDEN, "invalid Host header");
        }
    } else {
        return json_err(StatusCode::FORBIDDEN, "missing Host header");
    }

    let method = req.method().clone();
    if method != axum::http::Method::GET && method != axum::http::Method::HEAD {
        if let Some(origin) = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
        {
            let allowed_origins = [
                format!("http://127.0.0.1:{port}"),
                format!("http://localhost:{port}"),
                format!("http://[::1]:{port}"),
            ];
            if !allowed_origins.iter().any(|o| o == origin) {
                return json_err(StatusCode::FORBIDDEN, "invalid Origin");
            }
        }
    }

    next.run(req).await
}

fn json_err(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({"error": msg}))).into_response()
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            if k.trim() == name {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

fn set_session_cookie(headers: &mut HeaderMap, sid: &str) {
    let val = format!(
        "{SESSION_COOKIE}={sid}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
        12 * 60 * 60
    );
    headers.insert(header::SET_COOKIE, val.parse().expect("cookie header"));
}

fn clear_session_cookie(headers: &mut HeaderMap) {
    let val = format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0");
    headers.insert(header::SET_COOKIE, val.parse().expect("cookie header"));
}

fn format_uptime(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{h}h {m}m {s}s")
}
