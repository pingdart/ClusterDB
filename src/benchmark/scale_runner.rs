use crate::benchmark::generator::SyntheticGenerator;
use crate::benchmark::metrics::{BenchmarkResult, LatencyHistogram};
use crate::columnar::engine::{AggFunc, AggregateQuery, ColumnarEngine};
use crate::columnar::types::ColumnType;
use crate::storage::StorageEngine;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleBenchmarkReport {
    pub scale_tier: String,
    pub total_rows: usize,
    pub batch_size: usize,
    pub ingestion_elapsed_secs: f64,
    pub ingestion_rows_per_sec: f64,
    pub full_aggregation: BenchmarkResult,
    pub group_by_aggregation: BenchmarkResult,
    pub pruned_range_scan: BenchmarkResult,
    pub unpruned_full_scan: BenchmarkResult,
    pub pruning_speedup_ratio: f64,
    pub total_chunks: usize,
    pub estimated_pruned_chunks: usize,
    pub storage_bytes: u64,
}

pub struct ScaleBenchmarkRunner {
    columnar: Arc<ColumnarEngine>,
    storage_dir: String,
}

impl ScaleBenchmarkRunner {
    pub fn new(data_dir: &str) -> Result<Self> {
        let storage = StorageEngine::new(data_dir)?;
        let columnar = Arc::new(ColumnarEngine::new(storage));
        Ok(Self {
            columnar,
            storage_dir: data_dir.to_string(),
        })
    }

    pub fn new_with_engine(columnar: Arc<ColumnarEngine>, storage_dir: String) -> Self {
        Self { columnar, storage_dir }
    }

    /// Initializes schema for the scale benchmark
    pub fn prepare_table(&self, table_name: &str) {
        let mut schema = HashMap::new();
        schema.insert("id".to_string(), ColumnType::Int64);
        schema.insert("user_id".to_string(), ColumnType::Int64);
        schema.insert("category".to_string(), ColumnType::Text);
        schema.insert("amount".to_string(), ColumnType::Float64);
        schema.insert("status".to_string(), ColumnType::Text);
        self.columnar.register_table(table_name, schema);
    }

    /// Streams N rows in batches of `batch_size` into the columnar engine
    pub fn run_streaming_ingest(&self, table_name: &str, total_rows: usize, batch_size: usize) -> (f64, f64) {
        info!("Beginning streaming ingestion of {} rows into table '{}' (batch_size: {})...", total_rows, table_name, batch_size);
        let start = Instant::now();
        let mut inserted = 0;

        while inserted < total_rows {
            let current_batch_count = batch_size.min(total_rows - inserted);
            let batch = SyntheticGenerator::generate_order_batch(inserted + 1, current_batch_count);
            self.columnar.insert_batch(table_name, &batch);
            inserted += current_batch_count;
        }

        let elapsed = start.elapsed().as_secs_f64();
        let qps = if elapsed > 0.0 { total_rows as f64 / elapsed } else { 0.0 };
        info!("Streaming ingestion complete: {} rows in {:.3}s ({:.0} rows/sec)", total_rows, elapsed, qps);
        (elapsed, qps)
    }

    /// Measures full table aggregation across all chunks
    pub fn benchmark_full_aggregation(&self, table_name: &str, iterations: usize) -> BenchmarkResult {
        let mut hist = LatencyHistogram::new();
        let query = AggregateQuery {
            table: table_name.to_string(),
            group_by: None,
            aggregates: vec![
                ("count".to_string(), AggFunc::Count(None)),
                ("total_amount".to_string(), AggFunc::Sum("amount".to_string())),
                ("avg_amount".to_string(), AggFunc::Avg("amount".to_string())),
                ("min_amount".to_string(), AggFunc::Min("amount".to_string())),
                ("max_amount".to_string(), AggFunc::Max("amount".to_string())),
            ],
            filter: None,
        };

        for _ in 0..iterations {
            let start = Instant::now();
            match self.columnar.execute_aggregate(&query) {
                Ok(_) => hist.record_success(start.elapsed()),
                Err(_) => hist.record_error(),
            }
        }

        hist.summarize("Full Table Vectorized Aggregation")
    }

