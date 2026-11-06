use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use etcetera::base_strategy::{BaseStrategy, choose_base_strategy};
use rootcause::Result;
use serde::{Deserialize, Serialize};
use tracing::debug;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub etag: String,
    pub body: String,
}

/// Response bodies keyed by request, revalidated with `If-None-Match`; GitHub does not charge quota for a 304.
pub struct EtagCache {
    path: Option<PathBuf>,
    entries: Mutex<HashMap<String, Entry>>,
    dirty: AtomicBool,
}

impl EtagCache {
    pub fn default_path() -> Option<PathBuf> {
        choose_base_strategy()
            .ok()
            .map(|strategy| strategy.cache_dir().join("nix-package-updater").join("github-etags.json"))
    }

    /// A missing or unreadable cache file starts empty: every entry is revalidated anyway.
    pub fn load(path: Option<PathBuf>) -> Self {
        let entries = path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|content| {
                serde_json::from_str(&content)
                    .inspect_err(|e| debug!("Ignoring unreadable ETag cache: {e}"))
                    .ok()
            })
            .unwrap_or_default();

        Self {
            path,
            entries: Mutex::new(entries),
            dirty: AtomicBool::new(false),
        }
    }

    pub fn get(&self, key: &str) -> Option<Entry> {
        self.entries.lock().expect("ETag cache lock poisoned").get(key).cloned()
    }

    pub fn insert(&self, key: String, entry: Entry) {
        self.entries.lock().expect("ETag cache lock poisoned").insert(key, entry);
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn save(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };

        if !self.dirty.load(Ordering::Relaxed) {
            return Ok(());
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string(&*self.entries.lock().expect("ETag cache lock poisoned"))?;

        // Rename so a concurrent run never reads a half-written file.
        let tmp = path.with_extension(format!("json.{}", std::process::id()));
        fs::write(&tmp, content)?;
        fs::rename(&tmp, path)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{EtagCache, Entry};

    fn entry(etag: &str) -> Entry {
        Entry {
            etag: etag.to_string(),
            body: format!("body-{etag}"),
        }
    }

    #[test]
    fn round_trips_entries_through_disk() {
        let dir = std::env::temp_dir().join(format!("npu-etag-test-{}", std::process::id()));
        let path = dir.join("nested").join("cache.json");

        let cache = EtagCache::load(Some(path.clone()));
        cache.insert("a".to_string(), entry("1"));
        cache.save().unwrap();

        let reloaded = EtagCache::load(Some(path));
        assert_eq!(reloaded.get("a"), Some(entry("1")));
        assert_eq!(reloaded.get("b"), None);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_file_loads_empty() {
        let dir = std::env::temp_dir().join(format!("npu-etag-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cache.json");
        std::fs::write(&path, "{not json").unwrap();

        assert_eq!(EtagCache::load(Some(path)).get("a"), None);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unchanged_cache_is_not_written() {
        let dir = std::env::temp_dir().join(format!("npu-etag-clean-{}", std::process::id()));
        let path = dir.join("cache.json");

        EtagCache::load(Some(path.clone())).save().unwrap();

        assert!(!path.exists());
    }
}
