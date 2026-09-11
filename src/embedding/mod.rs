pub mod provider;
pub mod cache;
pub mod registry;

use crate::storage::StorageEngine;
use anyhow::Result;
pub use cache::EmbeddingCache;
pub use provider::{EmbeddingProvider, LocalSemanticProvider, OllamaProvider, OpenAIProvider};
pub use registry::{ModelConfig, ModelRegistry};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct EmbeddingManager {
    registry: ModelRegistry,
    cache: EmbeddingCache,
    providers: Arc<RwLock<HashMap<String, Arc<dyn EmbeddingProvider>>>>,
}

impl EmbeddingManager {
    pub fn new(storage: StorageEngine) -> Self {
        let registry = ModelRegistry::new(storage.clone());
        let cache = EmbeddingCache::new(storage);
        Self {
            registry,
            cache,
            providers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn registry(&self) -> &ModelRegistry {
        &self.registry
    }

    pub fn cache(&self) -> &EmbeddingCache {
        &self.cache
    }

    /// Resolves an embedding provider for a given model name
    pub fn get_provider(&self, model_name: &str) -> Result<Arc<dyn EmbeddingProvider>> {
        {
            let guard = self.providers.read().unwrap();
            if let Some(p) = guard.get(model_name) {
                return Ok(p.clone());
            }
        }

        let config = self.registry.get_model(model_name)?
            .ok_or_else(|| anyhow::anyhow!("Embedding model '{}' not found in registry", model_name))?;

        let provider: Arc<dyn EmbeddingProvider> = match config.provider_type.to_lowercase().as_str() {
            "local" => Arc::new(LocalSemanticProvider::new(
                Some(&config.model_name),
                Some(config.dimension),
                Some(&config.version),
            )),
            "ollama" => Arc::new(OllamaProvider::new(
                &config.model_name,
                config.endpoint.as_deref(),
                config.dimension,
            )),
            "openai" => Arc::new(OpenAIProvider::new(
                &config.model_name,
                config.secret_env_var.as_deref().unwrap_or("OPENAI_API_KEY"),
                config.dimension,
            )),
            other => return Err(anyhow::anyhow!("Unsupported provider type: {}", other)),
        };

        let mut guard = self.providers.write().unwrap();
        guard.insert(model_name.to_string(), provider.clone());
        Ok(provider)
    }

    /// Generates embedding vector with SHA-256 caching
    pub fn embed_text(&self, model_name: &str, text: &str) -> Result<Vec<f32>> {
        let provider = self.get_provider(model_name)?;
        let name = provider.model_name();
        let version = provider.model_version();

        // 1. Check cache
        if let Ok(Some(cached)) = self.cache.get(name, version, text) {
            return Ok(cached);
        }

        // 2. Generate embedding
        let vector = provider.embed(text)?;

        // 3. Populate cache
        let _ = self.cache.put(name, version, text, &vector);

        Ok(vector)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_manager_local() {
        let storage = StorageEngine::new_temp().unwrap();
        let manager = EmbeddingManager::new(storage);

        let v1 = manager.embed_text("default_embed", "heart surgery cardiology").unwrap();
        assert_eq!(v1.len(), 384);

        // Verify cache hit on second run
        let v2 = manager.embed_text("default_embed", "heart surgery cardiology").unwrap();
        assert_eq!(v1, v2);

        let (hits, _) = manager.cache().stats();
        assert_eq!(hits, 1);
    }
}
