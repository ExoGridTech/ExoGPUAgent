//! Shared GPU snapshot cache so telemetry and admin GUI share one NVML reader.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::gpu::{CollectorError, GpuCollector, GpuSnapshot};

pub struct SnapshotCache {
    inner: Mutex<CacheInner>,
    min_interval: Duration,
}

struct CacheInner {
    last: Option<(Instant, Result<Vec<GpuSnapshot>, String>)>,
}

impl SnapshotCache {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            inner: Mutex::new(CacheInner { last: None }),
            min_interval,
        }
    }

    pub fn get(&self, collector: &dyn GpuCollector) -> Result<Vec<GpuSnapshot>, CollectorError> {
        let now = Instant::now();
        {
            let guard = self.inner.lock().expect("snapshot mutex");
            if let Some((ts, cached)) = &guard.last {
                if now.duration_since(*ts) < self.min_interval {
                    return match cached {
                        Ok(v) => Ok(v.clone()),
                        Err(e) => Err(CollectorError::Unavailable(e.clone())),
                    };
                }
            }
        }

        let fresh = collector.collect().map_err(|e| e.to_string());
        let mut guard = self.inner.lock().expect("snapshot mutex");
        guard.last = Some((now, fresh.clone()));
        match fresh {
            Ok(v) => Ok(v),
            Err(e) => Err(CollectorError::Unavailable(e)),
        }
    }

    pub fn gpu_count(&self, collector: &dyn GpuCollector) -> Result<usize, CollectorError> {
        Ok(self.get(collector)?.len())
    }
}
