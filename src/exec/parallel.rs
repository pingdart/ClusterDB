use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::mpsc::channel;
use std::thread;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggOp {
    Sum,
    Count,
    Avg,
    Min,
    Max,
}

pub struct ParallelExecutor;

impl ParallelExecutor {
    /// Parallel Scan & Filter: Dispatches row chunks across worker threads
    pub fn parallel_filter<T, F>(items: Vec<T>, predicate: F, num_threads: usize) -> Vec<T>
    where
        T: Send + 'static,
        F: Fn(&T) -> bool + Send + Sync + 'static,
    {
        if items.is_empty() {
            return Vec::new();
        }
        let total = items.len();
        let threads = num_threads.max(1).min(total);
        let chunk_size = (total + threads - 1) / threads;

        let predicate = std::sync::Arc::new(predicate);
        let (tx, rx) = channel();

        let mut chunks: Vec<Vec<T>> = Vec::with_capacity(threads);
        let mut cur_chunk = Vec::with_capacity(chunk_size);
        for item in items {
            cur_chunk.push(item);
            if cur_chunk.len() == chunk_size {
                chunks.push(cur_chunk);
                cur_chunk = Vec::with_capacity(chunk_size);
            }
        }
        if !cur_chunk.is_empty() {
            chunks.push(cur_chunk);
        }

        let actual_threads = chunks.len();
        for chunk in chunks {
            let tx_clone = tx.clone();
            let pred_clone = predicate.clone();
            thread::spawn(move || {
                let filtered: Vec<T> = chunk.into_iter().filter(|x| pred_clone(x)).collect();
                let _ = tx_clone.send(filtered);
            });
        }
        drop(tx);

        let mut results = Vec::with_capacity(total);
        for _ in 0..actual_threads {
            if let Ok(batch) = rx.recv() {
                results.extend(batch);
            }
        }
        results
    }

    /// Parallel Aggregation: Evaluates partial sums/counts/min/max per thread, then merges
    pub fn parallel_aggregate(
        rows: &[HashMap<String, String>],
        target_column: &str,
        op: AggOp,
        num_threads: usize,
    ) -> Option<f64> {
        if rows.is_empty() {
            return match op {
                AggOp::Count => Some(0.0),
                _ => None,
            };
        }

        let total = rows.len();
        let threads = num_threads.max(1).min(total);
        let chunk_size = (total + threads - 1) / threads;

        let (tx, rx) = channel();

        let chunks: Vec<Vec<HashMap<String, String>>> = rows
            .chunks(chunk_size)
            .map(|c| c.to_vec())
            .collect();

        let actual_chunks = chunks.len();
        let col = target_column.to_string();

        for chunk in chunks {
            let tx_clone = tx.clone();
            let col_name = col.clone();
            thread::spawn(move || {
                let mut sum = 0.0;
                let mut count = 0.0;
                let mut min = f64::MAX;
                let mut max = f64::MIN;

                for r in chunk {
                    if let Some(val_str) = r.get(&col_name) {
                        if let Ok(num) = val_str.parse::<f64>() {
                            sum += num;
                            count += 1.0;
                            if num < min { min = num; }
                            if num > max { max = num; }
                        }
                    }
                }
                let _ = tx_clone.send((sum, count, min, max));
            });
        }
        drop(tx);

        let mut total_sum = 0.0;
        let mut total_count = 0.0;
        let mut global_min = f64::MAX;
        let mut global_max = f64::MIN;

        for _ in 0..actual_chunks {
            if let Ok((s, c, mi, ma)) = rx.recv() {
                total_sum += s;
                total_count += c;
                if mi < global_min { global_min = mi; }
                if ma > global_max { global_max = ma; }
            }
        }

        if total_count == 0.0 && op != AggOp::Count {
            return None;
        }

        match op {
            AggOp::Sum => Some(total_sum),
            AggOp::Count => Some(total_count),
            AggOp::Avg => Some(total_sum / total_count),
            AggOp::Min => Some(global_min),
            AggOp::Max => Some(global_max),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parallel_filter() {
        let numbers: Vec<i32> = (1..=1000).collect();
        let evens = ParallelExecutor::parallel_filter(numbers, |&x| x % 2 == 0, 4);
        assert_eq!(evens.len(), 500);
    }

    #[test]
    fn test_parallel_aggregate_sum_and_avg() {
        let mut rows = Vec::new();
        for i in 1..=100 {
            let mut r = HashMap::new();
            r.insert("amount".into(), (i * 10).to_string());
            rows.push(r);
        }

        // Sum of 10..1000 = 10 * (100 * 101 / 2) = 50500
        let sum = ParallelExecutor::parallel_aggregate(&rows, "amount", AggOp::Sum, 4).unwrap();
        assert_eq!(sum, 50500.0);

        let avg = ParallelExecutor::parallel_aggregate(&rows, "amount", AggOp::Avg, 4).unwrap();
        assert_eq!(avg, 505.0);

        let max = ParallelExecutor::parallel_aggregate(&rows, "amount", AggOp::Max, 4).unwrap();
        assert_eq!(max, 1000.0);
    }
}
