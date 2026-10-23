//! Local diagnostics (`exogpuagent doctor`) — does not stop a running service.

use std::net::TcpListener;
use std::path::Path;

use serde::Serialize;

use crate::config::Config;
use crate::gpu::{GpuCollector, NvmlCollector};
use crate::local_http;
use crate::net_policy;

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub ok: bool,
    pub config_path: String,
    pub config_summary: String,
    pub net_policy: String,
    pub port_free_for_bind: Option<bool>,
    pub nvml: String,
    pub gpu_count: Option<usize>,
    pub health_http: String,
    pub notes: Vec<String>,
}

pub fn run_doctor(config_path: &Path) -> DoctorReport {
    let mut notes = Vec::new();
    let mut ok = true;

    let config = if config_path.exists() {
        match Config::load(config_path) {
            Ok(c) => c,
            Err(e) => {
                ok = false;
                notes.push(format!("config load failed: {e}"));
                Config::default()
            }
        }
    } else {
        notes.push(format!(
            "config not found at {}; using defaults for checks (not written)",
            config_path.display()
        ));
        Config::default()
    };

    let config_summary = config.safe_summary();
    let net_policy_msg = match net_policy::validate(&config) {
        Ok(()) => "ok (localhost)".to_string(),
        Err(e) => {
            ok = false;
            format!("FAIL: {e}")
        }
    };

    let port = config.port;
    let bind = config.bind.clone();

    let port_free = match TcpListener::bind((bind.as_str(), port)) {
        Ok(listener) => {
            drop(listener);
            Some(true)
        }
        Err(_) => Some(false),
    };

    let (nvml, gpu_count) = match NvmlCollector::init() {
        Ok(c) => match c.gpu_count() {
            Ok(n) => ("ok".into(), Some(n)),
            Err(e) => {
                notes.push(format!("NVML count failed: {e}"));
                (format!("init ok but count failed: {e}"), None)
            }
        },
        Err(e) => {
            notes.push(
                "NVIDIA driver / NVML not available. Install a current NVIDIA driver; ExoGPUAgent does not install drivers."
                    .into(),
            );
            (format!("unavailable: {e}"), None)
        }
    };

    let health_http = match local_http::get_json_loopback(port, "/health") {
        Ok((status, body)) => {
            if status == 200 {
                format!("ok HTTP {status} {body}")
            } else {
                format!("HTTP {status} {body}")
            }
        }
        Err(e) => {
            if port_free == Some(false) {
                format!("port busy but /health failed: {e}")
            } else {
                format!("not listening ({e})")
            }
        }
    };

    notes.push("Phase 2B binds localhost only; no firewall rules or remote access.".into());
    notes.push("No cloud telemetry; no AtlasGPU dependency.".into());

    DoctorReport {
        ok,
        config_path: config_path.display().to_string(),
        config_summary,
        net_policy: net_policy_msg,
        port_free_for_bind: port_free,
        nvml,
        gpu_count,
        health_http,
        notes,
    }
}
