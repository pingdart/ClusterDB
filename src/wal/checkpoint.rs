use crate::wal::{Lsn, TxnId, WalManager, WalPayload, WalRecord};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMetadata {
    pub checkpoint_lsn: Lsn,
    pub timestamp_ns: u64,
    pub active_txns: Vec<TxnId>,
    pub min_active_lsn: Lsn,
}

#[derive(Debug)]
pub struct CheckpointManager {
    wal: Arc<WalManager>,
    meta_path: PathBuf,
    last_checkpoint_lsn: AtomicU64,
}

impl CheckpointManager {
    pub fn new<P: AsRef<Path>>(wal: Arc<WalManager>, db_dir: P) -> Self {
        let meta_path = db_dir.as_ref().join("checkpoint.meta");
        let initial_lsn = if meta_path.exists() {
            if let Ok(bytes) = std::fs::read(&meta_path) {
                bincode::deserialize::<CheckpointMetadata>(&bytes)
                    .map(|m| m.checkpoint_lsn)
                    .unwrap_or(0)
            } else {
                0
            }
        } else {
            0
        };

        Self {
            wal,
            meta_path,
            last_checkpoint_lsn: AtomicU64::new(initial_lsn),
        }
    }

    /// Creates a consistent checkpoint:
    /// 1. Flushes in-flight WAL buffers to disk via physical fsync.
    /// 2. Writes a Checkpoint WAL record containing active transactions.
    /// 3. Flushes the checkpoint record to disk.
    /// 4. Updates and persists the checkpoint metadata file.
    pub fn create_checkpoint(&self, active_txns: Vec<TxnId>, min_active_lsn: Lsn) -> Result<CheckpointMetadata> {
        // Step 1: Ensure all pending logs before checkpoint are durable
        self.wal.flush()?;

        // Step 2: Write Checkpoint record into WAL
        let cp_lsn = self.wal.append(
            0, // System Txn ID
            WalPayload::Checkpoint {
                active_txns: active_txns.clone(),
            },
        )?;

        // Step 3: Flush the checkpoint record
        self.wal.flush()?;

        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let meta = CheckpointMetadata {
            checkpoint_lsn: cp_lsn,
            timestamp_ns: now_ns,
            active_txns,
            min_active_lsn,
        };

        // Step 4: Atomically persist checkpoint metadata
        let encoded = bincode::serialize(&meta)?;
        let tmp_meta = self.meta_path.with_extension("meta.tmp");
        std::fs::write(&tmp_meta, encoded)?;
        std::fs::rename(tmp_meta, &self.meta_path)?;

        self.last_checkpoint_lsn.store(cp_lsn, Ordering::SeqCst);
        Ok(meta)
    }

    pub fn last_checkpoint_lsn(&self) -> Lsn {
        self.last_checkpoint_lsn.load(Ordering::SeqCst)
    }

    pub fn load_latest_metadata(&self) -> Option<CheckpointMetadata> {
        if self.meta_path.exists() {
            if let Ok(bytes) = std::fs::read(&self.meta_path) {
                return bincode::deserialize::<CheckpointMetadata>(&bytes).ok();
            }
        }
        None
    }

    /// Truncates the WAL by pruning records prior to `safe_lsn`.
    /// `safe_lsn` is typically `min(checkpoint_lsn, min_active_txn_lsn)`.
    pub fn truncate_wal(&self, safe_lsn: Lsn) -> Result<usize> {
        let all_records = self.wal.read_all_records()?;
        let records_to_keep: Vec<WalRecord> = all_records
            .into_iter()
            .filter(|r| r.lsn >= safe_lsn)
            .collect();

        let pruned_count = self.wal.current_lsn() as usize - records_to_keep.len();

        // Rewrite WAL file cleanly with remaining records
        let mut file = self.wal.file_mutex.lock().unwrap();
        file.seek(SeekFrom::Start(0))?;
        file.set_len(0)?;

        for r in &records_to_keep {
            let encoded = bincode::serialize(r)?;
            let len = (encoded.len() as u32).to_le_bytes();
            file.write_all(&len)?;
            file.write_all(&encoded)?;
        }
        file.sync_data()?;

        Ok(pruned_count)
    }
}
