use crate::storage::StorageEngine;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub provider_type: String, // "local" | "ollama" | "openai"
    pub model_name: String,
    pub version: String,
    pub dimension: usize,
    pub endpoint: Option<String>,
    pub secret_env_var: Option<String>,
}

#[derive(Clone)]
pub struct ModelRegistry {
    storage: StorageEngine,
}

impl ModelRegistry {
    pub fn new(storage: StorageEngine) -> Self {
        let registry = Self { storage };
        // Ensure default local model exists
        if let Ok(None) = registry.get_model("default_embed") {
            let default_model = ModelConfig {
                name: "default_embed".to_string(),
                provider_type: "local".to_string(),
                model_name: "local-minilm".to_string(),
                version: "v1".to_string(),
                dimension: 384,
                endpoint: None,
                secret_env_var: None,
            };
            let _ = registry.register_model(default_model);
        }
        registry
    }

    pub fn register_model(&self, config: ModelConfig) -> Result<()> {
        self.storage.set("_embedding_models", &config.name, &config)?;
        Ok(())
    }

    pub fn get_model(&self, name: &str) -> Result<Option<ModelConfig>> {
        self.storage.get("_embedding_models", name)
    }

    pub fn list_models(&self) -> Result<Vec<ModelConfig>> {
        self.storage.get_all("_embedding_models")
    }

    pub fn drop_model(&self, name: &str) -> Result<bool> {
        self.storage.delete("_embedding_models", name)?;
        Ok(true)
    }
}
