//! Admin Mini GUI API security tests (no NVIDIA hardware).

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use exogpuagent::admin::{admin_router, AdminState, LoginRateLimiter, SessionStore, SnapshotCache};
use exogpuagent::admin_auth::AdminAuthFile;
use exogpuagent::config::{Config, GuiConfig};
use exogpuagent::gpu::{GpuSnapshot, MockCollector};
use exogpuagent::http::{app_router, AppState};
use exogpuagent::pci::normalize_pci;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn temp_dir(name: &str) -> std::path::PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("ega-admin-{name}-{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn test_service_status() -> String {
    "test".into()
}

fn one_gpu() -> Vec<GpuSnapshot> {
    vec![GpuSnapshot {
        index: 0,
        name: "Mock GPU".into(),
        pci_address: normalize_pci("0000:01:00.0"),
        uuid: None,
        power_w: Some(100.0),
        temperature_c: Some(60),
        utilization_pct: Some(10),
        fan_pct: None,
    }]
}

fn state_for(dir: &std::path::Path, gui_port: u16) -> AdminState {
    state_with_collector(
        dir,
        gui_port,
        12_001,
        Arc::new(MockCollector::new(one_gpu())),
    )
}

fn state_with_collector(
    dir: &std::path::Path,
    gui_port: u16,
    telemetry_port: u16,
    collector: Arc<dyn exogpuagent::gpu::GpuCollector>,
) -> AdminState {
    let cfg_path = dir.join("config.toml");
    let cfg = Config {
        port: telemetry_port,
        gui: GuiConfig {
            enabled: true,
            bind: "127.0.0.1".into(),
            port: gui_port,
            poll_interval_secs: 5,
        },
        ..Config::default()
    };
    cfg.save(&cfg_path).unwrap();
    let auth_path = AdminAuthFile::path_beside_config(&cfg_path);
    if !AdminAuthFile::exists(&auth_path) {
        AdminAuthFile::bootstrap(&auth_path, "correct-horse-battery").unwrap();
    }

    AdminState {
        config_path: cfg_path,
        auth_path,
        config: Arc::new(std::sync::RwLock::new(cfg)),
        collector,
        cache: Arc::new(SnapshotCache::new(Duration::from_millis(20))),
        sessions: Arc::new(SessionStore::new()),
        rate_limit: Arc::new(LoginRateLimiter::new()),
        started_at: std::time::Instant::now(),
        service_status: test_service_status,
    }
}

async fn login_cookie(app: axum::Router, host: &str, origin: &str) -> (String, String) {
    let (st, body, set_cookie) = call(
        app,
        "POST",
        "/admin/api/login",
        host,
        Some(origin),
        None,
        None,
        Some(r#"{"password":"correct-horse-battery"}"#),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let cookie = extract_session_cookie(set_cookie.as_deref().expect("set-cookie"));
    let csrf: serde_json::Value = serde_json::from_str(&body).unwrap();
    let csrf = csrf["csrf_token"].as_str().unwrap().to_string();
    (cookie, csrf)
}

#[allow(clippy::too_many_arguments)]
async fn call(
    app: axum::Router,
    method: &str,
    path: &str,
    host: &str,
    origin: Option<&str>,
    cookie: Option<&str>,
    csrf: Option<&str>,
    body: Option<&str>,
) -> (StatusCode, String, Option<String>) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, host);
    if let Some(o) = origin {
        builder = builder.header(header::ORIGIN, o);
    }
    if let Some(c) = cookie {
        builder = builder.header(header::COOKIE, c);
    }
    if let Some(t) = csrf {
        builder = builder.header("x-csrf-token", t);
        builder = builder.header("x-requested-with", "ExoGPUAgentAdmin");
    }
    let req = if let Some(b) = body {
        builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    let response = app.oneshot(req).await.unwrap();
    let status = response.status();
    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        String::from_utf8_lossy(&bytes).into_owned(),
        set_cookie,
    )
}

fn extract_session_cookie(set_cookie: &str) -> String {
    set_cookie.split(';').next().unwrap().trim().to_string()
}

