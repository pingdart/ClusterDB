pub mod store;
pub mod decay;
pub mod consolidation;

use crate::storage::StorageEngine;
use anyhow::Result;
pub use consolidation::{ConsolidationEngine, ConsolidationReport};
pub use decay::DecayEngine;
pub use store::{MemoryItem, MemoryType};
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct MemoryEngine {
    storage: StorageEngine,
    decay: Arc<DecayEngine>,
    consolidation: Arc<ConsolidationEngine>,
    /// In-memory cache: session_or_user_id -> Vec<MemoryItem>
    cache: Arc<RwLock<Vec<MemoryItem>>>,
}

impl MemoryEngine {
    pub fn new(storage: StorageEngine) -> Self {
        Self {
            storage,
            decay: Arc::new(DecayEngine::default()),
            consolidation: Arc::new(ConsolidationEngine::default()),
            cache: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Stores a new memory item and persists to Sled
    pub fn store(&self, memory: MemoryItem) -> Result<()> {
        let key = memory.id.clone();
        self.storage.set("_ai_memories", &key, &memory)?;

        let mut cache = self.cache.write().unwrap();
        cache.push(memory);
        Ok(())
    }

    /// Recalls memories matching a query embedding, ranking by similarity, recency decay, and importance
    pub fn recall(
        &self,
        query_embedding: Option<&[f32]>,
        memory_type: Option<MemoryType>,
        session_id: Option<&str>,
        top_k: usize,
    ) -> Result<Vec<(MemoryItem, f32)>> {
        let now = chrono::Utc::now().timestamp();
        let all_memories: Vec<MemoryItem> = self.storage.get_all("_ai_memories")?;

        let mut scored: Vec<(MemoryItem, f32)> = all_memories
            .into_iter()
            .filter(|m| {
                if let Some(ref target_type) = memory_type {
                    if &m.memory_type != target_type {
                        return false;
                    }
                }
                if let Some(s_id) = session_id {
                    if m.session_id.as_deref() != Some(s_id) {
                        return false;
                    }
                }
                true
            })
            .map(|mut m| {
                let score = self.decay.score_memory(&m, query_embedding, now);
                m.mark_accessed();
                let _ = self.storage.set("_ai_memories", &m.id, &m);
                (m, score)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        Ok(scored)
    }

    /// Triggers memory consolidation for a given session
    pub fn consolidate_session(&self, session_id: Option<String>) -> Result<Option<ConsolidationReport>> {
        let all_memories: Vec<MemoryItem> = self.storage.get_all("_ai_memories")?;
        let mut session_items: Vec<MemoryItem> = all_memories
            .into_iter()
            .filter(|m| m.session_id == session_id)
            .collect();

        if let Some(report) = self.consolidation.consolidate(session_id.clone(), &mut session_items) {
            // Re-save session items to Sled
            for item in &session_items {
                self.storage.set("_ai_memories", &item.id, item)?;
            }
            return Ok(Some(report));
        }

        Ok(None)
    }
}
