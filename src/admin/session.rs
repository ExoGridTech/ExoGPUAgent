//! In-memory admin sessions (cookie + CSRF). Secrets are never logged.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rand::RngCore;
use sha2::{Digest, Sha256};

const SESSION_TTL: Duration = Duration::from_secs(12 * 60 * 60);
const IDLE_TTL: Duration = Duration::from_secs(2 * 60 * 60);

#[derive(Debug, Clone)]
pub struct Session {
    pub csrf_token: String,
    created: Instant,
    last_seen: Instant,
}

pub struct SessionStore {
    inner: Mutex<HashMap<String, Session>>,
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn create(&self) -> (String, Session) {
        self.purge_expired();
        let id = random_token(32);
        let csrf = random_token(32);
        let now = Instant::now();
        let session = Session {
            csrf_token: csrf.clone(),
            created: now,
            last_seen: now,
        };
        self.inner
            .lock()
            .expect("session mutex")
            .insert(id.clone(), session.clone());
        (id, session)
    }

    pub fn get(&self, id: &str) -> Option<Session> {
        self.purge_expired();
        let mut map = self.inner.lock().expect("session mutex");
        let session = map.get_mut(id)?;
        let now = Instant::now();
        if now.duration_since(session.created) > SESSION_TTL
            || now.duration_since(session.last_seen) > IDLE_TTL
        {
            map.remove(id);
            return None;
        }
        session.last_seen = now;
        Some(session.clone())
    }

    pub fn remove(&self, id: &str) {
        self.inner.lock().expect("session mutex").remove(id);
    }

    fn purge_expired(&self) {
        let now = Instant::now();
        let mut map = self.inner.lock().expect("session mutex");
        map.retain(|_, s| {
            now.duration_since(s.created) <= SESSION_TTL
                && now.duration_since(s.last_seen) <= IDLE_TTL
        });
    }
}

pub fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

/// Stable fingerprint for rate-limit buckets (not a secret).
pub fn peer_bucket(peer: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(peer.as_bytes());
    hex::encode(hasher.finalize())[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_get_remove() {
        let store = SessionStore::new();
        let (id, s) = store.create();
        assert!(!id.is_empty());
        assert!(!s.csrf_token.is_empty());
        assert!(store.get(&id).is_some());
        store.remove(&id);
        assert!(store.get(&id).is_none());
    }
}
