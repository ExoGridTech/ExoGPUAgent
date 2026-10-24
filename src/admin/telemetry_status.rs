//! Collector / NVML status for the Mini GUI via shared cache (no self-HTTP).
//!
//! This reflects whether the in-process collector can produce GPU data — the same
//! data path as `GET /health` / `GET /gpu`. It is **not** a live TCP probe of the
//! telemetry listener. Blocking self-HTTP on Tokio `current_thread` starves the
//! server (Windows WSAETIMEDOUT / Linux EAGAIN).

use crate::gpu::{CollectorError, GpuSnapshot};

/// Constant source tag for Diagnostics JSON.
pub const SOURCE_SHARED_CACHE: &str = "shared_cache";

/// Dashboard shows [`TelemetryStatus::brief`]; Diagnostics shows the full struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryStatus {
    /// Short UI label (collector state only).
    pub brief: String,
    /// Technical explanation for Diagnostics.
    pub detail: String,
    /// Collector/cache produced data successfully.
    pub ok: bool,
    pub gpu_count: Option<usize>,
}

impl TelemetryStatus {
    /// Build status from the shared-cache result already used by the dashboard
    /// (no extra NVML read, no HTTP).
    pub fn from_cache_result(result: &Result<Vec<GpuSnapshot>, CollectorError>) -> Self {
        match result {
            Ok(snaps) => Self {
                brief: "collector ok".into(),
                detail: format!(
                    "source={SOURCE_SHARED_CACHE}; NVML/collector reachable in-process \
                     (same data path as GET /health); gpu_count={}. \
                     This is NOT a live HTTP connectivity check of the telemetry listener.",
                    snaps.len()
                ),
                ok: true,
                gpu_count: Some(snaps.len()),
            },
            Err(e) => Self {
                brief: "collector unavailable".into(),
                detail: format!(
                    "source={SOURCE_SHARED_CACHE}; collector/cache error \
                     (GET /health would return 503): {e}. \
                     This is NOT a live HTTP connectivity check of the telemetry listener."
                ),
                ok: false,
                gpu_count: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::GpuSnapshot;
    use crate::pci::normalize_pci;

    #[test]
    fn ok_brief_is_collector_not_http() {
        let snaps = vec![GpuSnapshot {
            index: 0,
            name: "Mock".into(),
            pci_address: normalize_pci("0000:01:00.0"),
            uuid: None,
            power_w: None,
            temperature_c: None,
            utilization_pct: None,
            fan_pct: None,
        }];
        let s = TelemetryStatus::from_cache_result(&Ok(snaps));
        assert!(s.ok);
        assert_eq!(s.brief, "collector ok");
        assert_eq!(s.gpu_count, Some(1));
        assert!(s.detail.contains(SOURCE_SHARED_CACHE));
        assert!(s.detail.contains("NOT a live HTTP"));
        assert!(!s.detail.contains("unreachable"));
    }

    #[test]
    fn unavailable_brief_on_collector_error() {
        let s = TelemetryStatus::from_cache_result(&Err(CollectorError::Unavailable(
            "nvml down".into(),
        )));
        assert!(!s.ok);
        assert_eq!(s.brief, "collector unavailable");
        assert!(s.detail.contains(SOURCE_SHARED_CACHE));
        assert!(s.detail.contains("503") || s.detail.contains("nvml"));
    }
}
