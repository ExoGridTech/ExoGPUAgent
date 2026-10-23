//! ExoGPUAgent library: local NVIDIA GPU telemetry over HTTP.

pub mod config;
pub mod gpu;
pub mod http;
pub mod models;
pub mod pci;

pub use config::Config;
pub use gpu::{CollectorError, GpuCollector, GpuSnapshot, MockCollector, NvmlCollector};
pub use models::{ErrorDetail, GpuInfo, GpuResponse, HealthResponse};
