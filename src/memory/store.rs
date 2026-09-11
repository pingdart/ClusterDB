use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MemoryType {
    /// Past chronological events, conversation turns, task outputs
    Episodic,
    /// Extracted factual knowledge, user preferences, entity relations
    Semantic,
    /// Current conversation buffer / immediate sliding window
    Working,
    /// Consolidated long-term core knowledge
    LongTerm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    pub id: String,
    pub session_id: Option<String>,
    pub memory_type: MemoryType,
    pub content: String,
    pub metadata: String, // JSON-serialized string
    pub embedding: Option<Vec<f32>>,
    pub importance: f32, // 0.0 to 1.0
    pub created_at: i64,  // Unix timestamp (seconds)
    pub last_accessed_at: i64,
    pub access_count: u32,
}

impl MemoryItem {
    pub fn new(
        id: String,
        session_id: Option<String>,
        memory_type: MemoryType,
        content: String,
        importance: f32,
        embedding: Option<Vec<f32>>,
        metadata: serde_json::Value,
    ) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id,
            session_id,
            memory_type,
            content,
            metadata: metadata.to_string(),
            embedding,
            importance: importance.clamp(0.0, 1.0),
            created_at: now,
            last_accessed_at: now,
            access_count: 1,
        }
    }

    pub fn metadata_json(&self) -> serde_json::Value {
        serde_json::from_str(&self.metadata).unwrap_or(serde_json::json!({}))
    }

    pub fn mark_accessed(&mut self) {
        self.last_accessed_at = chrono::Utc::now().timestamp();
        self.access_count += 1;
    }
}
