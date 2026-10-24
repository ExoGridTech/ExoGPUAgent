//! Admin Mini GUI API security tests (no NVIDIA hardware).

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use exogpuagent::admin::{admin_router, AdminState, LoginRateLimiter, SessionStore, SnapshotCache};
use exogpuagent::admin_auth::AdminAuthFile;
use exogpuagent::config::{Config, GuiConfig};
use exogpuagent::gpu::MockCollector;
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

fn state_for(dir: &std::path::Path, gui_port: u16) -> AdminState {
    let cfg_path = dir.join("config.toml");
    let cfg = Config {
        port: 12_001,
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
    AdminAuthFile::bootstrap(&auth_path, "correct-horse-battery").unwrap();

    AdminState {
        config_path: cfg_path,
        auth_path,
        config: Arc::new(std::sync::RwLock::new(cfg)),
        collector: Arc::new(MockCollector::new(vec![])),
        cache: Arc::new(SnapshotCache::new(Duration::from_millis(20))),
        sessions: Arc::new(SessionStore::new()),
        rate_limit: Arc::new(LoginRateLimiter::new()),
        started_at: std::time::Instant::now(),
        service_status: test_service_status,
    }
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
