use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub type Lsn = u64;
pub type TxnId = u64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WalPayload {
    Begin,
    Commit,
    Abort,
    Insert {
        table: String,
        key: String,
        data: Vec<u8>,
    },
    Update {
        table: String,
        key: String,
        old_data: Option<Vec<u8>>,
        new_data: Vec<u8>,
    },
    Delete {
        table: String,
        key: String,
        old_data: Option<Vec<u8>>,
    },
    Checkpoint {
        active_txns: Vec<TxnId>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WalRecord {
    pub lsn: Lsn,
    pub prev_lsn: Lsn,
    pub txn_id: TxnId,
    pub timestamp_ns: u64,
    pub payload: WalPayload,
    pub checksum: [u8; 32],
}

impl WalRecord {
    pub fn new(lsn: Lsn, prev_lsn: Lsn, txn_id: TxnId, payload: WalPayload) -> Self {
        let timestamp_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let mut record = Self {
            lsn,
            prev_lsn,
            txn_id,
            timestamp_ns,
            payload,
            checksum: [0u8; 32],
        };
        record.checksum = record.compute_checksum();
        record
    }

    pub fn compute_checksum(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.lsn.to_le_bytes());
        hasher.update(self.prev_lsn.to_le_bytes());
        hasher.update(self.txn_id.to_le_bytes());
        hasher.update(self.timestamp_ns.to_le_bytes());
        if let Ok(encoded_payload) = bincode::serialize(&self.payload) {
            hasher.update(&encoded_payload);
        }
        let result = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&result);
        out
    }

    pub fn verify_integrity(&self) -> bool {
        self.checksum == self.compute_checksum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    ImmediateFsync,
    GroupCommit { max_buffered_records: usize },
}

pub mod checkpoint;
pub use checkpoint::{CheckpointManager, CheckpointMetadata};

#[derive(Debug)]
pub struct WalManager {
    #[allow(dead_code)]
    wal_path: PathBuf,
    current_lsn: AtomicU64,
    last_flushed_lsn: AtomicU64,
    txn_prev_lsn: Mutex<HashMap<TxnId, Lsn>>,
    pub(crate) file_mutex: Mutex<File>,
    sync_mode: SyncMode,
    pending_records_count: AtomicU64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RecoveryReport {
    pub total_records_scanned: usize,
    pub committed_txns: Vec<TxnId>,
    pub aborted_txns: Vec<TxnId>,
    pub rolled_back_txns: Vec<TxnId>,
    pub redone_records: usize,
    pub undone_records: usize,
    pub max_lsn: Lsn,
}

impl WalManager {
    pub fn open<P: AsRef<Path>>(wal_dir: P, sync_mode: SyncMode) -> Result<Arc<Self>> {
        let wal_dir = wal_dir.as_ref();
        std::fs::create_dir_all(wal_dir)?;
        let wal_path = wal_dir.join("pingdart.wal");

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&wal_path)
            .with_context(|| format!("Failed to open WAL file at {:?}", wal_path))?;

        let mgr = Self {
            wal_path,
            current_lsn: AtomicU64::new(0),
            last_flushed_lsn: AtomicU64::new(0),
            txn_prev_lsn: Mutex::new(HashMap::new()),
            file_mutex: Mutex::new(file),
            sync_mode,
            pending_records_count: AtomicU64::new(0),
        };

        // Scan existing records to locate the highest LSN
        let records = mgr.read_all_records()?;
        if let Some(last) = records.last() {
            mgr.current_lsn.store(last.lsn, Ordering::SeqCst);
            mgr.last_flushed_lsn.store(last.lsn, Ordering::SeqCst);
        }

        Ok(Arc::new(mgr))
    }

    /// Appends a new WAL record, returning its assigned monotonic LSN.
    pub fn append(&self, txn_id: TxnId, payload: WalPayload) -> Result<Lsn> {
        let lsn = self.current_lsn.fetch_add(1, Ordering::SeqCst) + 1;

        let prev_lsn = {
            let mut map = self.txn_prev_lsn.lock().unwrap();
            let prev = map.get(&txn_id).copied().unwrap_or(0);
            map.insert(txn_id, lsn);
            prev
        };

        let record = WalRecord::new(lsn, prev_lsn, txn_id, payload);
        let encoded = bincode::serialize(&record)?;
        let len = (encoded.len() as u32).to_le_bytes();

        let mut file = self.file_mutex.lock().unwrap();
        file.seek(SeekFrom::End(0))?;
        file.write_all(&len)?;
        file.write_all(&encoded)?;

        match self.sync_mode {
            SyncMode::ImmediateFsync => {
                file.sync_data()?;
                self.last_flushed_lsn.store(lsn, Ordering::SeqCst);
            }
            SyncMode::GroupCommit { max_buffered_records } => {
                let pending = self.pending_records_count.fetch_add(1, Ordering::SeqCst) + 1;
                if pending as usize >= max_buffered_records {
                    file.sync_data()?;
                    self.pending_records_count.store(0, Ordering::SeqCst);
                    self.last_flushed_lsn.store(lsn, Ordering::SeqCst);
                }
            }
        }

        Ok(lsn)
    }

    /// Forces a physical fsync barrier to guarantee durability up to current LSN
    pub fn flush(&self) -> Result<()> {
        let file = self.file_mutex.lock().unwrap();
        file.sync_data()?;
        let curr = self.current_lsn.load(Ordering::SeqCst);
        self.last_flushed_lsn.store(curr, Ordering::SeqCst);
        self.pending_records_count.store(0, Ordering::SeqCst);
        Ok(())
    }

    /// Reads all records from the log sequentially, verifying cryptographic integrity
    pub fn read_all_records(&self) -> Result<Vec<WalRecord>> {
        let mut file = self.file_mutex.lock().unwrap();
        file.seek(SeekFrom::Start(0))?;

        let mut records = Vec::new();
        let mut len_buf = [0u8; 4];

        while file.read_exact(&mut len_buf).is_ok() {
            let len = u32::from_le_bytes(len_buf) as usize;
            let mut record_buf = vec![0u8; len];
            file.read_exact(&mut record_buf)
                .context("Premature EOF reading WAL record body")?;

            let record: WalRecord = bincode::deserialize(&record_buf)
                .context("Corrupt binary WAL record encountered")?;

            if !record.verify_integrity() {
                bail!("WAL record checksum mismatch at LSN {}", record.lsn);
            }

            records.push(record);
        }

        Ok(records)
    }

    /// Executes the 3-phase ARIES recovery algorithm (Analysis, Redo, Undo)
    pub fn recover<FRedo, FUndo>(
        &self,
        mut redo_fn: FRedo,
        mut undo_fn: FUndo,
    ) -> Result<RecoveryReport>
    where
        FRedo: FnMut(&WalRecord) -> Result<()>,
        FUndo: FnMut(&WalRecord) -> Result<()>,
    {
        let records = self.read_all_records()?;
        let mut report = RecoveryReport {
            total_records_scanned: records.len(),
            ..Default::default()
        };

        if records.is_empty() {
            return Ok(report);
        }

        // ==========================================
        // Phase 1: Analysis Pass
        // ==========================================
        let mut active_txns = HashSet::new();
        let mut committed_txns = HashSet::new();
        let mut aborted_txns = HashSet::new();

        for record in &records {
            if record.lsn > report.max_lsn {
                report.max_lsn = record.lsn;
            }
            match &record.payload {
                WalPayload::Begin => {
                    active_txns.insert(record.txn_id);
                }
                WalPayload::Commit => {
                    active_txns.remove(&record.txn_id);
                    committed_txns.insert(record.txn_id);
                }
                WalPayload::Abort => {
                    active_txns.remove(&record.txn_id);
                    aborted_txns.insert(record.txn_id);
                }
                WalPayload::Checkpoint {
                    active_txns: cp_txns,
                } => {
                    for tid in cp_txns {
                        active_txns.insert(*tid);
                    }
                }
                _ => {
                    active_txns.insert(record.txn_id);
                }
            }
        }

        report.committed_txns = committed_txns.into_iter().collect();
        report.aborted_txns = aborted_txns.into_iter().collect();
        let uncommitted_to_undo: HashSet<TxnId> = active_txns;
        report.rolled_back_txns = uncommitted_to_undo.iter().copied().collect();

        // ==========================================
        // Phase 2: Redo Pass (Repeating History)
        // ==========================================
        for record in &records {
            match &record.payload {
                WalPayload::Insert { .. }
                | WalPayload::Update { .. }
                | WalPayload::Delete { .. } => {
                    // Only redo updates of transactions that ultimately committed
                    if report.committed_txns.contains(&record.txn_id) {
                        redo_fn(record)?;
                        report.redone_records += 1;
                    }
                }
                _ => {}
            }
        }

        // ==========================================
        // Phase 3: Undo Pass (Rolling Back Losers)
        // ==========================================
        // Traverse backwards in time to roll back uncommitted transactions
        for record in records.iter().rev() {
            if uncommitted_to_undo.contains(&record.txn_id) {
                match &record.payload {
                    WalPayload::Insert { .. }
                    | WalPayload::Update { .. }
                    | WalPayload::Delete { .. } => {
                        undo_fn(record)?;
                        report.undone_records += 1;
                    }
                    _ => {}
                }
            }
        }

        Ok(report)
    }

    pub fn current_lsn(&self) -> Lsn {
        self.current_lsn.load(Ordering::SeqCst)
    }

    pub fn last_flushed_lsn(&self) -> Lsn {
        self.last_flushed_lsn.load(Ordering::SeqCst)
    }

    pub fn path(&self) -> &Path {
        &self.wal_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wal_append_and_integrity_check() {
        let dir = std::env::temp_dir().join(format!("wal_test_1_{}", uuid::Uuid::new_v4()));
        let wal = WalManager::open(&dir, SyncMode::ImmediateFsync).unwrap();

        let lsn1 = wal
            .append(101, WalPayload::Begin)
            .unwrap();
        let lsn2 = wal
            .append(
                101,
                WalPayload::Insert {
                    table: "users".into(),
                    key: "user_1".into(),
                    data: b"Alice".to_vec(),
                },
            )
            .unwrap();
        let lsn3 = wal
            .append(101, WalPayload::Commit)
            .unwrap();

        assert_eq!(lsn1, 1);
        assert_eq!(lsn2, 2);
        assert_eq!(lsn3, 3);
        assert_eq!(wal.current_lsn(), 3);
        assert_eq!(wal.last_flushed_lsn(), 3);

        let records = wal.read_all_records().unwrap();
        assert_eq!(records.len(), 3);
        assert!(records[0].verify_integrity());
        assert!(records[1].verify_integrity());
        assert!(records[2].verify_integrity());
    }

    #[test]
    fn test_wal_aries_crash_recovery_cycle() {
        let dir = std::env::temp_dir().join(format!("wal_test_2_{}", uuid::Uuid::new_v4()));
        let wal = WalManager::open(&dir, SyncMode::ImmediateFsync).unwrap();

        // Transaction 1: Commits
        wal.append(1, WalPayload::Begin).unwrap();
        wal.append(
            1,
            WalPayload::Insert {
                table: "accounts".into(),
                key: "acc_1".into(),
                data: b"Balance: 100".to_vec(),
            },
        )
        .unwrap();
        wal.append(1, WalPayload::Commit).unwrap();

        // Transaction 2: Crashes without commit (Loser transaction)
        wal.append(2, WalPayload::Begin).unwrap();
        wal.append(
            2,
            WalPayload::Insert {
                table: "accounts".into(),
                key: "acc_2".into(),
                data: b"Balance: 500".to_vec(),
            },
        )
        .unwrap();
        wal.flush().unwrap();

        // Simulate crash and recovery
        let mut redone_keys = Vec::new();
        let mut undone_keys = Vec::new();

        let report = wal
            .recover(
                |rec| {
                    if let WalPayload::Insert { key, .. } = &rec.payload {
                        redone_keys.push(key.clone());
                    }
                    Ok(())
                },
                |rec| {
                    if let WalPayload::Insert { key, .. } = &rec.payload {
                        undone_keys.push(key.clone());
                    }
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(report.total_records_scanned, 5);
        assert_eq!(report.redone_records, 1);
        assert_eq!(report.undone_records, 1);
        assert_eq!(redone_keys, vec!["acc_1"]);
        assert_eq!(undone_keys, vec!["acc_2"]);
        assert!(report.rolled_back_txns.contains(&2));
    }

    #[test]
    fn test_wal_checkpoint_and_truncation() {
        let temp_dir = std::env::temp_dir().join(format!("pingdart_cp_test_{}", rand::random::<u64>()));
        let wal = WalManager::open(&temp_dir, SyncMode::ImmediateFsync).unwrap();
        let cp_mgr = CheckpointManager::new(wal.clone(), &temp_dir);

        // Append initial committed transaction
        wal.append(1, WalPayload::Begin).unwrap();
        wal.append(
            1,
            WalPayload::Insert {
                table: "users".into(),
                key: "u1".into(),
                data: b"Alice".to_vec(),
            },
        ).unwrap();
        wal.append(1, WalPayload::Commit).unwrap();

        // Checkpoint at this state: no active txns
        let cp = cp_mgr.create_checkpoint(vec![], 4).unwrap();
        assert_eq!(cp.active_txns.len(), 0);
        assert_eq!(cp_mgr.last_checkpoint_lsn(), 4);

        // Append subsequent records
        wal.append(2, WalPayload::Begin).unwrap(); // LSN 5
        wal.append(
            2,
            WalPayload::Insert {
                table: "users".into(),
                key: "u2".into(),
                data: b"Bob".to_vec(),
            },
        ).unwrap(); // LSN 6

        // Before truncation: 6 records exist
        let all_records = wal.read_all_records().unwrap();
        assert_eq!(all_records.len(), 6);

        // Truncate records prior to LSN 4 (safe LSN)
        let pruned = cp_mgr.truncate_wal(4).unwrap();
        assert_eq!(pruned, 3); // Records 1, 2, 3 pruned

        let remaining = wal.read_all_records().unwrap();
        assert_eq!(remaining.len(), 3);
        assert_eq!(remaining[0].lsn, 4); // Checkpoint record is first remaining
        assert_eq!(remaining[1].lsn, 5);
        assert_eq!(remaining[2].lsn, 6);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
