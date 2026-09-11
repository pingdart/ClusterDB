use crate::client::PingDartClient;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    pub id: Option<String>,
    pub memory_type: Option<String>,
    pub content: String,
    pub session_id: Option<String>,
    pub importance: Option<f32>,
    pub created_at: Option<u64>,
}

#[derive(Clone)]
pub struct AgentMemory {
    client: PingDartClient,
}

impl AgentMemory {
    pub fn new(client: PingDartClient) -> Self {
        Self { client }
    }

    pub async fn remember_working(&self, session_id: &str, thought: &str) -> Result<()> {
        self.client
            .store_memory("Working", thought, Some(session_id), None)
            .await
    }

    pub async fn remember_episodic(&self, session_id: &str, event: &str) -> Result<()> {
        self.client
            .store_memory("Episodic", event, Some(session_id), None)
            .await
    }

    pub async fn remember_semantic(&self, fact: &str) -> Result<()> {
        self.client.store_memory("Semantic", fact, None, None).await
    }

    pub async fn remember_long_term(
        &self,
        session_id: Option<&str>,
        permanent_rule: &str,
        metadata: Option<serde_json::Value>,
    ) -> Result<()> {
        self.client
            .store_memory("LongTerm", permanent_rule, session_id, metadata)
            .await
    }

    pub async fn recall(&self, session_id: Option<&str>, memory_type: Option<&str>, top_k: usize) -> Result<String> {
        self.client.recall_memory(session_id, memory_type, top_k).await
    }
}
