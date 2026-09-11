use crate::storage::StorageEngine;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct EmbeddingCache {
    storage: StorageEngine,
    hits: Arc<AtomicU64>,
    misses: Arc<AtomicU64>,
}

impl EmbeddingCache {
    pub fn new(storage: StorageEngine) -> Self {
        Self {
            storage,
            hits: Arc::new(AtomicU64::new(0)),
            misses: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Computes deterministic cache key: SHA256(model_name + "::" + model_version + "::" + normalized_text)
    pub fn cache_key(model_name: &str, model_version: &str, text: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(model_name.as_bytes());
        hasher.update(b"::");
        hasher.update(model_version.as_bytes());
        hasher.update(b"::");
        hasher.update(text.trim().to_lowercase().as_bytes());
        let hash = hasher.finalize();
        hex::encode(hash)
    }

    pub fn get(&self, model_name: &str, model_version: &str, text: &str) -> Result<Option<Vec<f32>>> {
        let key = Self::cache_key(model_name, model_version, text);
        let cached: Option<Vec<f32>> = self.storage.get("_embedding_cache", &key)?;
        if cached.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }
        Ok(cached)
    }

    pub fn put(&self, model_name: &str, model_version: &str, text: &str, vector: &[f32]) -> Result<()> {
        let key = Self::cache_key(model_name, model_version, text);
        self.storage.set("_embedding_cache", &key, &vector.to_vec())?;
        Ok(())
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.hits.load(Ordering::Relaxed), self.misses.load(Ordering::Relaxed))
    }
}

mod hex {
    pub fn encode<T: AsRef<[u8]>>(data: T) -> String {
        data.as_ref().iter().map(|b| format!("{:02x}", b)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_cache() {
        let storage = StorageEngine::new_temp().unwrap();
        let cache = EmbeddingCache::new(storage);

        let vec = vec![0.1f32, 0.2, 0.3];
        cache.put("minilm", "v1", "hello world", &vec).unwrap();

        let hit = cache.get("minilm", "v1", "hello world").unwrap();
        assert!(hit.is_some());
        assert_eq!(hit.unwrap(), vec);

        // Different version -> cache miss
        let miss_version = cache.get("minilm", "v2", "hello world").unwrap();
        assert!(miss_version.is_none());
    }
}
