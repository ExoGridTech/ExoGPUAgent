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
    pub service: String,
    pub nvml: String,
    pub gpu_count: Option<usize>,
    pub health_http: String,
    pub notes: Vec<String>,
}

/// Options for [`run_doctor`].
#[derive(Debug, Clone, Default)]
pub struct DoctorOptions {
    /// When false, skip the loopback HTTP probe (required for in-process Mini GUI
    /// diagnostics on a Tokio `current_thread` runtime to avoid self-deadlock).
    pub probe_http: bool,
    /// Optional precomputed health line (e.g. from shared cache).
    pub health_override: Option<String>,
}

impl DoctorOptions {
    /// CLI / external process: probe `127.0.0.1:<port>/health` over TCP.
    pub fn cli() -> Self {
        Self {
            probe_http: true,
            health_override: None,
        }
    }

    /// In-process Mini GUI: never block on self-HTTP.
    pub fn in_process(health_line: String) -> Self {
        Self {
            probe_http: false,
            health_override: Some(health_line),
        }
    }
}

pub fn run_doctor(config_path: &Path) -> DoctorReport {
    run_doctor_with(config_path, DoctorOptions::cli())
}

pub fn run_doctor_with(config_path: &Path, opts: DoctorOptions) -> DoctorReport {
    let mut notes = Vec::new();
    // Hard failures only: invalid config / net policy. NVML absence is a soft note
    // (daemon may still run and serve 503 on /gpu).
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

    let service = crate::service::service_state_string();

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

    let health_http = if let Some(override_line) = opts.health_override {
        notes
            .push("telemetry health from in-process shared cache (HTTP self-probe skipped)".into());
        override_line
    } else if opts.probe_http {
        match local_http::get_json_loopback(port, "/health") {
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
        }
    } else {
        "HTTP probe skipped".into()
    };

    if opts.probe_http && port_free == Some(false) && health_http.starts_with("port busy") {
        notes.push(format!(
            "port {bind}:{port} is in use but did not answer /health — resolve the conflict or change port in config.toml"
        ));
    }

    notes.push(
        "Phase 2C binds localhost only; no firewall rules, Tailscale, or remote access.".into(),
    );
    notes.push("No cloud telemetry; no AtlasGPU dependency.".into());
    if config.gui.enabled {
        notes.push(format!(
            "Mini GUI enabled on {}:{} (admin auth file must exist beside config)",
            config.gui.bind, config.gui.port
        ));
        let auth = crate::admin_auth::AdminAuthFile::path_beside_config(config_path);
        if !auth.exists() {
            notes.push(
                "gui.enabled=true but admin_auth.toml missing — run: exogpuagent gui setup".into(),
            );
            ok = false;
        }
    } else {
        notes.push("Mini GUI disabled (gui.enabled=false).".into());
    }

    DoctorReport {
        ok,
        config_path: config_path.display().to_string(),
        config_summary,
        net_policy: net_policy_msg,
        port_free_for_bind: port_free,
        service,
        nvml,
        gpu_count,
        health_http,
        notes,
    }
}
