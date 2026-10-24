use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JoinAlgorithm {
    HashJoin,
    MergeJoin,
    NestedLoop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquiJoinCondition {
    pub left_column: String,
    pub right_column: String,
}

pub struct HashJoinExecutor;

impl HashJoinExecutor {
    /// Attempts to extract an equi-join column pair from a binary expression string or AST.
    /// E.g. "orders.user_id = users.id" or "t1.k = t2.k"
    pub fn try_parse_equi_condition(condition_str: &str) -> Option<EquiJoinCondition> {
        let parts: Vec<&str> = condition_str.split('=').map(|s| s.trim()).collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            Some(EquiJoinCondition {
                left_column: parts[0].to_string(),
                right_column: parts[1].to_string(),
            })
        } else {
            None
        }
    }

    /// Executes an in-memory Hash Join between left and right relations on the specified keys.
    /// Returns rows formed by merging left and right column maps.
    pub fn execute(
        left_rows: &[HashMap<String, String>],
        right_rows: &[HashMap<String, String>],
        left_key: &str,
        right_key: &str,
    ) -> Vec<HashMap<String, String>> {
        let mut results = Vec::new();

        // Build phase: index the smaller relation into a hash multimap
        let (build_rows, probe_rows, build_key, probe_key, is_left_build) =
            if right_rows.len() <= left_rows.len() {
                (right_rows, left_rows, right_key, left_key, false)
            } else {
                (left_rows, right_rows, left_key, right_key, true)
            };

        let mut hash_table: HashMap<String, Vec<&HashMap<String, String>>> = HashMap::new();
        for row in build_rows {
            if let Some(val) = Self::extract_val(row, build_key) {
                hash_table.entry(val).or_default().push(row);
            }
        }

        // Probe phase: stream probe rows and lookup in O(1)
        for p_row in probe_rows {
            if let Some(val) = Self::extract_val(p_row, probe_key) {
                if let Some(matches) = hash_table.get(&val) {
                    for b_row in matches {
                        let mut joined = HashMap::new();
                        let (l, r) = if is_left_build { (*b_row, p_row) } else { (p_row, *b_row) };

                        for (k, v) in l {
                            joined.insert(k.clone(), v.clone());
                        }
                        for (k, v) in r {
                            joined.insert(k.clone(), v.clone());
                        }
                        results.push(joined);
                    }
                }
            }
        }

        results
    }

    fn extract_val(row: &HashMap<String, String>, key: &str) -> Option<String> {
        if let Some(v) = row.get(key) {
            return Some(v.clone());
        }
        // Also check unqualified suffix (e.g. key is "id" and column is "users.id")
        for (col, val) in row {
            if col.ends_with(&format!(".{}", key)) || key.ends_with(&format!(".{}", col)) {
                return Some(val.clone());
            }
        }
        None
    }
}

pub struct MergeJoinExecutor;

impl MergeJoinExecutor {
    /// Executes a Sort-Merge Join on two relations for equi-join keys
    pub fn execute(
        mut left_rows: Vec<HashMap<String, String>>,
        mut right_rows: Vec<HashMap<String, String>>,
        left_key: &str,
        right_key: &str,
    ) -> Vec<HashMap<String, String>> {
        // Sort both by their respective join keys
        left_rows.sort_by(|a, b| {
            let va = HashJoinExecutor::extract_val(a, left_key).unwrap_or_default();
            let vb = HashJoinExecutor::extract_val(b, left_key).unwrap_or_default();
            va.cmp(&vb)
        });

        right_rows.sort_by(|a, b| {
            let va = HashJoinExecutor::extract_val(a, right_key).unwrap_or_default();
            let vb = HashJoinExecutor::extract_val(b, right_key).unwrap_or_default();
            va.cmp(&vb)
        });

        let mut results = Vec::new();
        let mut i = 0;
        let mut j = 0;

        while i < left_rows.len() && j < right_rows.len() {
            let val_l = HashJoinExecutor::extract_val(&left_rows[i], left_key).unwrap_or_default();
            let val_r = HashJoinExecutor::extract_val(&right_rows[j], right_key).unwrap_or_default();

            if val_l < val_r {
                i += 1;
            } else if val_l > val_r {
                j += 1;
            } else {
                // Match found: collect duplicate run on right side
                let mut match_j = j;
                while match_j < right_rows.len() {
                    let vr = HashJoinExecutor::extract_val(&right_rows[match_j], right_key).unwrap_or_default();
                    if vr != val_l {
                        break;
                    }
                    let mut joined = left_rows[i].clone();
                    for (k, v) in &right_rows[match_j] {
                        joined.insert(k.clone(), v.clone());
                    }
                    results.push(joined);
                    match_j += 1;
                }
                i += 1;
            }
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_join_execution() {
        let mut left = Vec::new();
        for i in 1..=5 {
            let mut row = HashMap::new();
            row.insert("id".to_string(), i.to_string());
            row.insert("name".to_string(), format!("User_{}", i));
            left.push(row);
        }

        let mut right = Vec::new();
        for i in [2, 4, 6] {
            let mut row = HashMap::new();
            row.insert("user_id".to_string(), i.to_string());
            row.insert("amount".to_string(), format!("{}", i * 100));
            right.push(row);
        }

        let joined = HashJoinExecutor::execute(&left, &right, "id", "user_id");
        assert_eq!(joined.len(), 2); // Users 2 and 4 match

        let user2 = joined.iter().find(|r| r.get("id").map(|s| s.as_str()) == Some("2")).unwrap();
        assert_eq!(user2.get("amount").map(|s| s.as_str()), Some("200"));
    }

    #[test]
    fn test_merge_join_execution() {
        let mut left = Vec::new();
        for i in [3, 1, 2] {
            let mut row = HashMap::new();
            row.insert("dept_id".to_string(), i.to_string());
            row.insert("dept_name".to_string(), format!("Dept_{}", i));
            left.push(row);
        }

        let mut right = Vec::new();
        for i in [2, 1, 5] {
            let mut row = HashMap::new();
            row.insert("d_id".to_string(), i.to_string());
            row.insert("budget".to_string(), format!("{}", i * 5000));
            right.push(row);
        }

        let joined = MergeJoinExecutor::execute(left, right, "dept_id", "d_id");
        assert_eq!(joined.len(), 2); // Depts 1 and 2 match
    }
}
