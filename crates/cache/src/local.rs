use dashmap::DashMap;
use std::time::{Duration, Instant};

pub struct LocalCache {
    inner: DashMap<String, (Vec<u8>, Instant)>,
    ttl: Duration,
}

impl LocalCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: DashMap::new(),
            ttl,
        }
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.inner.get(key).and_then(|v| {
            if v.1.elapsed() < self.ttl {
                Some(v.0.clone())
            } else {
                None
            }
        })
    }

    pub fn set(&self, key: String, value: Vec<u8>) {
        self.inner.insert(key, (value, Instant::now()));
    }
}
