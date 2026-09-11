use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionStrategy {
    /// Use B-Tree index first to filter rows, then calculate vector distances on the subset
    BTreePreFilter {
        index_name: String,
        column: String,
        estimated_candidates: usize,
    },
    /// Use HNSW vector index first for Top-K candidates, then filter by relational predicates
    HnswPreFilter {
        vector_column: String,
        top_k: usize,
        threshold: Option<f32>,
    },
    /// Combine BM25 Full-Text and HNSW Vector indexes with rank fusion
    HybridSearch {
        vector_column: String,
        text_column: String,
        alpha: f32,
    },
    /// Fallback sequential full scan
    SequentialScan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedPlan {
    pub strategy: ExecutionStrategy,
    pub explanation: String,
    pub estimated_cost_ms: f64,
}

pub struct UnifiedPlanner;

impl UnifiedPlanner {
    /// Decides the optimal execution plan for queries combining relational and vector filters
    pub fn plan(
        has_btree_index: bool,
        btree_column: Option<&str>,
        btree_index_name: Option<&str>,
        has_hnsw_index: bool,
        vector_column: Option<&str>,
        has_text_query: bool,
        text_column: Option<&str>,
        estimated_row_count: usize,
        requested_top_k: usize,
        similarity_threshold: Option<f32>,
    ) -> UnifiedPlan {
        // Case 1: Hybrid Search (both text and vector conditions)
        if has_text_query && has_hnsw_index && vector_column.is_some() && text_column.is_some() {
            return UnifiedPlan {
                strategy: ExecutionStrategy::HybridSearch {
                    vector_column: vector_column.unwrap().to_string(),
                    text_column: text_column.unwrap().to_string(),
                    alpha: 0.5,
                },
                explanation: format!(
                    "Plan: Parallel HNSW ({}) and BM25 ({}) execution with rank fusion",
                    vector_column.unwrap(),
                    text_column.unwrap()
                ),
                estimated_cost_ms: 1.5,
            };
        }

        // Case 2: Both B-Tree and HNSW exist
        if has_btree_index && has_hnsw_index && vector_column.is_some() {
            // Cost heuristic:
            // If table has many rows (> 1,000) and top_k is small (<= 50), HNSW is O(log N) -> typically < 1ms
            // If B-tree condition is high-selectivity point lookup (est. candidates <= 10), B-tree pre-filter is faster.
            if requested_top_k <= 50 && estimated_row_count > 500 {
                return UnifiedPlan {
                    strategy: ExecutionStrategy::HnswPreFilter {
                        vector_column: vector_column.unwrap().to_string(),
                        top_k: requested_top_k * 3, // Oversample for post-filtering
                        threshold: similarity_threshold,
                    },
                    explanation: format!(
                        "Plan: HNSW index scan on '{}' (Top-{} candidate oversampling) -> Post-filter by relational predicate",
                        vector_column.unwrap(),
                        requested_top_k * 3
                    ),
                    estimated_cost_ms: 0.8,
                };
            } else {
                return UnifiedPlan {
                    strategy: ExecutionStrategy::BTreePreFilter {
                        index_name: btree_index_name.unwrap_or("btree_idx").to_string(),
                        column: btree_column.unwrap_or("").to_string(),
                        estimated_candidates: estimated_row_count / 10,
                    },
                    explanation: format!(
                        "Plan: B-Tree index scan on '{}' -> Vector similarity calculation on filtered candidate subset",
                        btree_column.unwrap_or("")
                    ),
                    estimated_cost_ms: 1.2,
                };
            }
        }

        // Case 3: Only HNSW vector index matches
        if has_hnsw_index && vector_column.is_some() {
            return UnifiedPlan {
                strategy: ExecutionStrategy::HnswPreFilter {
                    vector_column: vector_column.unwrap().to_string(),
                    top_k: requested_top_k,
                    threshold: similarity_threshold,
                },
                explanation: format!("Plan: Direct HNSW vector index search on '{}'", vector_column.unwrap()),
                estimated_cost_ms: 0.6,
            };
        }

        // Case 4: Only B-Tree index matches
        if has_btree_index && btree_column.is_some() {
            return UnifiedPlan {
                strategy: ExecutionStrategy::BTreePreFilter {
                    index_name: btree_index_name.unwrap_or("btree_idx").to_string(),
                    column: btree_column.unwrap().to_string(),
                    estimated_candidates: estimated_row_count / 10,
                },
                explanation: format!("Plan: B-Tree index scan on '{}'", btree_column.unwrap()),
                estimated_cost_ms: 0.9,
            };
        }

        // Case 5: Sequential fallback
        UnifiedPlan {
            strategy: ExecutionStrategy::SequentialScan,
            explanation: "Plan: Full collection scan (no matching index found)".to_string(),
            estimated_cost_ms: (estimated_row_count as f64) * 0.01 + 5.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unified_planner_strategies() {
        // Test B-Tree + HNSW with high row count and low top_k (favors HNSW pre-filter)
        let plan1 = UnifiedPlanner::plan(
            true, Some("city"), Some("idx_users_city"),
            true, Some("embedding"),
            false, None,
            10000, 10, Some(0.8)
        );
        assert!(matches!(plan1.strategy, ExecutionStrategy::HnswPreFilter { .. }));

        // Test Hybrid Search
        let plan2 = UnifiedPlanner::plan(
            false, None, None,
            true, Some("embedding"),
            true, Some("bio"),
            5000, 10, None
        );
        assert!(matches!(plan2.strategy, ExecutionStrategy::HybridSearch { .. }));

        // Test BTree only
        let plan3 = UnifiedPlanner::plan(
            true, Some("email"), Some("idx_users_email"),
            false, None,
            false, None,
            5000, 10, None
        );
        assert!(matches!(plan3.strategy, ExecutionStrategy::BTreePreFilter { .. }));
    }
}
