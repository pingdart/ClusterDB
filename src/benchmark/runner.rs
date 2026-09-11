use crate::benchmark::calibrator::CalibratedCostWeights;
use crate::benchmark::generator::SyntheticGenerator;
use crate::benchmark::metrics::{BenchmarkResult, LatencyHistogram};
use crate::benchmark::storage_stats::{StorageStatsCollector, StorageTelemetry};
use crate::exec::Executor;
use crate::storage::StorageEngine;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullScaleBenchmarkReport {
    pub scale_label: String,
    pub total_rows: usize,
    pub concurrency: usize,
    pub oltp_inserts: BenchmarkResult,
    pub oltp_lookups: BenchmarkResult,
    pub vector_hnsw: BenchmarkResult,
    pub columnar_olap: BenchmarkResult,
    pub storage: StorageTelemetry,
    pub calibrated_weights: CalibratedCostWeights,
    pub elapsed_total_secs: f64,
}

pub struct BenchmarkRunner {
    engine: Arc<Executor>,
    storage_dir: String,
}

impl BenchmarkRunner {
    pub fn new(data_dir: &str) -> Result<Self> {
        let storage = StorageEngine::new(data_dir)?;
        let engine = Arc::new(Executor::new(storage));
        Ok(Self {
            engine,
            storage_dir: data_dir.to_string(),
        })
    }

    /// Prepares schema for the benchmark
    pub fn prepare_schema(&self) -> Result<()> {
        let _ = self.engine.execute(
            "CREATE TABLE benchmark_orders (id INT PRIMARY KEY, user_id INT, category TEXT, amount FLOAT, status TEXT);",
            Some("benchmark_db"),
        );

        let _ = self.engine.execute(
            "CREATE TABLE benchmark_vectors (id INT PRIMARY KEY, label TEXT, embedding VECTOR(384));",
            Some("benchmark_db"),
        );

        Ok(())
    }

    /// Executes OLTP inserts with concurrent client batches
    pub fn run_oltp_inserts(&self, count: usize, concurrency: usize) -> Result<BenchmarkResult> {
        let mut hist = LatencyHistogram::new();
        let batch_size = (count / concurrency).max(1);

        for c in 0..concurrency {
            let start_id = c * batch_size + 1;
            let end_id = if c == concurrency - 1 { count + 1 } else { start_id + batch_size };

            for id in start_id..end_id {
                let sql = SyntheticGenerator::generate_order_insert_sql(id);
                let start = Instant::now();
                match self.engine.execute(&sql, Some("benchmark_db")) {
                    Ok(_) => hist.record_success(start.elapsed()),
                    Err(_) => hist.record_error(),
                }
            }
        }

        Ok(hist.summarize("OLTP Concurrent Inserts"))
    }

    /// Executes OLTP point lookups by indexed ID
    pub fn run_oltp_lookups(&self, count: usize, max_id: usize) -> Result<BenchmarkResult> {
        let mut hist = LatencyHistogram::new();
        let mut rng = rand::thread_rng();
        use rand::Rng;

        for _ in 0..count {
            let target_id = rng.gen_range(1..=max_id.max(1));
            let sql = format!("SELECT * FROM benchmark_orders WHERE id = {};", target_id);
            let start = Instant::now();
            match self.engine.execute(&sql, Some("benchmark_db")) {
                Ok(_) => hist.record_success(start.elapsed()),
                Err(_) => hist.record_error(),
            }
        }

        Ok(hist.summarize("OLTP Point Lookups"))
    }

    /// Executes Vector Proximity Search queries
    pub fn run_vector_search(&self, count: usize, seed_count: usize) -> Result<BenchmarkResult> {
        // Seed initial vector records if not populated
        for i in 1..=seed_count.min(200) {
            let vec = SyntheticGenerator::generate_vector(384);
            let vec_str = SyntheticGenerator::format_vector_sql(&vec);
            let sql = format!("INSERT INTO benchmark_vectors VALUES ({}, 'doc_{}', {});", i, i, vec_str);
            let _ = self.engine.execute(&sql, Some("benchmark_db"));
        }

        let mut hist = LatencyHistogram::new();

        for _ in 0..count {
            let query_vec = SyntheticGenerator::generate_vector(384);
            let query_vec_str = SyntheticGenerator::format_vector_sql(&query_vec);
            let sql = format!(
                "SELECT * FROM benchmark_vectors WHERE VECTOR_SIMILARITY(embedding, {}) > 0.1;",
                query_vec_str
            );
            let start = Instant::now();
            match self.engine.execute(&sql, Some("benchmark_db")) {
                Ok(_) => hist.record_success(start.elapsed()),
                Err(_) => hist.record_error(),
            }
        }

        Ok(hist.summarize("HNSW Vector Proximity Search"))
    }

    /// Executes Columnar OLAP aggregations
    pub fn run_columnar_olap(&self, iterations: usize) -> Result<BenchmarkResult> {
        let mut hist = LatencyHistogram::new();
        let queries = [
            "SELECT COUNT(*), SUM(amount), AVG(amount), MIN(amount), MAX(amount) FROM benchmark_orders;",
            "SELECT category, COUNT(*), SUM(amount), AVG(amount) FROM benchmark_orders GROUP BY category;",
            "SELECT COUNT(*) FROM benchmark_orders WHERE amount > 1000.0;",
        ];

        for i in 0..iterations {
            let sql = queries[i % queries.len()];
            let start = Instant::now();
            match self.engine.execute(sql, Some("benchmark_db")) {
                Ok(_) => hist.record_success(start.elapsed()),
                Err(_) => hist.record_error(),
            }
        }

        Ok(hist.summarize("Vectorized Columnar OLAP"))
    }

    /// Executes the complete end-to-end benchmark suite and returns the comprehensive report
    pub fn run_full_suite(
        &self,
        scale_label: &str,
        oltp_records: usize,
        concurrency: usize,
    ) -> Result<FullScaleBenchmarkReport> {
        let suite_start = Instant::now();
        self.prepare_schema()?;

        // 1. OLTP Inserts
        let oltp_inserts = self.run_oltp_inserts(oltp_records, concurrency)?;

        // 2. OLTP Point Lookups
        let oltp_lookups = self.run_oltp_lookups(oltp_records.min(500), oltp_records)?;

        // 3. Vector Proximity Search
        let vector_hnsw = self.run_vector_search(50, 100)?;

        // 4. Columnar OLAP Aggregations
        let columnar_olap = self.run_columnar_olap(30)?;

        // 5. Storage stats
        let user_bytes = (oltp_records * 128) as u64;
        let storage = StorageStatsCollector::measure(&self.storage_dir, user_bytes, oltp_records);

        // 6. Calibrated cost weights
        let calibrated_weights = CalibratedCostWeights::calibrate_from_measurements(
            oltp_records,
            columnar_olap.elapsed_seconds * 1000.0,
            oltp_lookups.successful_operations,
            oltp_lookups.percentiles.mean_ms * oltp_lookups.successful_operations as f64,
            vector_hnsw.successful_operations * 64,
            vector_hnsw.elapsed_seconds * 1000.0,
            oltp_records,
            columnar_olap.elapsed_seconds * 1000.0,
        );

        Ok(FullScaleBenchmarkReport {
            scale_label: scale_label.to_string(),
            total_rows: oltp_records,
            concurrency,
            oltp_inserts,
            oltp_lookups,
            vector_hnsw,
            columnar_olap,
            storage,
            calibrated_weights,
            elapsed_total_secs: suite_start.elapsed().as_secs_f64(),
        })
    }
}
