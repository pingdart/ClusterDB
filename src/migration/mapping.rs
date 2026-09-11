use crate::embedding::EmbeddingManager;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SchemaPolicy {
    #[default]
    Strict,
    AddMissingColumns,
    IgnoreUnknownColumns,
}

impl SchemaPolicy {
    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "ADD_MISSING_COLUMNS" | "ADD_COLUMNS" | "ADDMISSINGCOLUMNS" => SchemaPolicy::AddMissingColumns,
            "IGNORE_UNKNOWN_COLUMNS" | "IGNORE_UNKNOWN" | "IGNORE" => SchemaPolicy::IgnoreUnknownColumns,
            _ => SchemaPolicy::Strict,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    pub source_column: String,
    pub vector_column: String,
    pub dimensions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MigrationMapping {
    #[serde(default)]
    pub column_renames: HashMap<String, String>,
    #[serde(default)]
    pub type_coercions: HashMap<String, String>,
    #[serde(default)]
    pub primary_key: Option<String>,
    #[serde(default)]
    pub default_values: HashMap<String, String>,
    #[serde(default)]
    pub embedding_configs: Vec<EmbeddingConfig>,
    #[serde(default)]
    pub schema_policy: SchemaPolicy,
}

impl MigrationMapping {
    pub fn new() -> Self {
        Self::default()
    }

    /// Transforms a raw key-value record according to mappings, type coercions, and embedding rules.
    pub fn transform_record(
        &self,
        raw: &HashMap<String, serde_json::Value>,
        embedding_mgr: Option<&EmbeddingManager>,
        known_columns: Option<&[String]>,
    ) -> Result<HashMap<String, String>> {
        let mut out = HashMap::new();

        // 1. Process default values first
        for (k, v) in &self.default_values {
            out.insert(k.clone(), v.clone());
        }

        // 2. Process raw fields
        for (raw_k, raw_v) in raw {
            let target_key = self.column_renames.get(raw_k).cloned().unwrap_or_else(|| raw_k.clone());

            // Handle schema policy if known columns are provided
            if let Some(known) = known_columns {
                if !known.contains(&target_key) && !out.contains_key(&target_key) {
                    match self.schema_policy {
                        SchemaPolicy::Strict => {
                            return Err(anyhow!("Schema policy Strict: unknown column '{}' found in source", raw_k));
                        }
                        SchemaPolicy::IgnoreUnknownColumns => {
                            continue;
                        }
                        SchemaPolicy::AddMissingColumns => {
                            // Allowed, will be handled by target schema updater
                        }
                    }
                }
            }

            let string_val = match raw_v {
                serde_json::Value::Null => "".to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Array(_) | serde_json::Value::Object(_) => raw_v.to_string(),
            };

            out.insert(target_key, string_val);
        }

        // 3. Generate AI vector embeddings if configured
        for emb in &self.embedding_configs {
            if let Some(source_val) = out.get(&emb.source_column) {
                if !source_val.is_empty() {
                    use crate::embedding::provider::{EmbeddingProvider, LocalSemanticProvider};
                    let vec_res = if let Some(mgr) = embedding_mgr {
                        mgr.embed_text("local-minilm", source_val)
                            .unwrap_or_else(|_| {
                                LocalSemanticProvider::new(None, Some(emb.dimensions), None)
                                    .embed(source_val)
                                    .unwrap_or_default()
                            })
                    } else {
                        LocalSemanticProvider::new(None, Some(emb.dimensions), None)
                            .embed(source_val)
                            .unwrap_or_default()
                    };

                    let vec_str = format!(
                        "[{}]",
                        vec_res
                            .iter()
                            .take(emb.dimensions)
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                    out.insert(emb.vector_column.clone(), vec_str);
                }
            }
        }

        Ok(out)
    }
}
