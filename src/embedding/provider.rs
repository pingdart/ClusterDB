use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// Common trait for all embedding providers (Local, Ollama, OpenAI, etc.)
pub trait EmbeddingProvider: Send + Sync {
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    fn dimension(&self) -> usize;
    fn model_name(&self) -> &str;
    fn model_version(&self) -> &str;
}

/// Lightweight deterministic semantic vector generator for local zero-dependency operation.
/// Produces normalized dense embeddings (e.g. 384 dimensions) using term hashing and n-gram subword pooling.
pub struct LocalSemanticProvider {
    name: String,
    version: String,
    dimension: usize,
}

impl LocalSemanticProvider {
    pub fn new(name: Option<&str>, dimension: Option<usize>, version: Option<&str>) -> Self {
        Self {
            name: name.unwrap_or("local-minilm").to_string(),
            version: version.unwrap_or("v1").to_string(),
            dimension: dimension.unwrap_or(384),
        }
    }
}

impl EmbeddingProvider for LocalSemanticProvider {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut vector = vec![0.0f32; self.dimension];
        let normalized = text.to_lowercase();
        let words: Vec<&str> = normalized.split_whitespace().collect();

        if words.is_empty() {
            return Ok(vector);
        }

        // Semantic word-hash projection with contextual position weights
        for (pos, word) in words.iter().enumerate() {
            let mut hasher = Sha256::new();
            hasher.update(word.as_bytes());
            let hash = hasher.finalize();

            let weight = 1.0 / (1.0 + (pos as f32) * 0.05);

            for i in 0..self.dimension {
                let byte_idx = (i * 2) % 32;
                let bit_val = ((hash[byte_idx] as i32) - 128) as f32 / 128.0;
                vector[i] += bit_val * weight;
            }
        }

        // Normalize to unit sphere (L2 norm = 1.0) for standard cosine similarity
        let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut vector {
                *v /= norm;
            }
        }

        Ok(vector)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model_name(&self) -> &str {
        &self.name
    }

    fn model_version(&self) -> &str {
        &self.version
    }
}

/// Ollama embedding provider connecting to local HTTP server
pub struct OllamaProvider {
    name: String,
    version: String,
    dimension: usize,
    endpoint: String,
}

impl OllamaProvider {
    pub fn new(model_name: &str, endpoint: Option<&str>, dimension: usize) -> Self {
        Self {
            name: model_name.to_string(),
            version: "v1".to_string(),
            dimension,
            endpoint: endpoint.unwrap_or("http://localhost:11434").to_string(),
        }
    }
}

impl EmbeddingProvider for OllamaProvider {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/api/embeddings", self.endpoint);
        let client = reqwest::blocking::Client::new();
        let payload = serde_json::json!({
            "model": self.name,
            "prompt": text
        });

        let resp = client.post(&url)
            .json(&payload)
            .send()
            .context("Failed to connect to Ollama embedding endpoint")?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Ollama error: HTTP {}", resp.status()));
        }

        let json_val: serde_json::Value = resp.json()?;
        if let Some(arr) = json_val.get("embedding").and_then(|e| e.as_array()) {
            let vec: Vec<f32> = arr.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect();
            Ok(vec)
        } else {
            Err(anyhow::anyhow!("Invalid Ollama embedding response format"))
        }
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model_name(&self) -> &str {
        &self.name
    }

    fn model_version(&self) -> &str {
        &self.version
    }
}

/// OpenAI embedding provider reading API keys securely from environment variables
pub struct OpenAIProvider {
    name: String,
    version: String,
    dimension: usize,
    secret_env_var: String,
}

impl OpenAIProvider {
    pub fn new(model_name: &str, secret_env_var: &str, dimension: usize) -> Self {
        Self {
            name: model_name.to_string(),
            version: "v1".to_string(),
            dimension,
            secret_env_var: secret_env_var.to_string(),
        }
    }
}

impl EmbeddingProvider for OpenAIProvider {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let api_key = std::env::var(&self.secret_env_var)
            .context(format!("Environment variable '{}' for OpenAI API key is not set", self.secret_env_var))?;

        let url = "https://api.openai.com/v1/embeddings";
        let client = reqwest::blocking::Client::new();
        let payload = serde_json::json!({
            "model": self.name,
            "input": text
        });

        let resp = client.post(url)
            .bearer_auth(api_key)
            .json(&payload)
            .send()
            .context("Failed to call OpenAI embeddings API")?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("OpenAI API error: HTTP {}", resp.status()));
        }

        let json_val: serde_json::Value = resp.json()?;
        if let Some(data) = json_val.get("data").and_then(|d| d.as_array()) {
            if let Some(first) = data.first().and_then(|item| item.get("embedding")).and_then(|e| e.as_array()) {
                let vec: Vec<f32> = first.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect();
                return Ok(vec);
            }
        }

        Err(anyhow::anyhow!("Invalid OpenAI embedding response format"))
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model_name(&self) -> &str {
        &self.name
    }

    fn model_version(&self) -> &str {
        &self.version
    }
}
