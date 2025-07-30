use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]

pub struct Cache<K, V> {
    data: Arc<Mutex<HashMap<K, (V, Instant)>>>,
    ttl: Duration,
}

impl<K: Eq + std::hash::Hash + Clone, V: Clone> Cache<K, V> {
    pub fn new(ttl: Duration) -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
            ttl,
        }
    }

    pub fn get(&self, key: &K) -> Option<V> {
        let mut data = self.data.lock().unwrap();

        if let Some((value, timestamp)) = data.get(key) {
            if timestamp.elapsed() < self.ttl {
                return Some(value.clone());
            } else {
                data.remove(key);
            }
        }

        None
    }

    pub fn insert(&self, key: K, value: V) {
        let mut data = self.data.lock().unwrap();

        data.insert(key, (value, Instant::now()));
    }

    pub fn clear(&self) {
        let mut data = self.data.lock().unwrap();

        data.clear();
    }
}
