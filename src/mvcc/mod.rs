use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

pub mod vacuum;
use vacuum::{VacuumEngine, VacuumReport};

pub type TxnId = u64;
pub type Timestamp = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxnState {
    Active,
    Committed(Timestamp),
    Aborted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TupleVersion {
    pub xmin: TxnId,
    pub xmax: Option<TxnId>,
    pub cid: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadView {
    pub reader_txn_id: TxnId,
    pub active_txns_at_start: HashSet<TxnId>,
    pub high_watermark_txn_id: TxnId,
}

impl ReadView {
    /// Determines whether a tuple version is visible to this snapshot
    pub fn is_visible(
        &self,
        tuple: &TupleVersion,
        txn_states: &HashMap<TxnId, TxnState>,
    ) -> bool {
        // 1. Did the current transaction create this version?
        if tuple.xmin == self.reader_txn_id {
            // Visible unless deleted by this transaction
            if let Some(xmax) = tuple.xmax {
                if xmax == self.reader_txn_id {
                    return false; // Deleted by self
                }
            }
            return true;
        }

        // 2. Was xmin created after this snapshot was taken?
        if tuple.xmin > self.high_watermark_txn_id {
            return false;
        }

        // 3. Was xmin active (uncommitted) when this snapshot was taken?
        if self.active_txns_at_start.contains(&tuple.xmin) {
            return false;
        }

        // 4. Is xmin actually committed?
        match txn_states.get(&tuple.xmin) {
            Some(TxnState::Committed(_)) => {}
            _ => return false, // Uncommitted or aborted
        }

        // 5. Check xmax (deletion/update)
        if let Some(xmax) = tuple.xmax {
            // Deleted by self?
            if xmax == self.reader_txn_id {
                return false;
            }

            // Deleted after snapshot watermark?
            if xmax > self.high_watermark_txn_id {
                return true; // Still alive in our view
            }

            // Was deleting transaction in-flight when snapshot was taken?
            if self.active_txns_at_start.contains(&xmax) {
                return true; // Deletion not yet committed at our snapshot time
            }

            // Check if deleting transaction committed
            match txn_states.get(&xmax) {
                Some(TxnState::Committed(_)) => false, // Deleted and committed before snapshot
                _ => true,                             // Deletion aborted or uncommitted
            }
        } else {
            true // Tuple was never deleted
        }
    }
}

#[derive(Debug)]
pub struct MvccManager {
    next_txn_id: AtomicU64,
    next_timestamp: AtomicU64,
    txn_states: RwLock<HashMap<TxnId, TxnState>>,
    /// Key-value version chains: (table, key) -> Vec<TupleVersion>
    version_chains: RwLock<HashMap<(String, String), Vec<TupleVersion>>>,
}

impl MvccManager {
    pub fn new() -> Self {
        Self {
            next_txn_id: AtomicU64::new(1),
            next_timestamp: AtomicU64::new(1),
            txn_states: RwLock::new(HashMap::new()),
            version_chains: RwLock::new(HashMap::new()),
        }
    }

    /// Begins a new transaction and captures an immutable Snapshot Read View
    pub fn begin_transaction(&self) -> (TxnId, ReadView) {
        let txn_id = self.next_txn_id.fetch_add(1, Ordering::SeqCst);
        let mut states = self.txn_states.write().unwrap();
        states.insert(txn_id, TxnState::Active);

        let active_set: HashSet<TxnId> = states
            .iter()
            .filter_map(|(&id, &state)| {
                if id != txn_id && state == TxnState::Active {
                    Some(id)
                } else {
                    None
                }
            })
            .collect();

        let read_view = ReadView {
            reader_txn_id: txn_id,
            active_txns_at_start: active_set,
            high_watermark_txn_id: txn_id,
        };

        (txn_id, read_view)
    }

    /// Commits an active transaction
    pub fn commit_transaction(&self, txn_id: TxnId) -> Result<()> {
        let ts = self.next_timestamp.fetch_add(1, Ordering::SeqCst);
        let mut states = self.txn_states.write().unwrap();
        if let Some(state) = states.get_mut(&txn_id) {
            *state = TxnState::Committed(ts);
            Ok(())
        } else {
            bail!("Transaction {} not found", txn_id);
        }
    }

    /// Aborts an active transaction and marks its versions invalid
    pub fn abort_transaction(&self, txn_id: TxnId) -> Result<()> {
        let mut states = self.txn_states.write().unwrap();
        if let Some(state) = states.get_mut(&txn_id) {
            *state = TxnState::Aborted;
            Ok(())
        } else {
            bail!("Transaction {} not found", txn_id);
        }
    }

    /// Inserts a new tuple version
    pub fn insert(
        &self,
        txn_id: TxnId,
        table: &str,
        key: &str,
        data: Vec<u8>,
        cid: u32,
    ) -> Result<()> {
        let mut chains = self.version_chains.write().unwrap();
        let chain = chains
            .entry((table.to_string(), key.to_string()))
            .or_default();

        chain.push(TupleVersion {
            xmin: txn_id,
            xmax: None,
            cid,
            data,
        });

        Ok(())
    }

    /// Updates a tuple version by setting xmax on the visible version and appending the new version
    pub fn update(
        &self,
        txn_id: TxnId,
        table: &str,
        key: &str,
        data: Vec<u8>,
        cid: u32,
        read_view: &ReadView,
    ) -> Result<()> {
        let states = self.txn_states.read().unwrap();
        let mut chains = self.version_chains.write().unwrap();
        let chain = chains
            .get_mut(&(table.to_string(), key.to_string()))
            .context("Cannot update non-existent key")?;

        // Locate current visible version and set xmax
        let mut found = false;
        for version in chain.iter_mut().rev() {
            if read_view.is_visible(version, &states) {
                if version.xmax.is_some() {
                    bail!("Write-write conflict: row is already modified or deleted");
                }
                version.xmax = Some(txn_id);
                found = true;
                break;
            }
        }

        if !found {
            bail!("Tuple not found for update under current read view");
        }

        chain.push(TupleVersion {
            xmin: txn_id,
            xmax: None,
            cid,
            data,
        });

        Ok(())
    }

    /// Deletes a tuple version by stamping xmax
    pub fn delete(
        &self,
        txn_id: TxnId,
        table: &str,
        key: &str,
        read_view: &ReadView,
    ) -> Result<()> {
        let states = self.txn_states.read().unwrap();
        let mut chains = self.version_chains.write().unwrap();
        let chain = chains
            .get_mut(&(table.to_string(), key.to_string()))
            .context("Cannot delete non-existent key")?;

        let mut found = false;
        for version in chain.iter_mut().rev() {
            if read_view.is_visible(version, &states) {
                if version.xmax.is_some() {
                    bail!("Write-write conflict: row is already deleted");
                }
                version.xmax = Some(txn_id);
                found = true;
                break;
            }
        }

        if !found {
            bail!("Tuple not found for delete under current read view");
        }

        Ok(())
    }

    /// Reads the tuple version visible to the snapshot
    pub fn read(
        &self,
        table: &str,
        key: &str,
        read_view: &ReadView,
    ) -> Option<Vec<u8>> {
        let states = self.txn_states.read().unwrap();
        let chains = self.version_chains.read().unwrap();
        let chain = chains.get(&(table.to_string(), key.to_string()))?;

        // Traverse backwards in version chain to find newest visible version
        for version in chain.iter().rev() {
            if read_view.is_visible(version, &states) {
                return Some(version.data.clone());
            }
        }

        None
    }

    /// Vacuums dead tuple versions across all tables
    pub fn vacuum(&self) -> Result<VacuumReport> {
        let engine = VacuumEngine::new(self);
        engine.vacuum_all()
    }

    /// Vacuums dead tuple versions for a single table
    pub fn vacuum_table(&self, table: &str) -> Result<VacuumReport> {
        let engine = VacuumEngine::new(self);
        engine.vacuum_table(table)
    }

    /// Returns the total number of versions currently held across all chains
    pub fn total_version_count(&self) -> usize {
        let chains = self.version_chains.read().unwrap();
        chains.values().map(|c| c.len()).sum()
    }
}

impl Default for MvccManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_mvcc_non_blocking_snapshot_isolation() {
        let mvcc = Arc::new(MvccManager::new());

        // Txn 1 inserts key1 and commits
        let (tx1, _rv1) = mvcc.begin_transaction();
        mvcc.insert(tx1, "users", "k1", b"Alice".to_vec(), 0)
            .unwrap();
        mvcc.commit_transaction(tx1).unwrap();

        // Txn 2 starts and takes Snapshot A
        let (_tx2, rv2) = mvcc.begin_transaction();

        // Verify Txn 2 sees Alice
        let val = mvcc.read("users", "k1", &rv2);
        assert_eq!(val, Some(b"Alice".to_vec()));

        // Concurrent Txn 3 updates k1 to Bob and commits
        let (tx3, rv3) = mvcc.begin_transaction();
        mvcc.update(tx3, "users", "k1", b"Bob".to_vec(), 0, &rv3)
            .unwrap();
        mvcc.commit_transaction(tx3).unwrap();

        // Snapshot Isolation: Txn 2 STILL sees Alice (non-blocking read consistency)
        let val_tx2 = mvcc.read("users", "k1", &rv2);
        assert_eq!(val_tx2, Some(b"Alice".to_vec()));

        // Txn 4 starts AFTER Txn 3 commits -> sees Bob
        let (_tx4, rv4) = mvcc.begin_transaction();
        let val_tx4 = mvcc.read("users", "k1", &rv4);
        assert_eq!(val_tx4, Some(b"Bob".to_vec()));
    }

    #[test]
    fn test_mvcc_aborted_transaction_rollback() {
        let mvcc = Arc::new(MvccManager::new());

        // Txn 1 inserts Charlie and aborts
        let (tx1, _rv1) = mvcc.begin_transaction();
        mvcc.insert(tx1, "users", "k2", b"Charlie".to_vec(), 0)
            .unwrap();
        mvcc.abort_transaction(tx1).unwrap();

        // Txn 2 starts -> Charlie must not be visible
        let (_tx2, rv2) = mvcc.begin_transaction();
        let val = mvcc.read("users", "k2", &rv2);
        assert_eq!(val, None);
    }

    #[test]
    fn test_mvcc_vacuum_garbage_collection() {
        let mvcc = Arc::new(MvccManager::new());

        // Step 1: Insert row
        let (tx1, _rv1) = mvcc.begin_transaction();
        mvcc.insert(tx1, "accounts", "acc_1", b"Balance: 100".to_vec(), 0).unwrap();
        mvcc.commit_transaction(tx1).unwrap();

        // Step 2: Update row 3 times in successive committed txns
        let (tx2, rv2) = mvcc.begin_transaction();
        mvcc.update(tx2, "accounts", "acc_1", b"Balance: 200".to_vec(), 0, &rv2).unwrap();
        mvcc.commit_transaction(tx2).unwrap();

        let (tx3, rv3) = mvcc.begin_transaction();
        mvcc.update(tx3, "accounts", "acc_1", b"Balance: 300".to_vec(), 0, &rv3).unwrap();
        mvcc.commit_transaction(tx3).unwrap();

        let (tx4, rv4) = mvcc.begin_transaction();
        mvcc.update(tx4, "accounts", "acc_1", b"Balance: 400".to_vec(), 0, &rv4).unwrap();
        mvcc.commit_transaction(tx4).unwrap();

        // Before vacuum: 4 versions exist in the chain
        assert_eq!(mvcc.total_version_count(), 4);

        // Run vacuum: because no long-running transactions are active, older superseded versions are dead
        let report = mvcc.vacuum_table("accounts").unwrap();
        assert_eq!(report.dead_versions_pruned, 3);
        assert_eq!(mvcc.total_version_count(), 1);

        // Verify latest version is intact and readable
        let (_tx5, rv5) = mvcc.begin_transaction();
        let val = mvcc.read("accounts", "acc_1", &rv5);
        assert_eq!(val, Some(b"Balance: 400".to_vec()));
    }
}
