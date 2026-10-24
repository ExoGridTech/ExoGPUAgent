//! Simple in-memory login rate limiter (localhost peers).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(300);
const MAX_ATTEMPTS: u32 = 5;

pub struct LoginRateLimiter {
    inner: Mutex<HashMap<String, Vec<Instant>>>,
}

impl Default for LoginRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl LoginRateLimiter {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn check_and_record(&self, bucket: &str) -> bool {
        let now = Instant::now();
        let mut map = self.inner.lock().expect("rate limit mutex");
        let entry = map.entry(bucket.to_string()).or_default();
        entry.retain(|t| now.duration_since(*t) < WINDOW);
        if entry.len() as u32 >= MAX_ATTEMPTS {
            return false;
        }
        entry.push(now);
        true
    }

    pub fn clear(&self, bucket: &str) {
        self.inner.lock().expect("rate limit mutex").remove(bucket);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_max() {
        let lim = LoginRateLimiter::new();
        for _ in 0..MAX_ATTEMPTS {
            assert!(lim.check_and_record("peer"));
        }
        assert!(!lim.check_and_record("peer"));
        lim.clear("peer");
        assert!(lim.check_and_record("peer"));
    }
}
