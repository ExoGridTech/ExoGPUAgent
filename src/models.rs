//! Public JSON contract compatible with the original Python agent.

use serde::{Deserialize, Serialize};

/// `GET /health` success body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub gpu_count: usize,
}

/// Single GPU entry in `GET /gpu`.
///
/// Sensor fields are `null` when unavailable (never fail the whole response).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuInfo {
    pub index: u32,
    pub name: String,
    pub pci_address: String,
    pub power_w: Option<f64>,
    pub temperature_c: Option<i32>,
    pub utilization_pct: Option<i32>,
    pub fan_pct: Option<i32>,
}

/// `GET /gpu` success body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuResponse {
    pub gpu_count: usize,
    pub total_gpu_power_w: f64,
    pub gpus: Vec<GpuInfo>,
}

impl GpuResponse {
    pub fn from_gpus(gpus: Vec<GpuInfo>) -> Self {
        let total_power: f64 = gpus.iter().filter_map(|g| g.power_w).sum();
        let total_gpu_power_w = (total_power * 100.0).round() / 100.0;
        Self {
            gpu_count: gpus.len(),
            total_gpu_power_w,
            gpus,
        }
    }
}

/// FastAPI-compatible error body for HTTP 503.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorDetail {
    pub detail: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_power_rounds_to_two_decimals() {
        let gpus = vec![
            GpuInfo {
                index: 0,
                name: "A".into(),
                pci_address: "01:00.0".into(),
                power_w: Some(10.555),
                temperature_c: None,
                utilization_pct: None,
                fan_pct: None,
            },
            GpuInfo {
                index: 1,
                name: "B".into(),
                pci_address: "02:00.0".into(),
                power_w: Some(20.111),
                temperature_c: None,
                utilization_pct: None,
                fan_pct: None,
            },
        ];
        let resp = GpuResponse::from_gpus(gpus);
        assert_eq!(resp.total_gpu_power_w, 30.67);
        assert_eq!(resp.gpu_count, 2);
    }

    #[test]
    fn total_power_zero_when_all_null() {
        let gpus = vec![GpuInfo {
            index: 0,
            name: "A".into(),
            pci_address: "01:00.0".into(),
            power_w: None,
            temperature_c: Some(50),
            utilization_pct: Some(10),
            fan_pct: None,
        }];
        let resp = GpuResponse::from_gpus(gpus);
        assert_eq!(resp.total_gpu_power_w, 0.0);
    }

    #[test]
    fn json_omits_no_extra_fields() {
        let gpu = GpuInfo {
            index: 0,
            name: "GPU".into(),
            pci_address: "01:00.0".into(),
            power_w: Some(100.0),
            temperature_c: Some(60),
            utilization_pct: Some(5),
            fan_pct: None,
        };
        let value = serde_json::to_value(&gpu).unwrap();
        let obj = value.as_object().unwrap();
        assert_eq!(obj.len(), 7);
        assert!(obj["fan_pct"].is_null());
    }
}
