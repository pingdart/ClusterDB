use crate::memory::store::MemoryItem;
use crate::vector::distance::cosine_similarity;

/// Default decay rate lambda (half-life of ~7 days in seconds: 7 * 86400 = 604800s)
pub const DEFAULT_DECAY_LAMBDA: f64 = 0.00000114; // ln(2) / 604800

pub struct DecayEngine {
    pub lambda: f64,
}

impl DecayEngine {
    pub fn new(lambda: Option<f64>) -> Self {
        Self {
            lambda: lambda.unwrap_or(DEFAULT_DECAY_LAMBDA),
        }
    }

    /// Computes exponential decay factor: e^(-lambda * delta_t)
    pub fn decay_factor(&self, last_accessed_at: i64, current_time: i64) -> f32 {
        let delta_t = (current_time.saturating_sub(last_accessed_at)) as f64;
        let factor = (-self.lambda * delta_t).exp();
        factor as f32
    }

    /// Computes composite retrieval score combining:
    /// 1. Vector Semantic Similarity (w_rel = 0.5)
    /// 2. Memory Recency / Decayed Strength (w_rec = 0.3)
    /// 3. Intrinsic Importance & Access Boost (w_imp = 0.2)
    pub fn score_memory(
        &self,
        memory: &MemoryItem,
        query_embedding: Option<&[f32]>,
        current_time: i64,
    ) -> f32 {
        let recency_factor = self.decay_factor(memory.last_accessed_at, current_time);
        let decayed_importance = memory.importance * recency_factor;

        let similarity = if let (Some(q_emb), Some(m_emb)) = (query_embedding, &memory.embedding) {
            if q_emb.len() == m_emb.len() {
                cosine_similarity(q_emb, m_emb).max(0.0)
            } else {
                0.0
            }
        } else {
            0.5 // Neutral baseline if no embedding is provided
        };

        let frequency_boost = ((memory.access_count as f32).ln_1p() * 0.1).min(0.2);

        let w_rel = 0.5;
        let w_rec = 0.3;
        let w_imp = 0.2;

        (w_rel * similarity) + (w_rec * recency_factor) + (w_imp * (decayed_importance + frequency_boost))
    }
}

impl Default for DecayEngine {
    fn default() -> Self {
        Self::new(None)
    }
}
