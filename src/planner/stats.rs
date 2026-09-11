use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Fixed-size HyperLogLog with 64 registers (6-bit register index)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperLogLog {
    registers: Vec<u8>,
}

impl HyperLogLog {
    pub fn new() -> Self {
        Self {
            registers: vec![0; 64],
        }
    }

    pub fn insert<T: Hash>(&mut self, item: &T) {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        item.hash(&mut hasher);
        let hash = hasher.finish();

        // Use top 6 bits for register index (2^6 = 64)
        let idx = (hash >> 58) as usize;
        // Remaining 58 bits for leading zeros count
        let w = hash << 6;
        let leading_zeros = (w.leading_zeros() + 1) as u8;

        if leading_zeros > self.registers[idx] {
            self.registers[idx] = leading_zeros;
        }
    }

    pub fn estimate(&self) -> usize {
        let m = self.registers.len() as f64;
        let alpha = 0.709; // For m = 64
        let sum: f64 = self.registers.iter().map(|&val| 2.0f64.powi(-(val as i32))).sum();
        let raw_estimate = alpha * m * m / sum;

        // Linear counting for small range
        if raw_estimate <= 2.5 * m {
            let zeros = self.registers.iter().filter(|&&v| v == 0).count();
            if zeros > 0 {
                return (m * (m / zeros as f64).ln()).round() as usize;
            }
        }

        raw_estimate.round() as usize
    }
}

impl Default for HyperLogLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Typed value for equi-depth histogram boundaries
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub enum HistogramValue {
    Int(i64),
    Float(f64),
    Text(String),
    Bool(bool),
}

impl HistogramValue {
    pub fn from_str_auto(s: &str) -> Self {
        if let Ok(i) = s.parse::<i64>() {
            HistogramValue::Int(i)
        } else if let Ok(f) = s.parse::<f64>() {
            HistogramValue::Float(f)
        } else if s.eq_ignore_ascii_case("true") {
            HistogramValue::Bool(true)
        } else if s.eq_ignore_ascii_case("false") {
            HistogramValue::Bool(false)
        } else {
            HistogramValue::Text(s.to_string())
        }
    }

    /// Interpolates fraction [0.0, 1.0] of a range [self, upper]
    pub fn range_fraction(&self, upper: &HistogramValue, target: &HistogramValue) -> f64 {
        match (self, upper, target) {
            (HistogramValue::Int(l), HistogramValue::Int(u), HistogramValue::Int(t)) => {
                if u <= l { 0.5 } else { (*t - *l) as f64 / (*u - *l) as f64 }
            }
            (HistogramValue::Float(l), HistogramValue::Float(u), HistogramValue::Float(t)) => {
                if u <= l { 0.5 } else { (*t - *l) / (*u - *l) }
            }
            _ => 0.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramBucket {
    pub lower_bound: HistogramValue,
    pub upper_bound: HistogramValue,
    pub row_count: usize,
    pub distinct_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnStats {
    pub column_name: String,
    pub null_count: usize,
    pub ndv: usize, // Estimated via HyperLogLog
    pub min_val: Option<HistogramValue>,
    pub max_val: Option<HistogramValue>,
    pub buckets: Vec<HistogramBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableStats {
    pub table_name: String,
    pub total_rows: usize,
    pub analyzed_rows: usize,
    pub modification_counter: usize,
    pub last_analyzed_epoch_secs: u64,
    pub columns: HashMap<String, ColumnStats>,
}

impl TableStats {
    pub fn new(table_name: impl Into<String>) -> Self {
        Self {
            table_name: table_name.into(),
            total_rows: 0,
            analyzed_rows: 0,
            modification_counter: 0,
            last_analyzed_epoch_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            columns: HashMap::new(),
        }
    }

    /// Detects statistics staleness. Returns (is_stale, modified_fraction).
    pub fn is_stale(&self) -> (bool, f64) {
        if self.analyzed_rows == 0 {
            return (true, 1.0);
        }
        let ratio = self.modification_counter as f64 / self.analyzed_rows as f64;
        (ratio > 0.20, ratio)
    }

    pub fn record_modification(&mut self) {
        self.modification_counter += 1;
        self.total_rows += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hyperloglog_cardinality() {
        let mut hll = HyperLogLog::new();
        for i in 0..1000 {
            hll.insert(&format!("user_{}", i));
        }
        let est = hll.estimate();
        // HyperLogLog error for m=64 is typically ~13%
        assert!(est >= 800 && est <= 1300, "Estimated {} was out of range", est);
    }

    #[test]
    fn test_typed_histogram_and_staleness() {
        let mut stats = TableStats::new("users");
        stats.total_rows = 1000;
        stats.analyzed_rows = 1000;
        stats.modification_counter = 50;

        let (stale, ratio) = stats.is_stale();
        assert!(!stale);
        assert!((ratio - 0.05).abs() < 1e-4);

        stats.modification_counter = 250;
        let (stale2, ratio2) = stats.is_stale();
        assert!(stale2);
        assert!((ratio2 - 0.25).abs() < 1e-4);
    }
}
