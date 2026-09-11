use crate::memory::store::{MemoryItem, MemoryType};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationReport {
    pub session_id: Option<String>,
    pub original_items_count: usize,
    pub consolidated_items_created: usize,
    pub pruned_items_count: usize,
    pub summary_preview: String,
}

pub struct ConsolidationEngine {
    pub threshold: usize, // e.g. 20 items before consolidation triggers
}

impl ConsolidationEngine {
    pub fn new(threshold: usize) -> Self {
        Self { threshold }
    }

    /// Consolidates a slice of episodic/working items into higher-level long-term/semantic summaries
    pub fn consolidate(
        &self,
        session_id: Option<String>,
        items: &mut Vec<MemoryItem>,
    ) -> Option<ConsolidationReport> {
        let episodic_or_working: Vec<&MemoryItem> = items
            .iter()
            .filter(|m| m.memory_type == MemoryType::Episodic || m.memory_type == MemoryType::Working)
            .collect();

        if episodic_or_working.len() < self.threshold {
            return None;
        }

        let original_count = episodic_or_working.len();

        // 1. Extract key themes by taking high importance items and clustering content
        let mut key_contents: Vec<String> = episodic_or_working
            .iter()
            .filter(|m| m.importance >= 0.4 || m.access_count > 1)
            .map(|m| m.content.clone())
            .collect();

        if key_contents.is_empty() {
            // Take the latest sample if none had high importance
            key_contents = episodic_or_working.iter().take(5).map(|m| m.content.clone()).collect();
        }

        let summary_text = format!(
            "Consolidated Knowledge Summary [{} events]: {}",
            original_count,
            key_contents.join(" | ")
        );

        // 2. Create the consolidated LongTerm memory item
        let consolidated_id = Uuid::new_v4().to_string();
        let consolidated_memory = MemoryItem::new(
            consolidated_id,
            session_id.clone(),
            MemoryType::LongTerm,
            summary_text.clone(),
            0.85, // Consolidated knowledge has high long-term importance
            None,
            serde_json::json!({
                "source": "consolidation_engine",
                "events_consolidated": original_count,
            }),
        );

        // 3. Prune low-importance working/episodic items to prevent memory explosion
        let mut pruned = 0usize;
        items.retain(|m| {
            let should_prune = (m.memory_type == MemoryType::Working || m.memory_type == MemoryType::Episodic)
                && m.importance < 0.7
                && m.access_count <= 2;
            if should_prune {
                pruned += 1;
                false
            } else {
                true
            }
        });

        items.push(consolidated_memory);

        Some(ConsolidationReport {
            session_id,
            original_items_count: original_count,
            consolidated_items_created: 1,
            pruned_items_count: pruned,
            summary_preview: summary_text,
        })
    }
}

impl Default for ConsolidationEngine {
    fn default() -> Self {
        Self::new(20)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_consolidation() {
        let engine = ConsolidationEngine::new(5); // threshold 5
        let mut items = Vec::new();

        for i in 1..=6 {
            items.push(MemoryItem::new(
                format!("mem_{}", i),
                Some("session_123".to_string()),
                MemoryType::Working,
                format!("User discussed project milestone {}", i),
                0.3,
                None,
                serde_json::json!({}),
            ));
        }

        let report = engine.consolidate(Some("session_123".to_string()), &mut items);
        assert!(report.is_some());
        let rep = report.unwrap();
        assert_eq!(rep.original_items_count, 6);
        assert_eq!(rep.consolidated_items_created, 1);
        assert!(rep.pruned_items_count > 0);
        assert!(items.iter().any(|m| m.memory_type == MemoryType::LongTerm));
    }
}