#[tokio::test]
async fn login_dashboard_requires_auth_and_csrf() {
    let dir = temp_dir("login");
    let state = state_for(&dir, 12_010);
    let app = admin_router(state);

    let (st, _, _) = call(
        app.clone(),
        "GET",
        "/admin/api/dashboard",
        "127.0.0.1:12010",
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    let (st, body, set_cookie) = call(
        app.clone(),
        "POST",
        "/admin/api/login",
        "127.0.0.1:12010",
        Some("http://127.0.0.1:12010"),
        None,
        None,
        Some(r#"{"password":"correct-horse-battery"}"#),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let cookie = extract_session_cookie(set_cookie.as_deref().expect("set-cookie"));
    let csrf: serde_json::Value = serde_json::from_str(&body).unwrap();
    let csrf = csrf["csrf_token"].as_str().unwrap().to_string();

    let (st, body, _) = call(
        app.clone(),
        "GET",
        "/admin/api/dashboard",
        "127.0.0.1:12010",
        None,
        Some(&cookie),
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");

    // PUT without CSRF must fail
    let (st, _, _) = call(
        app.clone(),
        "PUT",
        "/admin/api/config",
        "127.0.0.1:12010",
        Some("http://127.0.0.1:12010"),
        Some(&cookie),
        None,
        Some(r#"{"host_name":"lab"}"#),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    let (st, body, _) = call(
        app,
        "PUT",
        "/admin/api/config",
        "127.0.0.1:12010",
        Some("http://127.0.0.1:12010"),
        Some(&cookie),
        Some(&csrf),
        Some(r#"{"host_name":"lab","gui_poll_interval_secs":7}"#),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn rejects_bad_host_and_origin() {
    let dir = temp_dir("host");
    let state = state_for(&dir, 12_010);
    let app = admin_router(state);

    let (st, _, _) = call(
        app.clone(),
        "GET",
        "/",
        "evil.example:12010",
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    let (st, _, _) = call(
        app,
        "POST",
        "/admin/api/login",
        "127.0.0.1:12010",
        Some("http://evil.example"),
        None,
        None,
        Some(r#"{"password":"correct-horse-battery"}"#),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn session_without_auth_has_no_secrets() {
    let dir = temp_dir("sess");
    let state = state_for(&dir, 12_010);
    let app = admin_router(state);
    let (st, body, _) = call(
        app.clone(),
        "GET",
        "/admin/api/session",
        "127.0.0.1:12010",
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(body.contains("\"authenticated\":false"));
    assert!(!body.contains("password"));
    assert!(!body.contains("auth_token"));
    assert!(!body.contains("password_hash"));

    let (st, _, _) = call(
        app,
        "GET",
        "/admin/api/diagnostics",
        "127.0.0.1:12010",
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn wrong_password_and_rate_limit() {
    let dir = temp_dir("rl");
    let state = state_for(&dir, 12_010);
    let app = admin_router(state);

    for _ in 0..5 {
        let (st, body, _) = call(
            app.clone(),
            "POST",
            "/admin/api/login",
            "127.0.0.1:12010",
            Some("http://127.0.0.1:12010"),
            None,
            None,
            Some(r#"{"password":"wrong-password!!"}"#),
        )
        .await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "{body}");
        assert!(!body.contains("password_hash"));
    }

    let (st, body, _) = call(
        app,
        "POST",
        "/admin/api/login",
        "127.0.0.1:12010",
        Some("http://127.0.0.1:12010"),
        None,
        None,
        Some(r#"{"password":"correct-horse-battery"}"#),
    )
    .await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "{body}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn bootstrap_required_message() {
    let dir = temp_dir("noboot");
    let cfg_path = dir.join("config.toml");
    let mut cfg = Config::default();
    cfg.gui.enabled = true;
    cfg.gui.port = 12_011;
    cfg.save(&cfg_path).unwrap();
    let state = AdminState {
        config_path: cfg_path.clone(),
        auth_path: AdminAuthFile::path_beside_config(&cfg_path),
        config: Arc::new(std::sync::RwLock::new(cfg)),
        collector: Arc::new(MockCollector::new(vec![])),
        cache: Arc::new(SnapshotCache::new(Duration::from_millis(20))),
        sessions: Arc::new(SessionStore::new()),
        rate_limit: Arc::new(LoginRateLimiter::new()),
        started_at: std::time::Instant::now(),
        service_status: test_service_status,
    };
    let app = admin_router(state);
    let (st, body, _) = call(
        app,
        "POST",
        "/admin/api/login",
        "127.0.0.1:12011",
        Some("http://127.0.0.1:12011"),
        None,
        None,
        Some(r#"{"password":"correct-horse-battery"}"#),
    )
    .await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body.contains("not bootstrapped") || body.contains("gui setup"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn dashboard_telemetry_ok_from_shared_cache_not_self_http() {
    let dir = temp_dir("telok");
    let state = state_for(&dir, 12_010);
    let app = admin_router(state);
    let (cookie, _) = login_cookie(app.clone(), "127.0.0.1:12010", "http://127.0.0.1:12010").await;

    let (st, body, _) = call(
        app.clone(),
        "GET",
        "/admin/api/dashboard",
        "127.0.0.1:12010",
        None,
        Some(&cookie),
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["telemetry_http"], "collector ok", "{body}");
    assert_eq!(v["gpu_count"], 1);
    assert!(!body.contains("unreachable"));
    assert!(!body.contains("os error"));

    let (st, body, _) = call(
        app,
        "GET",
        "/admin/api/diagnostics",
        "127.0.0.1:12010",
        None,
        Some(&cookie),
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let d: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(d["telemetry_status"]["source"], "shared_cache");
    assert_eq!(d["telemetry_status"]["kind"], "collector_cache");
    assert_eq!(d["telemetry_status"]["http_listener_probed"], false);
    assert!(
        d["telemetry_status"]["detail"]
            .as_str()
            .unwrap_or("")
            .contains("NOT a live HTTP"),
        "{body}"
    );
    assert!(!body.contains("unreachable"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn dashboard_telemetry_unavailable_when_collector_fails() {
    let dir = temp_dir("telfail");
    let state = state_with_collector(
        &dir,
        12_010,
        12_001,
        Arc::new(MockCollector::failing("nvml mock fail")),
    );
    let app = admin_router(state);
    let (cookie, _) = login_cookie(app.clone(), "127.0.0.1:12010", "http://127.0.0.1:12010").await;
    let (st, body, _) = call(
        app,
        "GET",
        "/admin/api/dashboard",
        "127.0.0.1:12010",
        None,
        Some(&cookie),
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["telemetry_http"], "collector unavailable");
    assert!(!body.contains("unreachable"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Regression: blocking self-HTTP to /health on the same current_thread runtime
/// previously hung (Win 10060 / Linux EAGAIN). Dashboard must stay responsive
/// while telemetry keeps serving on the same runtime.
#[tokio::test(flavor = "current_thread")]
async fn gui_dashboard_and_telemetry_health_same_runtime() {
    let dir = temp_dir("same_rt");
    let collector: Arc<dyn exogpuagent::gpu::GpuCollector> =
        Arc::new(MockCollector::new(one_gpu()));
    let cache = Arc::new(SnapshotCache::new(Duration::from_millis(10)));

    let tel_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tel_addr = tel_listener.local_addr().unwrap();
    let tel_port = tel_addr.port();

    let tel_app = app_router(AppState {
        collector: Arc::clone(&collector),
        cache: Arc::clone(&cache),
    });
    tokio::spawn(async move {
        axum::serve(tel_listener, tel_app).await.ok();
    });

    let state = state_with_collector(&dir, 12_010, tel_port, Arc::clone(&collector));
    // Reuse the same cache instance as telemetry so GPU data is shared.
    let state = AdminState { cache, ..state };
    let admin_app = admin_router(state);
    let (cookie, _) = login_cookie(
        admin_app.clone(),
        "127.0.0.1:12010",
        "http://127.0.0.1:12010",
    )
    .await;

    let dash = tokio::time::timeout(Duration::from_secs(2), async {
        call(
            admin_app,
            "GET",
            "/admin/api/dashboard",
            "127.0.0.1:12010",
            None,
            Some(&cookie),
            None,
            None,
        )
        .await
    })
    .await
    .expect("dashboard must not deadlock on current_thread");
    assert_eq!(dash.0, StatusCode::OK, "{}", dash.1);
    let v: serde_json::Value = serde_json::from_str(&dash.1).unwrap();
    assert_eq!(v["telemetry_http"], "collector ok");

    // Telemetry listener still answers (external probe via spawn_blocking — not
    // the GUI self-check path that previously deadlocked current_thread).
    let health = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::task::spawn_blocking(move || {
            exogpuagent::local_http::get_loopback(tel_port, "/health")
        }),
    )
    .await
    .expect("telemetry /health timed out")
    .expect("join")
    .expect("telemetry /health must remain reachable");
    assert_eq!(health.0, 200);
    assert!(health.1.contains("\"status\":\"ok\""), "{}", health.1);
    let _ = tel_addr; // bind succeeded
    let _ = std::fs::remove_dir_all(&dir);
}
