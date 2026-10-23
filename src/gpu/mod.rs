//! GPU telemetry collection via NVML (or mocks for tests).

mod mock;
mod nvml;

pub use mock::MockCollector;
pub use nvml::NvmlCollector;

use crate::models::GpuInfo;
use thiserror::Error;

/// Internal per-GPU snapshot.
///
/// `uuid` is captured for future stable identification but is **not** part of
/// the public JSON contract (`GpuInfo`).
#[derive(Debug, Clone, PartialEq)]
pub struct GpuSnapshot {
    pub index: u32,
    pub name: String,
    pub pci_address: String,
    /// NVIDIA GPU UUID (e.g. `GPU-xxxxxxxx-....`). Reserved for future use.
    pub uuid: Option<String>,
    pub power_w: Option<f64>,
    pub temperature_c: Option<i32>,
    pub utilization_pct: Option<i32>,
    pub fan_pct: Option<i32>,
}

impl GpuSnapshot {
    /// Map to the public JSON-compatible struct (UUID intentionally dropped).
    pub fn into_public(self) -> GpuInfo {
        GpuInfo {
            index: self.index,
            name: self.name,
            pci_address: self.pci_address,
            power_w: self.power_w,
            temperature_c: self.temperature_c,
            utilization_pct: self.utilization_pct,
            fan_pct: self.fan_pct,
        }
    }
}

#[derive(Debug, Error)]
pub enum CollectorError {
    #[error("{0}")]
    Unavailable(String),
}

/// Abstraction over NVML so tests can run without NVIDIA hardware.
pub trait GpuCollector: Send + Sync {
    /// Number of GPUs currently visible. Fails if the backend is unavailable.
    fn gpu_count(&self) -> Result<usize, CollectorError>;

    /// Collect telemetry for all GPUs. Per-sensor failures become `None`.
    /// A completely missing/failed GPU should be skipped rather than aborting
    /// the whole list when other GPUs remain readable; a total backend failure
    /// returns [`CollectorError`].
    fn collect(&self) -> Result<Vec<GpuSnapshot>, CollectorError>;
}
