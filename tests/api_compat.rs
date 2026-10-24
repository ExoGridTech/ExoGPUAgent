//! Contract tests: JSON shape matches the original Python FastAPI agent.
//! Runs entirely with MockCollector — no NVIDIA hardware required.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use exogpuagent::gpu::{GpuSnapshot, MockCollector};
use exogpuagent::http::{app_router, AppState};
use exogpuagent::pci::normalize_pci;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn multi_gpu_fixture() -> Vec<GpuSnapshot> {
    vec![
        GpuSnapshot {
            index: 0,
            name: "NVIDIA GeForce RTX 4090".into(),
            pci_address: normalize_pci("0000:01:00.0"),
            uuid: Some("GPU-1111".into()),
            power_w: Some(250.333),
            temperature_c: Some(70),
            utilization_pct: Some(99),
            fan_pct: None,
        },
        GpuSnapshot {
            index: 1,
            name: "NVIDIA Tesla T4".into(),
            pci_address: normalize_pci("0000:3b:00.0"),
            uuid: Some("GPU-2222".into()),
            power_w: None,
            temperature_c: None,
            utilization_pct: Some(0),
            fan_pct: Some(0),
        },
    ]
}

async fn get_json(app: axum::Router, path: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    (status, body)
}

fn test_app(collector: Arc<dyn exogpuagent::gpu::GpuCollector>) -> axum::Router {
    app_router(AppState {
        collector,
        cache: Arc::new(exogpuagent::admin::SnapshotCache::new(
            std::time::Duration::from_millis(10),
        )),
    })
}

#[tokio::test]
async fn health_contract() {
    let app = test_app(Arc::new(MockCollector::new(multi_gpu_fixture())));
    let (status, body) = get_json(app, "/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!({
            "status": "ok",
            "gpu_count": 2
        })
    );
}

#[tokio::test]
async fn gpu_contract_null_sensors_and_no_uuid() {
    let app = test_app(Arc::new(MockCollector::new(multi_gpu_fixture())));
    let (status, body) = get_json(app, "/gpu").await;
    assert_eq!(status, StatusCode::OK);

    // 250.333 + 0 (null skipped) → round(250.333, 2) = 250.33
    assert_eq!(body["gpu_count"], 2);
    assert_eq!(body["total_gpu_power_w"], 250.33);

    let g0 = &body["gpus"][0];
    assert_eq!(g0["index"], 0);
    assert_eq!(g0["name"], "NVIDIA GeForce RTX 4090");
    assert_eq!(g0["pci_address"], "01:00.0");
    assert_eq!(g0["power_w"], 250.333);
    assert_eq!(g0["temperature_c"], 70);
    assert_eq!(g0["utilization_pct"], 99);
    assert!(g0["fan_pct"].is_null());
    assert!(g0.get("uuid").is_none());

    let g1 = &body["gpus"][1];
    assert_eq!(g1["pci_address"], "3B:00.0");
    assert!(g1["power_w"].is_null());
    assert!(g1["temperature_c"].is_null());
    assert_eq!(g1["fan_pct"], 0);
}

#[tokio::test]
async fn error_contract_matches_fastapi_detail() {
    let app = test_app(Arc::new(MockCollector::failing(
        "Command '['nvidia-smi']' returned non-zero exit status 1.",
    )));
    let (status, body) = get_json(app, "/gpu").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        body,
        serde_json::json!({
            "detail": "Command '['nvidia-smi']' returned non-zero exit status 1."
        })
    );
}
