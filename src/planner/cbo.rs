use crate::planner::stats::TableStats;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanCandidateCost {
    pub plan_name: String,
    pub estimated_cost_ms: f64,
    pub estimated_rows: usize,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CboDecision {
    pub selected_plan: String,
    pub estimated_cost_ms: f64,
    pub estimated_rows: usize,
    pub candidate_costs: Vec<PlanCandidateCost>,
    pub staleness_warning: Option<String>,
}

pub struct CostBasedOptimizer {
    pub w_io: f64,
    pub w_cpu: f64,
    pub w_btree: f64,
    pub w_fetch: f64,
    pub w_vec: f64,
    pub w_agg: f64,
}

impl CostBasedOptimizer {
    pub fn new_calibrated() -> Self {
        Self {
            w_io: 0.001,      // ~1.0 µs per row sequential streaming scan
            w_cpu: 0.0001,    // ~100 ns per row CPU evaluation
            w_btree: 0.0113,  // ~11.3 µs BTree traversal
            w_fetch: 0.0125,  // ~12.5 µs per random record fetch (crossover at ~8.7% selectivity)
            w_vec: 0.0179,    // ~17.9 µs per vector proximity calculation
            w_agg: 0.00945,   // ~9.45 µs per 1k columnar chunk items
        }
    }

    /// Evaluates candidate plans and chooses the optimal physical path
    pub fn plan_query(
        &self,
        table_stats: Option<&TableStats>,
        has_index: bool,
        index_column: Option<&str>,
        has_vector_search: bool,
        has_aggregation: bool,
        selectivity: f64,
    ) -> CboDecision {
        let total_rows = table_stats.map(|s| s.total_rows).unwrap_or(1000).max(1);
        let mut candidates = Vec::new();

        // Check staleness
        let staleness_warning = if let Some(stats) = table_stats {
            let (stale, ratio) = stats.is_stale();
            if stale {
                Some(format!(
                    "Statistics are stale ({:.1}% of table modified since last ANALYZE)",
                    ratio * 100.0
                ))
            } else {
                None
            }
        } else {
            Some("No catalog statistics available; optimizer using default estimates. Run ANALYZE TABLE.".to_string())
        };

        // 1. Sequential Scan candidate
        let seq_cost = (total_rows as f64 * self.w_io) + (total_rows as f64 * self.w_cpu);
        let est_rows = ((total_rows as f64) * selectivity).round() as usize;
        candidates.push(PlanCandidateCost {
            plan_name: "SequentialScan".to_string(),
            estimated_cost_ms: seq_cost,
            estimated_rows: est_rows.max(1),
            description: format!("Full table scan over {} rows", total_rows),
        });

        // 2. BTree Index Scan candidate (if index exists)
        if has_index {
            let fetched_rows = (total_rows as f64 * selectivity).max(1.0);
            let index_cost = self.w_btree + (fetched_rows * self.w_fetch) + (fetched_rows * self.w_cpu);
            candidates.push(PlanCandidateCost {
                plan_name: "BTreeIndexScan".to_string(),
                estimated_cost_ms: index_cost,
                estimated_rows: fetched_rows.round() as usize,
                description: format!(
                    "Index scan on '{}' (selectivity: {:.2}%)",
                    index_column.unwrap_or("primary_key"),
                    selectivity * 100.0
                ),
            });
        }

        // 3. Columnar Aggregation candidate (if analytical aggregate)
        if has_aggregation {
            let chunks = (total_rows as f64 / 65536.0).ceil();
            let col_cost = chunks * self.w_agg;
            candidates.push(PlanCandidateCost {
                plan_name: "ColumnarAggregate".to_string(),
                estimated_cost_ms: col_cost,
                estimated_rows: 1,
                description: "Vectorized SIMD columnar hash aggregation".to_string(),
            });
        }

        // 4. HNSW Vector candidate (if vector search)
        if has_vector_search {
            let ef_search = 100.0;
            let log_n = (total_rows as f64).log2().max(1.0);
            let hnsw_cost = (ef_search * log_n * self.w_vec) + (10.0 * self.w_cpu);
            candidates.push(PlanCandidateCost {
                plan_name: "HnswProximitySearch".to_string(),
                estimated_cost_ms: hnsw_cost,
                estimated_rows: 10,
                description: "HNSW proximity beam graph search".to_string(),
            });
        }

        // Pick lowest cost candidate
        candidates.sort_by(|a, b| a.estimated_cost_ms.partial_cmp(&b.estimated_cost_ms).unwrap());
        let best = candidates[0].clone();

        CboDecision {
            selected_plan: best.plan_name,
            estimated_cost_ms: best.estimated_cost_ms,
            estimated_rows: best.estimated_rows,
            candidate_costs: candidates,
            staleness_warning,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cbo_plan_selection_high_vs_low_selectivity() {
        let optimizer = CostBasedOptimizer::new_calibrated();

        let mut stats = TableStats::new("users");
        stats.total_rows = 10000;
        stats.analyzed_rows = 10000;

        // Case 1: Point lookup selectivity (S = 0.001 on 10,000 rows -> 10 rows)
        let decision_point = optimizer.plan_query(Some(&stats), true, Some("id"), false, false, 0.001);
        assert_eq!(decision_point.selected_plan, "BTreeIndexScan");

        // Case 2: Low selectivity / bulk scan (S = 0.90 on 10,000 rows -> 9,000 rows)
        // Sequential Scan must win because random B-tree page seeks for 9,000 rows are costlier!
        let decision_bulk = optimizer.plan_query(Some(&stats), true, Some("status"), false, false, 0.90);
        assert_eq!(decision_bulk.selected_plan, "SequentialScan");
    }
}
