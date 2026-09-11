pub mod proto;
pub mod pb_package;

pub use pb_package::{PbPackageManager, ExportReport, ImportReport};

use crate::cdc::log::CDCLog;
use crate::storage::StorageEngine;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotInfo {
    pub snapshot_id: String,
    pub timestamp: u64,
    pub cdc_sequence: u64,
    pub trees: HashMap<String, usize>, // tree_name -> row_count
    pub total_rows: usize,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotData {
    pub info: SnapshotInfo,
    pub tree_data: HashMap<String, Vec<(Vec<u8>, Vec<u8>)>>, // tree_name -> list of (raw_key, raw_bytes)
}

pub struct BackupManager {
    storage: StorageEngine,
    cdc_log: Arc<CDCLog>,
    backup_dir: PathBuf,
}

impl BackupManager {
    pub fn new<P: AsRef<Path>>(storage: StorageEngine, cdc_log: Arc<CDCLog>, backup_dir: P) -> Result<Self> {
        let path = backup_dir.as_ref().to_path_buf();
        fs::create_dir_all(&path).context("Failed to create backup directory")?;
        Ok(Self {
            storage,
            cdc_log,
            backup_dir: path,
        })
    }

    /// Creates a consistent point-in-time snapshot of the database
    pub fn create_snapshot(&self) -> Result<SnapshotInfo> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let snapshot_id = format!("snap_{}", now);
        let cdc_sequence = self.cdc_log.latest_sequence();

        let tree_names = self.storage.tree_names();
        let mut tree_data = HashMap::new();
        let mut trees = HashMap::new();
        let mut total_rows = 0;

        for t_name in tree_names {
            // Exclude internal metadata or cdc logs
            if t_name == "_cdc_log" || t_name == "__sled__default" {
                continue;
            }

            if let Ok(raw_pairs) = self.storage.get_raw_pairs(&t_name) {
                let count = raw_pairs.len();
                trees.insert(t_name.clone(), count);
                total_rows += count;
                tree_data.insert(t_name, raw_pairs);
            }
        }

        let info = SnapshotInfo {
            snapshot_id: snapshot_id.clone(),
            timestamp: now,
            cdc_sequence,
            trees,
            total_rows,
            size_bytes: 0, // calculated upon serialization
        };

        let snapshot = SnapshotData {
            info: info.clone(),
            tree_data,
        };

        let snap_file = self.backup_dir.join(format!("{}.snap", snapshot_id));
        let encoded = bincode::serialize(&snapshot)?;
        fs::write(&snap_file, &encoded)?;

        let mut final_info = info;
        final_info.size_bytes = encoded.len() as u64;

        // Persist snapshot catalog in backup directory
        let catalog_file = self.backup_dir.join("catalog.json");
        let mut catalog = self.list_snapshots().unwrap_or_default();
        catalog.push(final_info.clone());
        if let Ok(cat_bytes) = serde_json::to_vec_pretty(&catalog) {
            let _ = fs::write(&catalog_file, cat_bytes);
        }

        Ok(final_info)
    }

    pub fn list_snapshots(&self) -> Result<Vec<SnapshotInfo>> {
        let catalog_file = self.backup_dir.join("catalog.json");
        if catalog_file.exists() {
            if let Ok(bytes) = fs::read(&catalog_file) {
                if let Ok(cat) = serde_json::from_slice::<Vec<SnapshotInfo>>(&bytes) {
                    return Ok(cat);
                }
            }
        }

        // Resilient fallback: scan .snap files directly
        let mut list = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.backup_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("snap") {
                    if let Ok(bytes) = fs::read(&p) {
                        if let Ok(snap) = bincode::deserialize::<SnapshotData>(&bytes) {
                            list.push(snap.info);
                        }
                    }
                }
            }
        }
        list.sort_by_key(|s| s.timestamp);
        Ok(list)
    }

    /// Restores the entire database from a snapshot
    pub fn restore_snapshot(&self, snapshot_id: &str) -> Result<SnapshotInfo> {
        let snap_file = self.backup_dir.join(format!("{}.snap", snapshot_id));
        if !snap_file.exists() {
            return Err(anyhow!("Snapshot file '{}' not found", snap_file.display()));
        }

        let bytes = fs::read(&snap_file)?;
        let snapshot: SnapshotData = bincode::deserialize(&bytes)?;

        // Restore trees
        for (tree_name, records) in snapshot.tree_data {
            let _ = self.storage.drop_tree(&tree_name);
            for (key, val_bytes) in records {
                self.storage.insert_raw(&tree_name, &key, &val_bytes)?;
            }
        }
        self.storage.flush()?;

        Ok(snapshot.info)
    }

    /// Point-In-Time Recovery (PITR): Restores base snapshot and replays CDC logs up to target_timestamp
    pub fn point_in_time_recovery(&self, target_timestamp: u64) -> Result<usize> {
        let snapshots = self.list_snapshots()?;
        // Find latest snapshot created at or before target_timestamp
        let base_snap = snapshots
            .into_iter()
            .filter(|s| s.timestamp <= target_timestamp)
            .max_by_key(|s| s.timestamp)
            .ok_or_else(|| anyhow!("No snapshot found prior to timestamp {}", target_timestamp))?;

        // 1. Restore base snapshot
        self.restore_snapshot(&base_snap.snapshot_id)?;

        // 2. Replay CDC commit logs between base_snap.cdc_sequence and target_timestamp
        let cdc_records = self.cdc_log.replay(base_snap.cdc_sequence, 100_000)?;
        let mut replayed_count = 0;
        let target_ms = if target_timestamp < 10_000_000_000 {
            (target_timestamp * 1000) as i64
        } else {
            target_timestamp as i64
        };

        for record in cdc_records {
            if record.timestamp_ms > target_ms {
                break;
            }

            let db_name = if record.database.is_empty() { "default" } else { &record.database };
            let full_table_dot = format!("{}.{}", db_name, record.table);
            let full_table_underscore = format!("{}_{}", db_name, record.table);
            let bare_table = record.table.clone();

            match record.operation.as_str() {
                "INSERT" | "UPDATE" => {
                    if let Some(after_val) = record.after_state_json() {
                        if let Ok(cols) = serde_json::from_value::<HashMap<String, String>>(after_val.clone()) {
                            let row = crate::exec::RowData {
                                _id: record.primary_key.clone(),
                                columns: cols,
                                ..Default::default()
                            };
                            let _ = self.storage.set(&full_table_underscore, &record.primary_key, &row);
                            let _ = self.storage.set(&full_table_dot, &record.primary_key, &row);
                            let _ = self.storage.set(&bare_table, &record.primary_key, &row);
                            replayed_count += 1;
                        } else if let Ok(s) = serde_json::from_value::<String>(after_val) {
                            let _ = self.storage.set(&full_table_underscore, &record.primary_key, &s);
                            let _ = self.storage.set(&full_table_dot, &record.primary_key, &s);
                            let _ = self.storage.set(&bare_table, &record.primary_key, &s);
                            replayed_count += 1;
                        }
                    }
                }
                "DELETE" => {
                    let _ = self.storage.delete(&full_table_underscore, &record.primary_key);
                    let _ = self.storage.delete(&full_table_dot, &record.primary_key);
                    let _ = self.storage.delete(&bare_table, &record.primary_key);
                    replayed_count += 1;
                }
                _ => {}
            }
        }
        self.storage.flush()?;

        Ok(replayed_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_and_pitr() {
        let storage = StorageEngine::new_temp().unwrap();
        let cdc_storage = StorageEngine::new_temp().unwrap();
        let cdc = Arc::new(CDCLog::new(cdc_storage).unwrap());
        let backup_dir = "test_backups_dir";
        let manager = BackupManager::new(storage.clone(), cdc.clone(), backup_dir).unwrap();

        let mut row1 = HashMap::new();
        row1.insert("name".to_string(), "Alice".to_string());
        let mut row2 = HashMap::new();
        row2.insert("name".to_string(), "Bob".to_string());
        let mut row3 = HashMap::new();
        row3.insert("name".to_string(), "Charlie".to_string());

        let r1 = crate::exec::RowData { _id: "1".into(), columns: row1, ..Default::default() };
        let r2 = crate::exec::RowData { _id: "2".into(), columns: row2, ..Default::default() };
        let r3 = crate::exec::RowData { _id: "3".into(), columns: row3.clone(), ..Default::default() };

        // 1. Seed table
        storage.set("users", "1", &r1).unwrap();
        storage.set("users", "2", &r2).unwrap();
        storage.flush().unwrap();

        // 2. Snapshot
        let snap = manager.create_snapshot().unwrap();
        assert_eq!(snap.trees.get("users"), Some(&2));

        // 3. Mutate data & record CDC
        storage.set("users", "3", &r3).unwrap();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        cdc.append("default", "users", "INSERT", "3", None, Some(serde_json::to_value(&row3).unwrap())).unwrap();

        // Verify Charlie is present
        assert!(storage.get::<crate::exec::RowData>("users", "3").unwrap().is_some());

        // 4. Restore base snapshot -> Charlie should disappear
        manager.restore_snapshot(&snap.snapshot_id).unwrap();
        assert!(storage.get::<crate::exec::RowData>("users", "3").unwrap().is_none());
        assert!(storage.get::<crate::exec::RowData>("users", "1").unwrap().is_some());

        // 5. Point in time recovery up to current timestamp -> Charlie should reappear via CDC replay!
        let replayed = manager.point_in_time_recovery(now + 10).unwrap();
        assert_eq!(replayed, 1);
        assert!(storage.get::<crate::exec::RowData>("users", "3").unwrap().is_some());

        let _ = std::fs::remove_dir_all(backup_dir);
    }
}
