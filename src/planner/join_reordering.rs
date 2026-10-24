use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableRelationInfo {
    pub table_name: String,
    pub row_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinPredicate {
    pub left_table: String,
    pub right_table: String,
    pub selectivity: f64, // e.g. 0.01 = 1%
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinOrderPlan {
    pub ordered_tables: Vec<String>,
    pub estimated_total_cost: f64,
    pub intermediate_sizes: Vec<usize>,
}

pub struct JoinReorderingOptimizer;

impl JoinReorderingOptimizer {
    /// Determines the optimal left-deep join order using a greedy heuristic
    /// based on intermediate cardinality minimization.
    pub fn find_optimal_order(
        tables: &[TableRelationInfo],
        predicates: &[JoinPredicate],
    ) -> JoinOrderPlan {
        if tables.is_empty() {
            return JoinOrderPlan {
                ordered_tables: Vec::new(),
                estimated_total_cost: 0.0,
                intermediate_sizes: Vec::new(),
            };
        }
        if tables.len() == 1 {
            return JoinOrderPlan {
                ordered_tables: vec![tables[0].table_name.clone()],
                estimated_total_cost: 0.0,
                intermediate_sizes: vec![tables[0].row_count],
            };
        }

        // Predicate lookup helper: (table_a, table_b) -> selectivity
        let mut sel_map: HashMap<(String, String), f64> = HashMap::new();
        for p in predicates {
            sel_map.insert((p.left_table.clone(), p.right_table.clone()), p.selectivity);
            sel_map.insert((p.right_table.clone(), p.left_table.clone()), p.selectivity);
        }

        let mut remaining_tables: Vec<TableRelationInfo> = tables.to_vec();

        // 1. Pick the pair with the smallest estimated intermediate output size
        let mut best_pair = (0, 1);
        let mut min_initial_cost = f64::MAX;

        for i in 0..remaining_tables.len() {
            for j in (i + 1)..remaining_tables.len() {
                let t1 = &remaining_tables[i];
                let t2 = &remaining_tables[j];
                let sel = sel_map.get(&(t1.table_name.clone(), t2.table_name.clone())).copied().unwrap_or(0.1);
                let cost = (t1.row_count as f64) * (t2.row_count as f64) * sel;
                if cost < min_initial_cost {
                    min_initial_cost = cost;
                    best_pair = (i, j);
                }
            }
        }

        let t_first = remaining_tables.remove(best_pair.1.max(best_pair.0));
        let t_second = remaining_tables.remove(best_pair.1.min(best_pair.0));

        let mut ordered_names = vec![t_second.table_name, t_first.table_name];
        let mut current_interm_size = min_initial_cost.round().max(1.0) as usize;
        let mut intermediate_sizes = vec![current_interm_size];
        let mut total_cost = min_initial_cost;

        // 2. Greedily join remaining tables that result in smallest cumulative size
        while !remaining_tables.is_empty() {
            let mut best_next_idx = 0;
            let mut best_next_cost = f64::MAX;
            let mut best_next_size = 0;

            for (idx, candidate) in remaining_tables.iter().enumerate() {
                // Find minimum selectivity between current joined set and candidate
                let mut best_sel: f64 = 0.1; // Default
                for joined_t in &ordered_names {
                    if let Some(&s) = sel_map.get(&(joined_t.clone(), candidate.table_name.clone())) {
                        best_sel = best_sel.min(s);
                    }
                }

                let new_size = (current_interm_size as f64) * (candidate.row_count as f64) * best_sel;
                if new_size < best_next_cost {
                    best_next_cost = new_size;
                    best_next_idx = idx;
                    best_next_size = new_size.round().max(1.0) as usize;
                }
            }

            let next_t = remaining_tables.remove(best_next_idx);
            ordered_names.push(next_t.table_name);
            total_cost += best_next_cost;
            current_interm_size = best_next_size;
            intermediate_sizes.push(current_interm_size);
        }

        JoinOrderPlan {
            ordered_tables: ordered_names,
            estimated_total_cost: total_cost,
            intermediate_sizes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_reordering_minimizes_intermediate_cardinality() {
        // Tables: Orders (1,000,000), Customers (10,000), Nations (25)
        let tables = vec![
            TableRelationInfo { table_name: "orders".into(), row_count: 1_000_000 },
            TableRelationInfo { table_name: "customers".into(), row_count: 10_000 },
            TableRelationInfo { table_name: "nations".into(), row_count: 25 },
        ];

        let predicates = vec![
            JoinPredicate {
                left_table: "orders".into(),
                right_table: "customers".into(),
                selectivity: 0.0001, // 1/10000
            },
            JoinPredicate {
                left_table: "customers".into(),
                right_table: "nations".into(),
                selectivity: 0.04, // 1/25
            },
        ];

        let plan = JoinReorderingOptimizer::find_optimal_order(&tables, &predicates);

        // Nations and Customers should be joined first because their intermediate size is tiny (10,000 * 25 * 0.04 = 10,000)
        // rather than starting with a massive join
        assert!(
            plan.ordered_tables[0] == "nations" || plan.ordered_tables[0] == "customers",
            "Smallest relations should be paired first: {:?}",
            plan.ordered_tables
        );
    }
}
