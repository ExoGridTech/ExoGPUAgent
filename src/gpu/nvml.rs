//! NVML-backed collector. Library is initialized once at construction time.

use super::{CollectorError, GpuCollector, GpuSnapshot};
use crate::pci::normalize_pci;
use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
use nvml_wrapper::error::NvmlError;
use nvml_wrapper::Nvml;
use std::sync::Mutex;

/// Holds a single [`Nvml`] instance for the process lifetime of the agent.
pub struct NvmlCollector {
    /// Mutex so blocking NVML calls from `spawn_blocking` stay exclusive
    /// without tying up the Tokio scheduler beyond the blocking pool.
    nvml: Mutex<Nvml>,
}

impl NvmlCollector {
    /// Initialize NVML once. Call from startup (or a blocking context), not
    /// from an async request handler.
    pub fn init() -> Result<Self, CollectorError> {
        let nvml = Nvml::init().map_err(map_nvml_error)?;
        Ok(Self {
            nvml: Mutex::new(nvml),
        })
    }
}

impl GpuCollector for NvmlCollector {
    fn gpu_count(&self) -> Result<usize, CollectorError> {
        let nvml = self
            .nvml
            .lock()
            .map_err(|_| CollectorError::Unavailable("NVML lock poisoned".to_string()))?;
        let count = nvml.device_count().map_err(map_nvml_error)?;
        Ok(count as usize)
    }

    fn collect(&self) -> Result<Vec<GpuSnapshot>, CollectorError> {
        let nvml = self
            .nvml
            .lock()
            .map_err(|_| CollectorError::Unavailable("NVML lock poisoned".to_string()))?;

        let count = nvml.device_count().map_err(map_nvml_error)?;
        let mut gpus = Vec::with_capacity(count as usize);

        for index in 0..count {
            // Per-GPU isolation: a failure opening one device skips it.
            let device = match nvml.device_by_index(index) {
                Ok(d) => d,
                Err(_) => continue,
            };

            let name = device.name().unwrap_or_else(|_| "Unknown".to_string());

            let pci_address = device
                .pci_info()
                .map(|p| normalize_pci(&p.bus_id))
                .unwrap_or_default();

            // Captured for future stable ID; not exposed in public JSON yet.
            let uuid = device.uuid().ok();

            let power_w = device.power_usage().ok().map(|mw| f64::from(mw) / 1000.0);

            let temperature_c = device
                .temperature(TemperatureSensor::Gpu)
                .ok()
                .map(|t| t as i32);

            let utilization_pct = device.utilization_rates().ok().map(|u| u.gpu as i32);

            // Fan index 0; NotSupported / NotFound → null (laptop GPUs, etc.).
            let fan_pct = device.fan_speed(0).ok().map(|f| f as i32);

            gpus.push(GpuSnapshot {
                index,
                name,
                pci_address,
                uuid,
                power_w,
                temperature_c,
                utilization_pct,
                fan_pct,
            });
        }

        Ok(gpus)
    }
}

fn map_nvml_error(err: NvmlError) -> CollectorError {
    CollectorError::Unavailable(err.to_string())
}
