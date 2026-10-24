use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

pub type ShardId = u32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TwoPhaseTxnState {
    Initiated,
    Preparing,
    Prepared,
    Committing,
    Committed(u64), // Commit timestamp (HLC)
    Aborting(String), // Reason
    Aborted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShardMutation {
    Insert { table: String, key: String, data: Vec<u8> },
    Update { table: String, key: String, data: Vec<u8> },
    Delete { table: String, key: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardVote {
    Yes,
    No(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedTxnRecord {
    pub txn_id: String,
    pub coordinator_id: String,
    pub state: TwoPhaseTxnState,
    pub participants: Vec<ShardId>,
    pub mutations: HashMap<ShardId, Vec<ShardMutation>>,
    pub commit_timestamp: Option<u64>,
}

#[derive(Debug, Default)]
pub struct MockShardEngine {
    pub staged_mutations: HashMap<String, Vec<ShardMutation>>,
    pub committed_data: HashMap<String, Vec<u8>>,
    pub fail_prepare: bool,
}

impl MockShardEngine {
    pub fn prepare(&mut self, txn_id: &str, mutations: Vec<ShardMutation>) -> ShardVote {
        if self.fail_prepare {
            return ShardVote::No("Constraint violation on shard".into());
        }
        self.staged_mutations.insert(txn_id.to_string(), mutations);
        ShardVote::Yes
    }

    pub fn commit(&mut self, txn_id: &str) -> Result<()> {
        if let Some(mutations) = self.staged_mutations.remove(txn_id) {
            for m in mutations {
                match m {
                    ShardMutation::Insert { key, data, .. } | ShardMutation::Update { key, data, .. } => {
                        self.committed_data.insert(key, data);
                    }
                    ShardMutation::Delete { key, .. } => {
                        self.committed_data.remove(&key);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn abort(&mut self, txn_id: &str) {
        self.staged_mutations.remove(txn_id);
    }
}

pub struct TwoPhaseCommitCoordinator {
    node_id: String,
    next_txn_seq: AtomicU64,
    next_hlc_timestamp: AtomicU64,
    transactions: RwLock<HashMap<String, DistributedTxnRecord>>,
    shards: RwLock<HashMap<ShardId, MockShardEngine>>,
}

impl TwoPhaseCommitCoordinator {
    pub fn new(node_id: String) -> Self {
        Self {
            node_id,
            next_txn_seq: AtomicU64::new(1),
            next_hlc_timestamp: AtomicU64::new(1_000_000),
            transactions: RwLock::new(HashMap::new()),
            shards: RwLock::new(HashMap::new()),
        }
    }

    pub fn register_shard(&self, shard_id: ShardId, engine: MockShardEngine) {
        let mut shards = self.shards.write().unwrap();
        shards.insert(shard_id, engine);
    }

    /// Begins a new distributed transaction
    pub fn begin_transaction(&self) -> String {
        let seq = self.next_txn_seq.fetch_add(1, Ordering::SeqCst);
        let txn_id = format!("dtxn_{}_{}", self.node_id, seq);

        let record = DistributedTxnRecord {
            txn_id: txn_id.clone(),
            coordinator_id: self.node_id.clone(),
            state: TwoPhaseTxnState::Initiated,
            participants: Vec::new(),
            mutations: HashMap::new(),
            commit_timestamp: None,
        };

        let mut txns = self.transactions.write().unwrap();
        txns.insert(txn_id.clone(), record);
        txn_id
    }

    /// Stages a mutation targeting a specific partition/shard
    pub fn add_mutation(&self, txn_id: &str, shard_id: ShardId, mutation: ShardMutation) -> Result<()> {
        let mut txns = self.transactions.write().unwrap();
        let record = txns.get_mut(txn_id).context("Transaction not found")?;

        if record.state != TwoPhaseTxnState::Initiated {
            bail!("Cannot add mutations to transaction in state {:?}", record.state);
        }

        if !record.participants.contains(&shard_id) {
            record.participants.push(shard_id);
        }

        record.mutations.entry(shard_id).or_default().push(mutation);
        Ok(())
    }

    /// Executes the 2-Phase Commit (2PC) protocol across all participant shards
    pub fn commit(&self, txn_id: &str) -> Result<u64> {
        // ----------------------------------------------------
        // Phase 1: Prepare
        // ----------------------------------------------------
        {
            let mut txns = self.transactions.write().unwrap();
            let record = txns.get_mut(txn_id).context("Transaction not found")?;
            record.state = TwoPhaseTxnState::Preparing;
        }

        let (participants, mutations_map) = {
            let txns = self.transactions.read().unwrap();
            let record = txns.get(txn_id).unwrap();
            (record.participants.clone(), record.mutations.clone())
        };

        let mut all_votes_yes = true;
        let mut abort_reason = None;

        for shard_id in &participants {
            let mutations = mutations_map.get(shard_id).cloned().unwrap_or_default();
            let mut shards = self.shards.write().unwrap();
            if let Some(shard) = shards.get_mut(shard_id) {
                let vote = shard.prepare(txn_id, mutations);
                if let ShardVote::No(reason) = vote {
                    all_votes_yes = false;
                    abort_reason = Some(format!("Shard {} rejected: {}", shard_id, reason));
                    break;
                }
            } else {
                all_votes_yes = false;
                abort_reason = Some(format!("Shard {} unreachable", shard_id));
                break;
            }
        }

        if !all_votes_yes {
            let reason = abort_reason.unwrap_or_else(|| "Unknown prepare failure".into());
            self.abort_internal(txn_id, &reason, &participants)?;
            bail!("Transaction {} aborted during 2PC prepare: {}", txn_id, reason);
        }

        // ----------------------------------------------------
        // Phase 2: Commit (Decision is immutable once written)
        // ----------------------------------------------------
        let commit_ts = self.next_hlc_timestamp.fetch_add(1, Ordering::SeqCst);

        {
            let mut txns = self.transactions.write().unwrap();
            let record = txns.get_mut(txn_id).unwrap();
            record.state = TwoPhaseTxnState::Committed(commit_ts);
            record.commit_timestamp = Some(commit_ts);
        }

        // Broadcast commit to all participant shards
        let mut shards = self.shards.write().unwrap();
        for shard_id in &participants {
            if let Some(shard) = shards.get_mut(shard_id) {
                let _ = shard.commit(txn_id);
            }
        }

        Ok(commit_ts)
    }

    /// Aborts an in-flight transaction and releases locks on all participant shards
    pub fn abort(&self, txn_id: &str, reason: &str) -> Result<()> {
        let participants = {
            let txns = self.transactions.read().unwrap();
            let record = txns.get(txn_id).context("Transaction not found")?;
            record.participants.clone()
        };
        self.abort_internal(txn_id, reason, &participants)
    }

    fn abort_internal(&self, txn_id: &str, _reason: &str, participants: &[ShardId]) -> Result<()> {
        {
            let mut txns = self.transactions.write().unwrap();
            if let Some(record) = txns.get_mut(txn_id) {
                record.state = TwoPhaseTxnState::Aborted;
            }
        }

        let mut shards = self.shards.write().unwrap();
        for shard_id in participants {
            if let Some(shard) = shards.get_mut(shard_id) {
                shard.abort(txn_id);
            }
        }

        Ok(())
    }

    pub fn get_state(&self, txn_id: &str) -> Option<TwoPhaseTxnState> {
        let txns = self.transactions.read().unwrap();
        txns.get(txn_id).map(|r| r.state.clone())
    }

    pub fn get_shard_data(&self, shard_id: ShardId, key: &str) -> Option<Vec<u8>> {
        let shards = self.shards.read().unwrap();
        shards.get(&shard_id).and_then(|s| s.committed_data.get(key).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_2pc_cross_shard_atomic_commit() {
        let coordinator = TwoPhaseCommitCoordinator::new("node_1".into());

        // Shard 1 (Users), Shard 2 (Payments)
        coordinator.register_shard(1, MockShardEngine::default());
        coordinator.register_shard(2, MockShardEngine::default());

        let txn_id = coordinator.begin_transaction();

        coordinator.add_mutation(
            &txn_id,
            1,
            ShardMutation::Insert {
                table: "users".into(),
                key: "u_100".into(),
                data: b"Alice".to_vec(),
            },
        ).unwrap();

        coordinator.add_mutation(
            &txn_id,
            2,
            ShardMutation::Insert {
                table: "payments".into(),
                key: "p_500".into(),
                data: b"$250.00".to_vec(),
            },
        ).unwrap();

        // 2PC Commit
        let commit_ts = coordinator.commit(&txn_id).unwrap();
        assert!(commit_ts > 0);
        assert_eq!(coordinator.get_state(&txn_id), Some(TwoPhaseTxnState::Committed(commit_ts)));

        // Both shards must hold the committed records atomically
        assert_eq!(coordinator.get_shard_data(1, "u_100"), Some(b"Alice".to_vec()));
        assert_eq!(coordinator.get_shard_data(2, "p_500"), Some(b"$250.00".to_vec()));
    }

    #[test]
    fn test_2pc_shard_rejection_triggers_global_abort() {
        let coordinator = TwoPhaseCommitCoordinator::new("node_1".into());

        coordinator.register_shard(1, MockShardEngine::default());
        // Shard 2 fails prepare phase (simulating constraint violation / lock conflict)
        coordinator.register_shard(2, MockShardEngine { fail_prepare: true, ..Default::default() });

        let txn_id = coordinator.begin_transaction();

        coordinator.add_mutation(
            &txn_id,
            1,
            ShardMutation::Insert {
                table: "users".into(),
                key: "u_200".into(),
                data: b"Bob".to_vec(),
            },
        ).unwrap();

        coordinator.add_mutation(
            &txn_id,
            2,
            ShardMutation::Insert {
                table: "payments".into(),
                key: "p_600".into(),
                data: b"$500.00".to_vec(),
            },
        ).unwrap();

        // 2PC Commit must fail due to Shard 2's rejection
        let res = coordinator.commit(&txn_id);
        assert!(res.is_err());
        assert_eq!(coordinator.get_state(&txn_id), Some(TwoPhaseTxnState::Aborted));

        // Shard 1 must have cleanly rolled back (no dirty data leaked)
        assert_eq!(coordinator.get_shard_data(1, "u_200"), None);
        assert_eq!(coordinator.get_shard_data(2, "p_600"), None);
    }
}
