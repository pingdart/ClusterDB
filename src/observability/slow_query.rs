use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlowQueryRecord {
    pub query_id: String,
    pub sql: String,
    pub tenant: String,
    pub duration_ms: f64,
    pub rows_scanned: usize,
    pub rows_returned: usize,
    pub timestamp_epoch_ms: u64,
    pub bottleneck_stage: String,
    pub recommendation: String,
}

pub struct SlowQueryLogger {
    records: RwLock<VecDeque<SlowQueryRecord>>,
    pub threshold_ms: f64,
    max_records: usize,
}

impl SlowQueryLogger {
    pub fn new(threshold_ms: f64, max_records: usize) -> Self {
        Self {
            records: RwLock::new(VecDeque::with_capacity(max_records)),
            threshold_ms,
            max_records,
        }
    }

    /// Evaluates execution stats and logs slow queries with automated heuristic recommendations
    pub fn maybe_log_query(
        &self,
        sql: &str,
        tenant: &str,
        duration_ms: f64,
        rows_scanned: usize,
        rows_returned: usize,
        bottleneck: Option<&str>,
    ) {
        if duration_ms < self.threshold_ms {
            return;
        }

        let query_id = format!("q-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let sql_upper = sql.to_uppercase();
        let bottleneck_stage = bottleneck.unwrap_or_else(|| {
            if sql_upper.contains("SEMANTIC_MATCH") {
                "SemanticEmbeddingMatch"
            } else if sql_upper.contains("GROUP BY") {
                "AggregationGrouping"
            } else if rows_scanned > rows_returned * 5 {
                "SequentialTableScan"
            } else {
                "QueryExecution"
            }
        }).to_string();

        let recommendation = if sql_upper.contains("SEMANTIC_MATCH") && !sql_upper.contains("HNSW") {
            "High semantic search latency. Ensure embeddings are indexed in HNSW proximity graph using GENERATED FROM or CREATE VECTOR INDEX."
        } else if sql_upper.contains("GROUP BY") && rows_scanned > 1000 {
            "Row-oriented aggregation bottleneck detected. Consider executing analytical queries over the HTAP vectorized columnar engine."
        } else if rows_scanned > rows_returned * 5 && rows_scanned > 500 {
            "High selectivity table scan detected. Consider creating a secondary B-tree index on the WHERE filter columns."
        } else {
            "Execution latency exceeded threshold. Run EXPLAIN ANALYZE to inspect stage timings and row progression."
        }.to_string();

        let record = SlowQueryRecord {
            query_id,
            sql: sql.to_string(),
            tenant: tenant.to_string(),
            duration_ms,
            rows_scanned,
            rows_returned,
            timestamp_epoch_ms: now,
            bottleneck_stage,
            recommendation,
        };

        let mut buffer = self.records.write().unwrap();
        if buffer.len() >= self.max_records {
            buffer.pop_front();
        }
        buffer.push_back(record);
    }

    pub fn get_slow_queries(&self, limit: usize, tenant_filter: Option<&str>) -> Vec<SlowQueryRecord> {
        let buffer = self.records.read().unwrap();
        buffer
            .iter()
            .rev()
            .filter(|r| {
                if let Some(t) = tenant_filter {
                    r.tenant == t
                } else {
                    true
                }
            })
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn clear(&self) {
        let mut buffer = self.records.write().unwrap();
        buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slow_query_logging_and_heuristics() {
        let logger = SlowQueryLogger::new(10.0, 50);

        // Fast query -> ignored
        logger.maybe_log_query("SELECT 1", "default", 2.5, 1, 1, None);
        assert_eq!(logger.get_slow_queries(10, None).len(), 0);

        // Slow unindexed scan query -> logged with recommendation
        logger.maybe_log_query(
            "SELECT * FROM patients WHERE blood_type = 'O+'",
            "org_health",
            65.2,
            5000,
            12,
            None,
        );

        let slow = logger.get_slow_queries(10, None);
        assert_eq!(slow.len(), 1);
        assert_eq!(slow[0].tenant, "org_health");
        assert!(slow[0].recommendation.contains("secondary B-tree index"));
    }
}
