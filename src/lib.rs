//! ExoGPUAgent library: local NVIDIA GPU telemetry over HTTP.

pub mod admin;
pub mod admin_auth;
pub mod config;
pub mod config_cmd;
pub mod doctor;
pub mod gpu;
pub mod http;
pub mod local_http;
pub mod models;
pub mod net_policy;
pub mod pci;
pub mod server;
pub mod service;
pub mod service_log;
pub mod status;

pub use config::{AccessMode, Config, GuiConfig};
pub use gpu::{CollectorError, GpuCollector, GpuSnapshot, MockCollector, NvmlCollector};
pub use models::{ErrorDetail, GpuInfo, GpuResponse, HealthResponse};
