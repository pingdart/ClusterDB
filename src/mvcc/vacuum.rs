use crate::mvcc::{MvccManager, TupleVersion, TxnId, TxnState};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VacuumReport {
    pub tables_scanned: usize,
    pub total_versions_scanned: usize,
    pub dead_versions_pruned: usize,
    pub dead_chains_removed: usize,
    pub bytes_reclaimed: usize,
    pub active_watermark_used: TxnId,
}

#[derive(Debug, Clone)]
pub struct VacuumConfig {
    /// Minimum dead version ratio to trigger autovacuum (e.g. 0.20 = 20%)
    pub dead_tuple_ratio_threshold: f64,
    /// Minimum dead version count to trigger autovacuum
    pub min_dead_tuples_threshold: usize,
}

impl Default for VacuumConfig {
    fn default() -> Self {
        Self {
            dead_tuple_ratio_threshold: 0.20,
            min_dead_tuples_threshold: 100,
        }
    }
}

pub struct VacuumEngine<'a> {
    mvcc: &'a MvccManager,
    config: VacuumConfig,
}

impl<'a> VacuumEngine<'a> {
    pub fn new(mvcc: &'a MvccManager) -> Self {
        Self {
            mvcc,
            config: VacuumConfig::default(),
        }
    }

    pub fn with_config(mvcc: &'a MvccManager, config: VacuumConfig) -> Self {
        Self { mvcc, config }
    }

    /// Computes the oldest active transaction watermark.
    /// Any version deleted/superseded by a transaction committed BEFORE this watermark
    /// can never be seen by any active or future transaction, and is safe to purge.
    pub fn compute_oldest_active_watermark(&self) -> TxnId {
        let states = self.mvcc.txn_states.read().unwrap();
        let mut min_active = self.mvcc.next_txn_id.load(Ordering::SeqCst);

        for (&txn_id, &state) in states.iter() {
            if state == TxnState::Active && txn_id < min_active {
                min_active = txn_id;
            }
        }
        min_active
    }

    /// Vacuums a specific table by pruning dead tuple versions older than the active watermark
    pub fn vacuum_table(&self, target_table: &str) -> Result<VacuumReport> {
        let watermark = self.compute_oldest_active_watermark();
        let states = self.mvcc.txn_states.read().unwrap();
        let mut chains = self.mvcc.version_chains.write().unwrap();

        let mut scanned_tables = 0;
        let mut total_scanned = 0;
        let mut pruned = 0;
        let mut dead_chains = 0;
        let mut bytes_reclaimed = 0;

        let mut keys_to_remove = Vec::new();

        for ((table, key), chain) in chains.iter_mut() {
            if table != target_table {
                continue;
            }
            scanned_tables = 1;
            total_scanned += chain.len();

            let mut retained: Vec<TupleVersion> = Vec::with_capacity(chain.len());

            for version in chain.drain(..) {
                // Check if this version was created by an aborted transaction
                let is_xmin_aborted = match states.get(&version.xmin) {
                    Some(TxnState::Aborted) => true,
                    _ => false,
                };

                if is_xmin_aborted {
                    // Entire version was created by an abort -> discard immediately
                    pruned += 1;
                    bytes_reclaimed += version.data.len();
                    continue;
                }

                // Check if this version was deleted or superseded
                let is_dead = if let Some(xmax) = version.xmax {
                    if let Some(TxnState::Committed(_)) = states.get(&xmax) {
                        // Deleting transaction committed before the oldest active reader
                        xmax < watermark
                    } else if let Some(TxnState::Aborted) = states.get(&xmax) {
                        // Deleting transaction aborted -> version is STILL alive
                        false
                    } else {
                        // Deletion is still active/uncommitted -> version must be preserved
                        false
                    }
                } else {
                    // Never deleted -> alive
                    false
                };

                if is_dead {
                    pruned += 1;
                    bytes_reclaimed += version.data.len();
                } else {
                    retained.push(version);
                }
            }

            if retained.is_empty() {
                keys_to_remove.push((table.clone(), key.clone()));
                dead_chains += 1;
            } else {
                *chain = retained;
            }
        }

        for k in keys_to_remove {
            chains.remove(&k);
        }

        Ok(VacuumReport {
            tables_scanned: scanned_tables,
            total_versions_scanned: total_scanned,
            dead_versions_pruned: pruned,
            dead_chains_removed: dead_chains,
            bytes_reclaimed,
            active_watermark_used: watermark,
        })
    }

    /// Vacuums all tables in the database
    pub fn vacuum_all(&self) -> Result<VacuumReport> {
        let watermark = self.compute_oldest_active_watermark();
        let states = self.mvcc.txn_states.read().unwrap();
        let mut chains = self.mvcc.version_chains.write().unwrap();

        let mut scanned_tables = std::collections::HashSet::new();
        let mut total_scanned = 0;
        let mut pruned = 0;
        let mut dead_chains = 0;
        let mut bytes_reclaimed = 0;

        let mut keys_to_remove = Vec::new();

        for ((table, key), chain) in chains.iter_mut() {
            scanned_tables.insert(table.clone());
            total_scanned += chain.len();

            let mut retained: Vec<TupleVersion> = Vec::with_capacity(chain.len());

            for version in chain.drain(..) {
                let is_xmin_aborted = match states.get(&version.xmin) {
                    Some(TxnState::Aborted) => true,
                    _ => false,
                };

                if is_xmin_aborted {
                    pruned += 1;
                    bytes_reclaimed += version.data.len();
                    continue;
                }

                let is_dead = if let Some(xmax) = version.xmax {
                    if let Some(TxnState::Committed(_)) = states.get(&xmax) {
                        xmax < watermark
                    } else {
                        false
                    }
                } else {
                    false
                };

                if is_dead {
                    pruned += 1;
                    bytes_reclaimed += version.data.len();
                } else {
                    retained.push(version);
                }
            }

            if retained.is_empty() {
                keys_to_remove.push((table.clone(), key.clone()));
                dead_chains += 1;
            } else {
                *chain = retained;
            }
        }

        for k in keys_to_remove {
            chains.remove(&k);
        }

        Ok(VacuumReport {
            tables_scanned: scanned_tables.len(),
            total_versions_scanned: total_scanned,
            dead_versions_pruned: pruned,
            dead_chains_removed: dead_chains,
            bytes_reclaimed,
            active_watermark_used: watermark,
        })
    }

    /// Evaluates whether an autovacuum run is needed based on dead tuple estimates
    pub fn should_autovacuum(&self, total_tuples: usize, dead_tuples: usize) -> bool {
        if dead_tuples < self.config.min_dead_tuples_threshold {
            return false;
        }
        let ratio = dead_tuples as f64 / (total_tuples.max(1) as f64);
        ratio >= self.config.dead_tuple_ratio_threshold
    }
}
