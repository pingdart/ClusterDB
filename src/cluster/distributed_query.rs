use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialAggregate {
    pub group_key: String,
    pub partial_sum: f64,
    pub partial_count: u64,
    pub partial_min: f64,
    pub partial_max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalAggregateResult {
    pub group_key: String,
    pub sum: f64,
    pub count: u64,
    pub avg: f64,
    pub min: f64,
    pub max: f64,
}

pub struct DistributedQueryCoordinator;

impl DistributedQueryCoordinator {
    /// Distributed GROUP BY & Aggregate:
    /// Merges partial partition aggregates from all cluster nodes into final global aggregates.
    pub fn merge_distributed_aggregates(
        node_partial_results: Vec<Vec<PartialAggregate>>,
    ) -> Vec<GlobalAggregateResult> {
        let mut groups: HashMap<String, (f64, u64, f64, f64)> = HashMap::new(); // key -> (sum, count, min, max)

        for node_partials in node_partial_results {
            for partial in node_partials {
                let entry = groups.entry(partial.group_key).or_insert((0.0, 0, f64::MAX, f64::MIN));
                entry.0 += partial.partial_sum;
                entry.1 += partial.partial_count;
                if partial.partial_min < entry.2 { entry.2 = partial.partial_min; }
                if partial.partial_max > entry.3 { entry.3 = partial.partial_max; }
            }
        }

        let mut results = Vec::new();
        for (group_key, (sum, count, min, max)) in groups {
            let avg = if count > 0 { sum / (count as f64) } else { 0.0 };
            results.push(GlobalAggregateResult {
                group_key,
                sum,
                count,
                avg,
                min,
                max,
            });
        }

        // Sort by group_key for deterministic order
        results.sort_by(|a, b| a.group_key.cmp(&b.group_key));
        results
    }

    /// Distributed Scan:
    /// Scatters pushdown predicates across cluster data nodes and gathers streams in parallel
    pub fn execute_distributed_scan<F>(
        node_partitions: Vec<(String, Vec<HashMap<String, String>>)>, // (node_id, rows)
        predicate: Option<F>,
    ) -> Vec<HashMap<String, String>>
    where
        F: Fn(&HashMap<String, String>) -> bool + Sync + Send,
    {
        let mut gathered_results = Vec::new();
        for (_node_id, rows) in node_partitions {
            for row in rows {
                if let Some(ref pred) = predicate {
                    if pred(&row) {
                        gathered_results.push(row);
                    }
                } else {
                    gathered_results.push(row);
                }
            }
        }
        gathered_results
    }

    /// Distributed Broadcast Join:
    /// Given a small dimension table broadcasted to all nodes, joins against partitioned fact table rows
    pub fn execute_broadcast_join(
        broadcast_dimension_rows: &[HashMap<String, String>],
        partition_fact_rows: &[HashMap<String, String>],
        dimension_key: &str,
        fact_key: &str,
    ) -> Vec<HashMap<String, String>> {
        let mut dim_hash_map: HashMap<String, Vec<&HashMap<String, String>>> = HashMap::new();
        for dim_r in broadcast_dimension_rows {
            if let Some(val) = dim_r.get(dimension_key) {
                dim_hash_map.entry(val.clone()).or_default().push(dim_r);
            }
        }

        let mut joined_results = Vec::new();
        for fact_r in partition_fact_rows {
            if let Some(val) = fact_r.get(fact_key) {
                if let Some(dim_matches) = dim_hash_map.get(val) {
                    for dim_r in dim_matches {
                        let mut combined = fact_r.clone();
                        for (k, v) in *dim_r {
                            combined.insert(k.clone(), v.clone());
                        }
                        joined_results.push(combined);
                    }
                }
            }
        }

        joined_results
    }

    /// Distributed Shuffled / Partitioned Hash Join:
    /// For two large partitioned tables, rehashes both streams across N nodes by join key,
    /// performs local hash joins per node partition, and unifies output.
    pub fn execute_distributed_hash_join(
        num_cluster_nodes: usize,
        left_rows: Vec<HashMap<String, String>>,
        right_rows: Vec<HashMap<String, String>>,
        left_key: &str,
        right_key: &str,
    ) -> Vec<HashMap<String, String>> {
        use std::hash::{Hash, Hasher};
        let hash_key = |key_val: &str| -> usize {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            key_val.hash(&mut hasher);
            (hasher.finish() as usize) % num_cluster_nodes
        };

        // 1. Shuffle left relation into N buckets
        let mut left_buckets: Vec<Vec<HashMap<String, String>>> = vec![Vec::new(); num_cluster_nodes];
        for row in left_rows {
            if let Some(val) = row.get(left_key) {
                let bucket = hash_key(val);
                left_buckets[bucket].push(row);
            }
        }

        // 2. Shuffle right relation into N buckets
        let mut right_buckets: Vec<Vec<HashMap<String, String>>> = vec![Vec::new(); num_cluster_nodes];
        for row in right_rows {
            if let Some(val) = row.get(right_key) {
                let bucket = hash_key(val);
                right_buckets[bucket].push(row);
            }
        }

        // 3. Perform local hash join in parallel on each node partition
        let mut global_joined = Vec::new();
        for node_idx in 0..num_cluster_nodes {
            let node_left = &left_buckets[node_idx];
            let node_right = &right_buckets[node_idx];

            let mut node_hash_map: HashMap<String, Vec<&HashMap<String, String>>> = HashMap::new();
            for l_row in node_left {
                if let Some(k) = l_row.get(left_key) {
                    node_hash_map.entry(k.clone()).or_default().push(l_row);
                }
            }

            for r_row in node_right {
                if let Some(k) = r_row.get(right_key) {
                    if let Some(matches) = node_hash_map.get(k) {
                        for l_match in matches {
                            let mut merged = (*l_match).clone();
                            for (rk, rv) in r_row {
                                merged.insert(rk.clone(), rv.clone());
                            }
                            global_joined.push(merged);
                        }
                    }
                }
            }
        }

        global_joined
    }

    /// Distributed K-Way Merge Sort:
    /// Merges pre-sorted streams from multiple nodes, with optional LIMIT and OFFSET
    pub fn execute_distributed_sort(
        node_streams: Vec<Vec<HashMap<String, String>>>,
        sort_key: &str,
        ascending: bool,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Vec<HashMap<String, String>> {
        let mut all_rows = Vec::new();
        for stream in node_streams {
            all_rows.extend(stream);
        }

        all_rows.sort_by(|a, b| {
            let va = a.get(sort_key).map(|s| s.as_str()).unwrap_or("");
            let vb = b.get(sort_key).map(|s| s.as_str()).unwrap_or("");
            // Try numeric comparison first
            let ord = match (va.parse::<f64>(), vb.parse::<f64>()) {
                (Ok(fa), Ok(fb)) => fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal),
                _ => va.cmp(vb),
            };
            if ascending { ord } else { ord.reverse() }
        });

        let off = offset.unwrap_or(0);
        let skipped = all_rows.into_iter().skip(off);
        if let Some(lim) = limit {
            skipped.take(lim).collect()
        } else {
            skipped.collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distributed_group_by_aggregation() {
        // Node 1 partials:
        // Hyderabad: sum 300, count 3
        // Bengaluru: sum 500, count 5
        let node1 = vec![
            PartialAggregate {
                group_key: "Hyderabad".into(),
                partial_sum: 300.0,
                partial_count: 3,
                partial_min: 80.0,
                partial_max: 120.0,
            },
            PartialAggregate {
                group_key: "Bengaluru".into(),
                partial_sum: 500.0,
                partial_count: 5,
                partial_min: 90.0,
                partial_max: 110.0,
            },
        ];

        // Node 2 partials:
        // Hyderabad: sum 700, count 7
        let node2 = vec![
            PartialAggregate {
                group_key: "Hyderabad".into(),
                partial_sum: 700.0,
                partial_count: 7,
                partial_min: 70.0,
                partial_max: 130.0,
            },
        ];

        let global = DistributedQueryCoordinator::merge_distributed_aggregates(vec![node1, node2]);

        assert_eq!(global.len(), 2);
        // Bengaluru: sum 500, count 5, avg 100.0
        assert_eq!(global[0].group_key, "Bengaluru");
        assert_eq!(global[0].sum, 500.0);
        assert_eq!(global[0].count, 5);
        assert_eq!(global[0].avg, 100.0);

        // Hyderabad: sum 1000, count 10, avg 100.0, min 70.0, max 130.0
        assert_eq!(global[1].group_key, "Hyderabad");
        assert_eq!(global[1].sum, 1000.0);
        assert_eq!(global[1].count, 10);
        assert_eq!(global[1].avg, 100.0);
        assert_eq!(global[1].min, 70.0);
        assert_eq!(global[1].max, 130.0);
    }

    #[test]
    fn test_distributed_broadcast_join() {
        // Replicated dimension table (e.g. stores)
        let stores = vec![
            HashMap::from([("store_id".into(), "S1".into()), ("store_city".into(), "Hyderabad".into())]),
            HashMap::from([("store_id".into(), "S2".into()), ("store_city".into(), "Bengaluru".into())]),
        ];

        // Local partition fact table (e.g. sales on Node 3)
        let sales = vec![
            HashMap::from([("sale_id".into(), "101".into()), ("store_id".into(), "S1".into()), ("amount".into(), "450".into())]),
            HashMap::from([("sale_id".into(), "102".into()), ("store_id".into(), "S2".into()), ("amount".into(), "800".into())]),
        ];

        let joined = DistributedQueryCoordinator::execute_broadcast_join(&stores, &sales, "store_id", "store_id");
        assert_eq!(joined.len(), 2);
        assert_eq!(joined[0].get("store_city").unwrap(), "Hyderabad");
        assert_eq!(joined[1].get("store_city").unwrap(), "Bengaluru");
    }

    #[test]
    fn test_4_node_distributed_scan_and_filter() {
        // Topology: Node 1, Node 2, Node 3, Node 4
        let node1 = ("node_1".into(), vec![
            HashMap::from([("id".into(), "1".into()), ("status".into(), "active".into())]),
            HashMap::from([("id".into(), "2".into()), ("status".into(), "inactive".into())]),
        ]);
        let node2 = ("node_2".into(), vec![
            HashMap::from([("id".into(), "3".into()), ("status".into(), "active".into())]),
        ]);
        let node3 = ("node_3".into(), vec![
            HashMap::from([("id".into(), "4".into()), ("status".into(), "active".into())]),
            HashMap::from([("id".into(), "5".into()), ("status".into(), "pending".into())]),
        ]);
        let node4 = ("node_4".into(), vec![
            HashMap::from([("id".into(), "6".into()), ("status".into(), "active".into())]),
        ]);

        let partitions = vec![node1, node2, node3, node4];
        let active_rows = DistributedQueryCoordinator::execute_distributed_scan(
            partitions,
            Some(|row: &HashMap<String, String>| row.get("status").map(|s| s.as_str()) == Some("active")),
        );

        assert_eq!(active_rows.len(), 4);
        let ids: Vec<&str> = active_rows.iter().map(|r| r.get("id").unwrap().as_str()).collect();
        assert_eq!(ids, vec!["1", "3", "4", "6"]);
    }

    #[test]
    fn test_4_node_distributed_hash_join() {
        // Left table: customers across 4 nodes
        let customers = vec![
            HashMap::from([("c_id".into(), "c10".into()), ("name".into(), "Alice".into())]),
            HashMap::from([("c_id".into(), "c20".into()), ("name".into(), "Bob".into())]),
            HashMap::from([("c_id".into(), "c30".into()), ("name".into(), "Charlie".into())]),
            HashMap::from([("c_id".into(), "c40".into()), ("name".into(), "Diana".into())]),
        ];

        // Right table: orders across 4 nodes
        let orders = vec![
            HashMap::from([("o_id".into(), "o1".into()), ("c_id".into(), "c10".into()), ("amount".into(), "100".into())]),
            HashMap::from([("o_id".into(), "o2".into()), ("c_id".into(), "c30".into()), ("amount".into(), "300".into())]),
        ];

        let joined = DistributedQueryCoordinator::execute_distributed_hash_join(
            4, // 4 cluster nodes
            customers,
            orders,
            "c_id",
            "c_id",
        );

        assert_eq!(joined.len(), 2);
        let names: Vec<&str> = joined.iter().map(|r| r.get("name").unwrap().as_str()).collect();
        assert!(names.contains(&"Alice"));
        assert!(names.contains(&"Charlie"));
    }

    #[test]
    fn test_4_node_distributed_sort() {
        // Pre-sorted streams from 4 cluster nodes
        let stream1 = vec![
            HashMap::from([("val".into(), "10".into())]),
            HashMap::from([("val".into(), "50".into())]),
        ];
        let stream2 = vec![
            HashMap::from([("val".into(), "20".into())]),
            HashMap::from([("val".into(), "60".into())]),
        ];
        let stream3 = vec![
            HashMap::from([("val".into(), "30".into())]),
            HashMap::from([("val".into(), "70".into())]),
        ];
        let stream4 = vec![
            HashMap::from([("val".into(), "40".into())]),
            HashMap::from([("val".into(), "80".into())]),
        ];

        // Distributed sort ascending with LIMIT 4, OFFSET 2
        let sorted = DistributedQueryCoordinator::execute_distributed_sort(
            vec![stream1, stream2, stream3, stream4],
            "val",
            true,
            Some(4),
            Some(2),
        );

        assert_eq!(sorted.len(), 4);
        let vals: Vec<&str> = sorted.iter().map(|r| r.get("val").unwrap().as_str()).collect();
        assert_eq!(vals, vec!["30", "40", "50", "60"]);
    }
}
