use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyPercentiles {
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub p99_9_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
    pub std_dev_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub workload_name: String,
    pub total_operations: usize,
    pub successful_operations: usize,
    pub failed_operations: usize,
    pub elapsed_seconds: f64,
    pub qps: f64,
    pub percentiles: LatencyPercentiles,
}

pub struct LatencyHistogram {
    latencies_ns: Vec<u64>,
    success_count: usize,
    error_count: usize,
    start_time: Instant,
}

impl LatencyHistogram {
    pub fn new() -> Self {
        Self {
            latencies_ns: Vec::with_capacity(100_000),
            success_count: 0,
            error_count: 0,
            start_time: Instant::now(),
        }
    }

    pub fn record_success(&mut self, duration: Duration) {
        self.latencies_ns.push(duration.as_nanos() as u64);
        self.success_count += 1;
    }

    pub fn record_error(&mut self) {
        self.error_count += 1;
    }

    pub fn summarize(&mut self, workload_name: &str) -> BenchmarkResult {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let total_ops = self.success_count + self.error_count;
        let qps = if elapsed > 0.0 {
            self.success_count as f64 / elapsed
        } else {
            0.0
        };

        if self.latencies_ns.is_empty() {
            return BenchmarkResult {
                workload_name: workload_name.to_string(),
                total_operations: total_ops,
                successful_operations: self.success_count,
                failed_operations: self.error_count,
                elapsed_seconds: elapsed,
                qps,
                percentiles: LatencyPercentiles {
                    p50_ms: 0.0,
                    p90_ms: 0.0,
                    p95_ms: 0.0,
                    p99_ms: 0.0,
                    p99_9_ms: 0.0,
                    min_ms: 0.0,
                    max_ms: 0.0,
                    mean_ms: 0.0,
                    std_dev_ms: 0.0,
                },
            };
        }

        self.latencies_ns.sort_unstable();

        let len = self.latencies_ns.len();
        let percentile_val = |p: f64| -> f64 {
            let idx = ((len as f64) * (p / 100.0)).round() as usize;
            let clamped = idx.min(len - 1);
            self.latencies_ns[clamped] as f64 / 1_000_000.0
        };

        let min_ms = self.latencies_ns[0] as f64 / 1_000_000.0;
        let max_ms = self.latencies_ns[len - 1] as f64 / 1_000_000.0;

        let sum_ms: f64 = self.latencies_ns.iter().map(|&ns| ns as f64 / 1_000_000.0).sum();
        let mean_ms = sum_ms / (len as f64);

        let variance: f64 = self
            .latencies_ns
            .iter()
            .map(|&ns| {
                let ms = ns as f64 / 1_000_000.0;
                (ms - mean_ms).powi(2)
            })
            .sum::<f64>()
            / (len as f64);
        let std_dev_ms = variance.sqrt();

        BenchmarkResult {
            workload_name: workload_name.to_string(),
            total_operations: total_ops,
            successful_operations: self.success_count,
            failed_operations: self.error_count,
            elapsed_seconds: elapsed,
            qps,
            percentiles: LatencyPercentiles {
                p50_ms: percentile_val(50.0),
                p90_ms: percentile_val(90.0),
                p95_ms: percentile_val(95.0),
                p99_ms: percentile_val(99.0),
                p99_9_ms: percentile_val(99.9),
                min_ms,
                max_ms,
                mean_ms,
                std_dev_ms,
            },
        }
    }
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latency_histogram_percentiles() {
        let mut hist = LatencyHistogram::new();
        for i in 1..=1000 {
            hist.record_success(Duration::from_millis(i));
        }

        let res = hist.summarize("OLTP Point Lookup");
        assert_eq!(res.successful_operations, 1000);
        assert!((res.percentiles.min_ms - 1.0).abs() < 0.1);
        assert!((res.percentiles.p50_ms - 500.0).abs() < 5.0);
        assert!((res.percentiles.p95_ms - 950.0).abs() < 5.0);
        assert!((res.percentiles.p99_ms - 990.0).abs() < 5.0);
        assert!((res.percentiles.max_ms - 1000.0).abs() < 0.1);
    }
}
