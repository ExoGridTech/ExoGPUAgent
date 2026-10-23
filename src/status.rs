//! `exogpuagent status` — service + local HTTP health.

use std::path::Path;

use serde::Serialize;

use crate::config::Config;
use crate::local_http;

#[derive(Debug, Serialize)]
pub struct StatusReport {
    pub config_path: String,
    pub bind: String,
    pub port: u16,
    pub service: String,
    pub health: String,
    pub gpu_endpoint: String,
}

pub fn run_status(config_path: &Path) -> StatusReport {
    let (bind, port) = if config_path.exists() {
        match Config::load(config_path) {
            Ok(c) => (c.bind, c.port),
            Err(_) => ("127.0.0.1".into(), 12_001),
        }
    } else {
        ("127.0.0.1".into(), 12_001)
    };

    let service = crate::service::service_state_string();

    let health = match local_http::get_json_loopback(port, "/health") {
        Ok((code, body)) => format!("HTTP {code} {body}"),
        Err(e) => format!("unreachable ({e})"),
    };

    let gpu_endpoint = match local_http::get_json_loopback(port, "/gpu") {
        Ok((code, body)) => {
            let count = body
                .get("gpu_count")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            format!("HTTP {code} gpu_count={count}")
        }
        Err(e) => format!("unreachable ({e})"),
    };

    StatusReport {
        config_path: config_path.display().to_string(),
        bind,
        port,
        service,
        health,
        gpu_endpoint,
    }
}
