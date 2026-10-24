//! Axum routes compatible with the original FastAPI agent contract.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};

use crate::admin::SnapshotCache;
use crate::gpu::{CollectorError, GpuCollector};
use crate::models::{ErrorDetail, GpuResponse, HealthResponse};

/// Shared application state. Collector is initialized once at startup.
#[derive(Clone)]
pub struct AppState {
    pub collector: Arc<dyn GpuCollector>,
    pub cache: Arc<SnapshotCache>,
}

pub fn app_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/gpu", get(gpu))
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> Response {
    let collector = Arc::clone(&state.collector);
    let cache = Arc::clone(&state.cache);
    match run_blocking(move || cache.gpu_count(collector.as_ref())).await {
        Ok(gpu_count) => Json(HealthResponse {
            status: "ok".to_string(),
            gpu_count,
        })
        .into_response(),
        Err(err) => service_unavailable(err),
    }
}

async fn gpu(State(state): State<AppState>) -> Response {
    let collector = Arc::clone(&state.collector);
    let cache = Arc::clone(&state.cache);
    match run_blocking(move || cache.get(collector.as_ref())).await {
        Ok(snapshots) => {
            let gpus = snapshots.into_iter().map(|s| s.into_public()).collect();
            Json(GpuResponse::from_gpus(gpus)).into_response()
        }
        Err(err) => service_unavailable(err),
    }
}

fn service_unavailable(err: CollectorError) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorDetail {
            detail: err.to_string(),
        }),
    )
        .into_response()
}

/// Run NVML / collector work off the async runtime.
async fn run_blocking<T, F>(f: F) -> Result<T, CollectorError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, CollectorError> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| CollectorError::Unavailable(format!("worker join error: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::{GpuSnapshot, MockCollector};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn sample_gpus() -> Vec<GpuSnapshot> {
        vec![
            GpuSnapshot {
                index: 0,
                name: "NVIDIA GeForce RTX 4090".into(),
                pci_address: "01:00.0".into(),
                uuid: Some("GPU-aaaa".into()),
                power_w: Some(120.5),
                temperature_c: Some(65),
                utilization_pct: Some(40),
                fan_pct: None,
            },
            GpuSnapshot {
                index: 1,
                name: "NVIDIA GeForce RTX 3080".into(),
                pci_address: "02:00.0".into(),
                uuid: Some("GPU-bbbb".into()),
                power_w: Some(80.25),
                temperature_c: Some(55),
                utilization_pct: Some(10),
                fan_pct: Some(30),
            },
        ]
    }

    async fn json_body(response: Response) -> serde_json::Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn test_state(collector: Arc<dyn crate::gpu::GpuCollector>) -> AppState {
        AppState {
            collector,
            cache: Arc::new(crate::admin::SnapshotCache::new(
                std::time::Duration::from_millis(50),
            )),
        }
    }

    #[tokio::test]
    async fn health_ok() {
        let state = test_state(Arc::new(MockCollector::new(sample_gpus())));
        let app = app_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["status"], "ok");
        assert_eq!(body["gpu_count"], 2);
    }

    #[tokio::test]
    async fn gpu_ok_and_omits_uuid() {
        let state = test_state(Arc::new(MockCollector::new(sample_gpus())));
        let app = app_router(state);
        let response = app
            .oneshot(Request::builder().uri("/gpu").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["gpu_count"], 2);
        assert_eq!(body["total_gpu_power_w"], 200.75);
        let gpus = body["gpus"].as_array().unwrap();
        assert_eq!(gpus.len(), 2);
        assert!(gpus[0].get("uuid").is_none());
        assert_eq!(gpus[0]["fan_pct"], serde_json::Value::Null);
        assert_eq!(gpus[0]["pci_address"], "01:00.0");
        assert_eq!(gpus[1]["fan_pct"], 30);
    }

    #[tokio::test]
    async fn health_503_fastapi_shape() {
        let state = test_state(Arc::new(MockCollector::failing("NVML init failed")));
        let app = app_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = json_body(response).await;
        assert_eq!(body, serde_json::json!({"detail": "NVML init failed"}));
    }

    #[tokio::test]
    async fn gpu_503_fastapi_shape() {
        let state = test_state(Arc::new(MockCollector::failing("driver missing")));
        let app = app_router(state);
        let response = app
            .oneshot(Request::builder().uri("/gpu").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = json_body(response).await;
        assert_eq!(body["detail"], "driver missing");
    }
}
