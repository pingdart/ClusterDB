use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeBound {
    pub start: Option<String>, // Inclusive, None = -infinity
    pub end: Option<String>,   // Exclusive, None = +infinity
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionDef {
    pub partition_name: String,
    pub range_bound: Option<RangeBound>,
    pub hash_modulus: Option<u32>,
    pub hash_remainder: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartitionStrategy {
    Range {
        column: String,
        partitions: Vec<PartitionDef>,
    },
    Hash {
        column: String,
        modulus: u32,
        partitions: Vec<PartitionDef>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TablePartitionConfig {
    pub table_name: String,
    pub strategy: PartitionStrategy,
}

pub struct PartitionPruner;

impl PartitionPruner {
    /// Determines which physical partition a row belongs to for writes
    pub fn route_row<'a>(
        config: &'a TablePartitionConfig,
        row_columns: &std::collections::HashMap<String, String>,
    ) -> Result<&'a PartitionDef> {
        match &config.strategy {
            PartitionStrategy::Range { column, partitions } => {
                let val = row_columns
                    .get(column)
                    .ok_or_else(|| anyhow::anyhow!("Missing partition column '{}'", column))?;

                for p in partitions {
                    if let Some(bound) = &p.range_bound {
                        let after_start = match &bound.start {
                            Some(s) => val >= s,
                            None => true,
                        };
                        let before_end = match &bound.end {
                            Some(e) => val < e,
                            None => true,
                        };
                        if after_start && before_end {
                            return Ok(p);
                        }
                    }
                }
                bail!("No matching range partition found for value '{}' in column '{}'", val, column);
            }
            PartitionStrategy::Hash { column, modulus, partitions } => {
                let val = row_columns
                    .get(column)
                    .ok_or_else(|| anyhow::anyhow!("Missing partition column '{}'", column))?;

                let mut hasher = DefaultHasher::new();
                val.hash(&mut hasher);
                let hash_val = (hasher.finish() % (*modulus as u64)) as u32;

                for p in partitions {
                    if p.hash_remainder == Some(hash_val) {
                        return Ok(p);
                    }
                }
                bail!("No matching hash partition found for hash {}", hash_val);
            }
        }
    }

    /// Static Partition Pruning: Prunes unneeded partition tables during query planning
    /// based on WHERE clause equality/range constraints.
    pub fn prune_partitions<'a>(
        config: &'a TablePartitionConfig,
        filter_col: &str,
        filter_val: &str,
        op: &str, // "=", ">", "<", ">=", "<="
    ) -> Vec<&'a PartitionDef> {
        match &config.strategy {
            PartitionStrategy::Range { column, partitions } => {
                if column != filter_col {
                    return partitions.iter().collect(); // Cannot prune on unrelated column
                }

                let mut matched = Vec::new();
                for p in partitions {
                    if let Some(bound) = &p.range_bound {
                        let can_overlap = match op {
                            "=" => {
                                let after_start = bound.start.as_ref().map_or(true, |s| filter_val >= s.as_str());
                                let before_end = bound.end.as_ref().map_or(true, |e| filter_val < e.as_str());
                                after_start && before_end
                            }
                            ">" | ">=" => {
                                // Filter asks for values >= filter_val
                                // Overlaps if partition's end is > filter_val or infinity
                                bound.end.as_ref().map_or(true, |e| e.as_str() > filter_val)
                            }
                            "<" | "<=" => {
                                // Filter asks for values < filter_val
                                // Overlaps if partition's start is <= filter_val or -infinity
                                bound.start.as_ref().map_or(true, |s| s.as_str() <= filter_val)
                            }
                            _ => true,
                        };

                        if can_overlap {
                            matched.push(p);
                        }
                    }
                }
                matched
            }
            PartitionStrategy::Hash { column, modulus, partitions } => {
                if column != filter_col || op != "=" {
                    return partitions.iter().collect();
                }

                let mut hasher = DefaultHasher::new();
                filter_val.hash(&mut hasher);
                let hash_val = (hasher.finish() % (*modulus as u64)) as u32;

                partitions
                    .iter()
                    .filter(|p| p.hash_remainder == Some(hash_val))
                    .collect()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_range_partitioning_routing_and_pruning() {
        let config = TablePartitionConfig {
            table_name: "sales".into(),
            strategy: PartitionStrategy::Range {
                column: "created_at".into(),
                partitions: vec![
                    PartitionDef {
                        partition_name: "sales_2025".into(),
                        range_bound: Some(RangeBound {
                            start: Some("2025-01-01".into()),
                            end: Some("2026-01-01".into()),
                        }),
                        hash_modulus: None,
                        hash_remainder: None,
                    },
                    PartitionDef {
                        partition_name: "sales_2026".into(),
                        range_bound: Some(RangeBound {
                            start: Some("2026-01-01".into()),
                            end: Some("2027-01-01".into()),
                        }),
                        hash_modulus: None,
                        hash_remainder: None,
                    },
                ],
            },
        };

        // 1. Route row
        let mut row = HashMap::new();
        row.insert("created_at".into(), "2026-05-15".into());
        let target = PartitionPruner::route_row(&config, &row).unwrap();
        assert_eq!(target.partition_name, "sales_2026");

        // 2. Prune partitions for query WHERE created_at = '2025-06-01'
        let pruned = PartitionPruner::prune_partitions(&config, "created_at", "2025-06-01", "=");
        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0].partition_name, "sales_2025");
    }

    #[test]
    fn test_hash_partitioning_routing_and_pruning() {
        let config = TablePartitionConfig {
            table_name: "customers".into(),
            strategy: PartitionStrategy::Hash {
                column: "customer_id".into(),
                modulus: 2,
                partitions: vec![
                    PartitionDef {
                        partition_name: "cust_p0".into(),
                        range_bound: None,
                        hash_modulus: Some(2),
                        hash_remainder: Some(0),
                    },
                    PartitionDef {
                        partition_name: "cust_p1".into(),
                        range_bound: None,
                        hash_modulus: Some(2),
                        hash_remainder: Some(1),
                    },
                ],
            },
        };

        let mut row = HashMap::new();
        row.insert("customer_id".into(), "cust_12345".into());
        let target = PartitionPruner::route_row(&config, &row).unwrap();
        assert!(target.partition_name == "cust_p0" || target.partition_name == "cust_p1");

        // Equality filter on hash column prunes to exactly 1 partition
        let pruned = PartitionPruner::prune_partitions(&config, "customer_id", "cust_12345", "=");
        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0].partition_name, target.partition_name);
    }
}
