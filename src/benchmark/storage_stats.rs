use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageTelemetry {
    pub total_disk_bytes: u64,
    pub total_disk_mb: f64,
    pub estimated_waf: f64,
    pub total_keys: usize,
}

pub struct StorageStatsCollector;

impl StorageStatsCollector {
    /// Computes recursive directory size in bytes
    pub fn get_dir_size(path: &Path) -> u64 {
        let mut total = 0;
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    total += Self::get_dir_size(&p);
                } else if let Ok(meta) = entry.metadata() {
                    total += meta.len();
                }
            }
        }
        total
    }

    /// Measures storage telemetry for a PingDart database directory
    pub fn measure(db_dir: &str, user_payload_bytes: u64, total_keys: usize) -> StorageTelemetry {
        let path = Path::new(db_dir);
        let disk_bytes = if path.exists() {
            Self::get_dir_size(path)
        } else {
            0
        };

        let estimated_waf = if user_payload_bytes > 0 {
            disk_bytes as f64 / user_payload_bytes as f64
        } else {
            1.0
        };

        StorageTelemetry {
            total_disk_bytes: disk_bytes,
            total_disk_mb: disk_bytes as f64 / (1024.0 * 1024.0),
            estimated_waf,
            total_keys,
        }
    }
}