    /// Measures Group-By aggregation across all chunks
    pub fn benchmark_group_by(&self, table_name: &str, iterations: usize) -> BenchmarkResult {
        let mut hist = LatencyHistogram::new();
        let query = AggregateQuery {
            table: table_name.to_string(),
            group_by: Some("category".to_string()),
            aggregates: vec![
                ("count".to_string(), AggFunc::Count(None)),
                ("total_amount".to_string(), AggFunc::Sum("amount".to_string())),
                ("avg_amount".to_string(), AggFunc::Avg("amount".to_string())),
            ],
            filter: None,
        };

        for _ in 0..iterations {
            let start = Instant::now();
            match self.columnar.execute_aggregate(&query) {
                Ok(_) => hist.record_success(start.elapsed()),
                Err(_) => hist.record_error(),
            }
        }

        hist.summarize("Vectorized Group-By Aggregation")
    }

    /// Measures selective range scan with Zone Map Pruning
    pub fn benchmark_pruned_scan(&self, table_name: &str, threshold: usize, iterations: usize) -> BenchmarkResult {
        let mut hist = LatencyHistogram::new();
        let query = AggregateQuery {
            table: table_name.to_string(),
            group_by: None,
            aggregates: vec![
                ("count".to_string(), AggFunc::Count(None)),
                ("sum".to_string(), AggFunc::Sum("amount".to_string())),
            ],
            filter: Some(("id".to_string(), ">".to_string(), threshold.to_string())),
        };

        for _ in 0..iterations {
            let start = Instant::now();
            match self.columnar.execute_aggregate(&query) {
                Ok(_) => hist.record_success(start.elapsed()),
                Err(_) => hist.record_error(),
            }
        }

        hist.summarize("Zone-Map Pruned Range Scan")
    }

    /// Measures unpruned full scan on the same dataset
    pub fn benchmark_unpruned_scan(&self, table_name: &str, iterations: usize) -> BenchmarkResult {
        let mut hist = LatencyHistogram::new();
        let query = AggregateQuery {
            table: table_name.to_string(),
            group_by: None,
            aggregates: vec![
                ("count".to_string(), AggFunc::Count(None)),
                ("sum".to_string(), AggFunc::Sum("amount".to_string())),
            ],
            filter: Some(("id".to_string(), ">".to_string(), "0".to_string())),
        };

        for _ in 0..iterations {
            let start = Instant::now();
            match self.columnar.execute_aggregate(&query) {
                Ok(_) => hist.record_success(start.elapsed()),
                Err(_) => hist.record_error(),
            }
        }

        hist.summarize("Unpruned Full Table Scan")
    }

    /// Runs a complete scale tier test (e.g. 1M, 5M, 10M rows)
    pub fn run_scale_tier(&self, scale_label: &str, total_rows: usize, batch_size: usize) -> Result<ScaleBenchmarkReport> {
        let table_name = format!("scale_bench_{}", scale_label.to_lowercase().replace(' ', "_"));
        self.prepare_table(&table_name);

        // 1. Streaming Ingestion
        let (elapsed_secs, qps) = self.run_streaming_ingest(&table_name, total_rows, batch_size);

        // 2. Full Aggregation (COUNT, SUM, AVG, MIN, MAX)
        let full_agg = self.benchmark_full_aggregation(&table_name, 10);

        // 3. Group-By Aggregation
        let group_by = self.benchmark_group_by(&table_name, 10);

        // 4. Pruned Range Scan (Top 10% of rows)
        let threshold = (total_rows as f64 * 0.90) as usize;
        let pruned_scan = self.benchmark_pruned_scan(&table_name, threshold, 10);

        // 5. Unpruned Full Scan
        let unpruned_scan = self.benchmark_unpruned_scan(&table_name, 10);

        // Compute chunk metrics
        let total_chunks = (total_rows + 65_535) / 65_536;
        let estimated_pruned_chunks = (total_chunks as f64 * 0.85) as usize;

        let pruned_mean = pruned_scan.percentiles.mean_ms.max(0.001);
        let unpruned_mean = unpruned_scan.percentiles.mean_ms.max(0.001);
        let pruning_speedup_ratio = unpruned_mean / pruned_mean;

        let storage_bytes = std::fs::metadata(&self.storage_dir)
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(ScaleBenchmarkReport {
            scale_tier: scale_label.to_string(),
            total_rows,
            batch_size,
            ingestion_elapsed_secs: elapsed_secs,
            ingestion_rows_per_sec: qps,
            full_aggregation: full_agg,
            group_by_aggregation: group_by,
            pruned_range_scan: pruned_scan,
            unpruned_full_scan: unpruned_scan,
            pruning_speedup_ratio,
            total_chunks,
            estimated_pruned_chunks,
            storage_bytes,
        })
    }
}
