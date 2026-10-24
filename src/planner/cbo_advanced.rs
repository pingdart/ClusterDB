use crate::planner::stats::TableStats;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhysicalAccessPath {
    SequentialScan { table: String, estimated_rows: usize },
    IndexScan { table: String, index_name: String, column: String, estimated_rows: usize },
    IndexOnlyScan { table: String, index_name: String, projected_cols: Vec<String>, estimated_rows: usize },
    PartitionPrunedScan { table: String, active_partitions: Vec<String>, total_partitions: usize },
    MaterializedViewScan { view_name: String, cached_rows: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhysicalJoinAlgorithm {
    HashJoin { build_side: String, probe_side: String },
    SortMergeJoin { left_side: String, right_side: String },
    NestedLoopJoin { outer_side: String, inner_side: String },
    DistributedBroadcastJoin { broadcast_table: String, distributed_table: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalQueryPlan {
    pub access_path: PhysicalAccessPath,
    pub join_algorithm: Option<PhysicalJoinAlgorithm>,
    pub use_parallel_workers: bool,
    pub worker_count: usize,
    pub estimated_startup_cost: f64,
    pub estimated_total_cost: f64,
    pub estimated_output_rows: usize,
    pub explain_text: String,
}

pub struct AdvancedCostBasedOptimizer {
    cost_page_io: f64,
    cost_cpu_tuple: f64,
    cost_cpu_index_tuple: f64,
    cost_cpu_operator: f64,
    cost_parallel_worker_startup: f64,
}

impl AdvancedCostBasedOptimizer {
    pub fn new() -> Self {
        Self {
            cost_page_io: 1.0,               // Base unit: 1 4KB page random fetch = 1.0 cost
            cost_cpu_tuple: 0.01,            // CPU tuple processing = 0.01
            cost_cpu_index_tuple: 0.005,     // Index leaf traversal per key = 0.005
            cost_cpu_operator: 0.0025,       // Operator / predicate evaluation = 0.0025
            cost_parallel_worker_startup: 2.5, // Worker thread spawn overhead
        }
    }

    /// Enumerates and chooses the optimal physical execution plan
    pub fn optimize_query(
        &self,
        table_name: &str,
        table_stats: Option<&TableStats>,
        has_matching_mat_view: Option<&str>,
        has_partitioning: Option<(Vec<String>, usize)>, // (active_partitions, total_partitions)
        has_index: Option<(&str, &str, bool)>, // (index_name, column, is_index_only)
        has_join: Option<(&str, &str, &str)>,  // (other_table, join_key, join_type)
        selectivity: f64,
        allow_parallel: bool,
    ) -> PhysicalQueryPlan {
        let total_rows = table_stats.map(|s| s.total_rows).unwrap_or(10_000).max(1);
        let pages = ((total_rows as f64 * 128.0) / 4096.0).ceil().max(1.0); // Assume 128 bytes/tuple

        // 1. Check Materialized View Rewrite (lowest possible cost)
        if let Some(view_name) = has_matching_mat_view {
            let cached_rows = (total_rows as f64 * 0.1).ceil() as usize;
            return PhysicalQueryPlan {
                access_path: PhysicalAccessPath::MaterializedViewScan {
                    view_name: view_name.to_string(),
                    cached_rows,
                },
                join_algorithm: None,
                use_parallel_workers: false,
                worker_count: 0,
                estimated_startup_cost: 0.0,
                estimated_total_cost: 0.05,
                estimated_output_rows: cached_rows,
                explain_text: format!("Materialized View Scan on '{}' (Cost: 0.05..0.05, Rows: {})", view_name, cached_rows),
            };
        }

        // 2. Candidate Access Paths
        let seq_cost = (pages * self.cost_page_io) + (total_rows as f64 * self.cost_cpu_tuple);
        let est_rows = ((total_rows as f64) * selectivity).round().max(1.0) as usize;

        let mut best_path = PhysicalAccessPath::SequentialScan {
            table: table_name.to_string(),
            estimated_rows: est_rows,
        };
        let mut min_access_cost = seq_cost;
        let mut explain = format!("Seq Scan on {} (Cost: 0.00..{:.2}, Rows: {})", table_name, seq_cost, est_rows);

        // Check Partition Pruning
        if let Some((active, total)) = has_partitioning {
            if active.len() < total {
                let ratio = active.len() as f64 / total.max(1) as f64;
                let pruned_cost = seq_cost * ratio;
                let pruned_rows = ((est_rows as f64) * ratio).round().max(1.0) as usize;
                if pruned_cost < min_access_cost {
                    min_access_cost = pruned_cost;
                    best_path = PhysicalAccessPath::PartitionPrunedScan {
                        table: table_name.to_string(),
                        active_partitions: active.clone(),
                        total_partitions: total,
                    };
                    explain = format!(
                        "Partition Pruned Scan on {} [{}/{} partitions active] (Cost: 0.00..{:.2}, Rows: {})",
                        table_name, active.len(), total, pruned_cost, pruned_rows
                    );
                }
            }
        }

        // Check B+Tree Index Scan vs Index-Only Scan
        if let Some((idx_name, col_name, is_index_only)) = has_index {
            let index_pages = (pages * 0.2).ceil().max(1.0);
            let btree_tree_traversal = 3.0 * self.cost_cpu_index_tuple;
            let index_read_cost = (index_pages * self.cost_page_io * selectivity) + ((est_rows as f64) * self.cost_cpu_index_tuple);

            if is_index_only {
                let index_only_cost = btree_tree_traversal + index_read_cost;
                if index_only_cost < min_access_cost {
                    min_access_cost = index_only_cost;
                    best_path = PhysicalAccessPath::IndexOnlyScan {
                        table: table_name.to_string(),
                        index_name: idx_name.to_string(),
                        projected_cols: vec![col_name.to_string()],
                        estimated_rows: est_rows,
                    };
                    explain = format!(
                        "Index Only Scan using {} on {} (Cost: {:.2}..{:.2}, Rows: {})",
                        idx_name, table_name, btree_tree_traversal, index_only_cost, est_rows
                    );
                }
            } else {
                let table_fetch_cost = (est_rows as f64) * self.cost_page_io * 0.5; // Random I/O
                let index_scan_cost = btree_tree_traversal + index_read_cost + table_fetch_cost;
                if index_scan_cost < min_access_cost {
                    min_access_cost = index_scan_cost;
                    best_path = PhysicalAccessPath::IndexScan {
                        table: table_name.to_string(),
                        index_name: idx_name.to_string(),
                        column: col_name.to_string(),
                        estimated_rows: est_rows,
                    };
                    explain = format!(
                        "Index Scan using {} on {} (Cost: {:.2}..{:.2}, Rows: {})",
                        idx_name, table_name, btree_tree_traversal, index_scan_cost, est_rows
                    );
                }
            }
        }

        // 3. Candidate Join Algorithm
        let mut chosen_join = None;
        let mut total_query_cost = min_access_cost;

        if let Some((other_t, _join_key, _)) = has_join {
            let other_rows = 5000;
            // Cost of Hash Join: O(N + M)
            let hash_join_cost = (est_rows as f64 + other_rows as f64) * self.cost_cpu_operator;
            // Cost of Nested Loop: O(N * M)
            let nested_loop_cost = (est_rows as f64 * other_rows as f64) * self.cost_cpu_operator;

            if hash_join_cost < nested_loop_cost {
                chosen_join = Some(PhysicalJoinAlgorithm::HashJoin {
                    build_side: other_t.to_string(),
                    probe_side: table_name.to_string(),
                });
                total_query_cost += hash_join_cost;
                explain = format!("Hash Join (Build: {}, Probe: {}) -> {}", other_t, table_name, explain);
            } else {
                chosen_join = Some(PhysicalJoinAlgorithm::NestedLoopJoin {
                    outer_side: table_name.to_string(),
                    inner_side: other_t.to_string(),
                });
                total_query_cost += nested_loop_cost;
                explain = format!("Nested Loop (Outer: {}, Inner: {}) -> {}", table_name, other_t, explain);
            }
        }

        // 4. Parallel Workers Evaluation
        let mut use_parallel = false;
        let mut workers = 0;
        if allow_parallel && total_rows >= 5000 && matches!(best_path, PhysicalAccessPath::SequentialScan { .. } | PhysicalAccessPath::PartitionPrunedScan { .. }) {
            workers = 4;
            let parallel_cost = (min_access_cost / 3.2) + self.cost_parallel_worker_startup;
            if parallel_cost < min_access_cost {
                use_parallel = true;
                total_query_cost = parallel_cost;
                explain = format!("Parallel Workers: {} -> {}", workers, explain);
            }
        }

        PhysicalQueryPlan {
            access_path: best_path,
            join_algorithm: chosen_join,
            use_parallel_workers: use_parallel,
            worker_count: workers,
            estimated_startup_cost: 0.0,
            estimated_total_cost: total_query_cost,
            estimated_output_rows: est_rows,
            explain_text: explain,
        }
    }
}

impl Default for AdvancedCostBasedOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cbo_selects_index_scan_on_high_selectivity() {
        let optimizer = AdvancedCostBasedOptimizer::new();

        // 1% selectivity with existing B+Tree index -> Index Scan must be chosen
        let plan = optimizer.optimize_query(
            "accounts",
            None, // Default 10,000 rows
            None,
            None,
            Some(("idx_accounts_acc_id", "acc_id", false)),
            None,
            0.01, // 1%
            false,
        );

        assert!(matches!(plan.access_path, PhysicalAccessPath::IndexScan { .. }));
        assert!(plan.explain_text.contains("Index Scan"));
    }

    #[test]
    fn test_cbo_selects_index_only_scan_when_applicable() {
        let optimizer = AdvancedCostBasedOptimizer::new();

        let plan = optimizer.optimize_query(
            "users",
            None,
            None,
            None,
            Some(("idx_users_email", "email", true)),
            None,
            0.05,
            false,
        );

        assert!(matches!(plan.access_path, PhysicalAccessPath::IndexOnlyScan { .. }));
        assert!(plan.explain_text.contains("Index Only Scan"));
    }

    #[test]
    fn test_cbo_selects_materialized_view_rewrite() {
        let optimizer = AdvancedCostBasedOptimizer::new();

        let plan = optimizer.optimize_query(
            "orders",
            None,
            Some("mv_monthly_orders"),
            None,
            None,
            None,
            1.0,
            false,
        );

        assert!(matches!(plan.access_path, PhysicalAccessPath::MaterializedViewScan { .. }));
        assert!(plan.explain_text.contains("Materialized View Scan on 'mv_monthly_orders'"));
    }

    #[test]
    fn test_cbo_selects_hash_join_over_nested_loop() {
        let optimizer = AdvancedCostBasedOptimizer::new();

        let plan = optimizer.optimize_query(
            "employees",
            None,
            None,
            None,
            None,
            Some(("departments", "dept_id", "INNER")),
            0.5,
            false,
        );

        assert!(matches!(plan.join_algorithm, Some(PhysicalJoinAlgorithm::HashJoin { .. })));
        assert!(plan.explain_text.contains("Hash Join"));
    }
}
