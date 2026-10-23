//! In-memory collector for unit tests (no NVIDIA hardware).

use super::{CollectorError, GpuCollector, GpuSnapshot};
use std::sync::Mutex;

/// Configurable mock used by HTTP and collector tests.
#[derive(Debug, Default)]
pub struct MockCollector {
    inner: Mutex<MockState>,
}

#[derive(Debug, Default)]
struct MockState {
    fail: Option<String>,
    gpus: Vec<GpuSnapshot>,
}

impl MockCollector {
    pub fn new(gpus: Vec<GpuSnapshot>) -> Self {
        Self {
            inner: Mutex::new(MockState { fail: None, gpus }),
        }
    }

    pub fn failing(message: impl Into<String>) -> Self {
        Self {
            inner: Mutex::new(MockState {
                fail: Some(message.into()),
                gpus: Vec::new(),
            }),
        }
    }

    pub fn set_gpus(&self, gpus: Vec<GpuSnapshot>) {
        let mut state = self.inner.lock().expect("mock poisoned");
        state.gpus = gpus;
        state.fail = None;
    }

    pub fn set_failure(&self, message: impl Into<String>) {
        let mut state = self.inner.lock().expect("mock poisoned");
        state.fail = Some(message.into());
    }
}

impl GpuCollector for MockCollector {
    fn gpu_count(&self) -> Result<usize, CollectorError> {
        let state = self.inner.lock().expect("mock poisoned");
        if let Some(msg) = &state.fail {
            return Err(CollectorError::Unavailable(msg.clone()));
        }
        Ok(state.gpus.len())
    }

    fn collect(&self) -> Result<Vec<GpuSnapshot>, CollectorError> {
        let state = self.inner.lock().expect("mock poisoned");
        if let Some(msg) = &state.fail {
            return Err(CollectorError::Unavailable(msg.clone()));
        }
        Ok(state.gpus.clone())
    }
}
