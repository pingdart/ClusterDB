use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub type TxnId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LockMode {
    IntentShared,          // IS
    IntentExclusive,       // IX
    Shared,                // S (Read)
    SharedIntentExclusive, // SIX
    Exclusive,             // X (Write)
}

impl LockMode {
    /// Standard 2PL / Hierarchical Locking Compatibility Matrix
    pub fn is_compatible(&self, other: &LockMode) -> bool {
        match (self, other) {
            (LockMode::IntentShared, LockMode::IntentShared) => true,
            (LockMode::IntentShared, LockMode::IntentExclusive) => true,
            (LockMode::IntentShared, LockMode::Shared) => true,
            (LockMode::IntentShared, LockMode::SharedIntentExclusive) => true,
            (LockMode::IntentShared, LockMode::Exclusive) => false,

            (LockMode::IntentExclusive, LockMode::IntentShared) => true,
            (LockMode::IntentExclusive, LockMode::IntentExclusive) => true,
            (LockMode::IntentExclusive, LockMode::Shared) => false,
            (LockMode::IntentExclusive, LockMode::SharedIntentExclusive) => false,
            (LockMode::IntentExclusive, LockMode::Exclusive) => false,

            (LockMode::Shared, LockMode::IntentShared) => true,
            (LockMode::Shared, LockMode::IntentExclusive) => false,
            (LockMode::Shared, LockMode::Shared) => true,
            (LockMode::Shared, LockMode::SharedIntentExclusive) => false,
            (LockMode::Shared, LockMode::Exclusive) => false,

            (LockMode::SharedIntentExclusive, LockMode::IntentShared) => true,
            (LockMode::SharedIntentExclusive, LockMode::IntentExclusive) => false,
            (LockMode::SharedIntentExclusive, LockMode::Shared) => false,
            (LockMode::SharedIntentExclusive, LockMode::SharedIntentExclusive) => false,
            (LockMode::SharedIntentExclusive, LockMode::Exclusive) => false,

            (LockMode::Exclusive, _) => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceId {
    Database(String),
    Table(String),
    Row { table: String, key: String },
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct LockRequest {
    pub txn_id: TxnId,
    pub mode: LockMode,
    pub granted: bool,
}

#[derive(Debug, Default)]
struct LockHead {
    holders: Vec<(TxnId, LockMode)>,
    waiters: VecDeque<(TxnId, LockMode)>,
}

impl LockHead {
    fn can_grant(&self, requester: TxnId, req_mode: LockMode) -> bool {
        for (holder_txn, holder_mode) in &self.holders {
            if *holder_txn == requester {
                continue; // Re-entrant lock check handled separately
            }
            if !holder_mode.is_compatible(&req_mode) {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Default)]
struct LockState {
    resource_locks: HashMap<ResourceId, LockHead>,
    txn_locks: HashMap<TxnId, HashSet<ResourceId>>,
}

#[derive(Debug)]
pub struct LockManager {
    state: Mutex<LockState>,
    cvar: Condvar,
}

impl LockManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(LockState::default()),
            cvar: Condvar::new(),
        })
    }

    /// Acquires a lock on a resource with deadlock detection and timeout
    pub fn acquire(
        &self,
        txn_id: TxnId,
        resource: ResourceId,
        mode: LockMode,
        timeout: Duration,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();

        // 1. Check if already holding an equal or stronger compatible lock
        if let Some(held) = state.txn_locks.get(&txn_id) {
            if held.contains(&resource) {
                let head = state.resource_locks.get(&resource).unwrap();
                if head.holders.iter().any(|(t, m)| *t == txn_id && (*m == mode || *m == LockMode::Exclusive)) {
                    return Ok(());
                }
            }
        }

        // 2. Fast path: Can lock be immediately granted?
        let head = state.resource_locks.entry(resource.clone()).or_default();
        if head.waiters.is_empty() && head.can_grant(txn_id, mode) {
            head.holders.push((txn_id, mode));
            state.txn_locks.entry(txn_id).or_default().insert(resource);
            return Ok(());
        }

        // 3. Register as a waiter on this resource
        head.waiters.push_back((txn_id, mode));

        // 4. Deadlock Detection: Build Wait-For Graph and test for directed cycles
        if let Some(cycle) = self.detect_deadlock_internal(&state) {
            // Remove self from waiters before returning error
            if let Some(h) = state.resource_locks.get_mut(&resource) {
                h.waiters.retain(|(t, _)| *t != txn_id);
            }
            bail!(
                "Deadlock detected involving transactions {:?}. Transaction {} aborted.",
                cycle,
                txn_id
            );
        }

        // 5. Wait on condition variable until granted or timed out
        let start_time = std::time::Instant::now();
        loop {
            let elapsed = start_time.elapsed();
            if elapsed >= timeout {
                // Timeout: remove from waiters
                if let Some(h) = state.resource_locks.get_mut(&resource) {
                    h.waiters.retain(|(t, _)| *t != txn_id);
                }
                bail!(
                    "Lock acquisition timed out after {:?} for txn {} on {:?}",
                    timeout,
                    txn_id,
                    resource
                );
            }

            let rem = timeout - elapsed;
            let result = self.cvar.wait_timeout(state, rem).unwrap();
            state = result.0;

            let h = state.resource_locks.get_mut(&resource).unwrap();
            // Check if we are now at the front of waiters and can grant
            if let Some(&(front_txn, front_mode)) = h.waiters.front() {
                if front_txn == txn_id && h.can_grant(txn_id, front_mode) {
                    h.waiters.pop_front();
                    h.holders.push((txn_id, mode));
                    state.txn_locks.entry(txn_id).or_default().insert(resource);
                    return Ok(());
                }
            }
        }
    }

    /// Releases a specific lock held by a transaction
    #[allow(dead_code)]
    pub fn release(&self, txn_id: TxnId, resource: &ResourceId) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if let Some(head) = state.resource_locks.get_mut(resource) {
            head.holders.retain(|(t, _)| *t != txn_id);
        }
        if let Some(held) = state.txn_locks.get_mut(&txn_id) {
            held.remove(resource);
        }
        self.cvar.notify_all();
        Ok(())
    }

    /// Releases all locks held by a transaction (called on Commit or Abort)
    pub fn release_all(&self, txn_id: TxnId) {
        let mut state = self.state.lock().unwrap();
        if let Some(resources) = state.txn_locks.remove(&txn_id) {
            for res in resources {
                if let Some(head) = state.resource_locks.get_mut(&res) {
                    head.holders.retain(|(t, _)| *t != txn_id);
                    head.waiters.retain(|(t, _)| *t != txn_id);
                }
            }
        }
        self.cvar.notify_all();
    }

    /// Detects cycles in the Wait-For Graph using Depth-First Search (DFS)
    fn detect_deadlock_internal(&self, state: &LockState) -> Option<Vec<TxnId>> {
        // Construct adjacency list: waiting_txn -> Vec<holding_txn>
        let mut wait_for: HashMap<TxnId, Vec<TxnId>> = HashMap::new();

        for head in state.resource_locks.values() {
            for &(waiter_id, _) in &head.waiters {
                for &(holder_id, _) in &head.holders {
                    if waiter_id != holder_id {
                        wait_for.entry(waiter_id).or_default().push(holder_id);
                    }
                }
            }
        }

        let mut visited: HashSet<TxnId> = HashSet::new();
        let mut rec_stack: Vec<TxnId> = Vec::new();

        for &start_txn in wait_for.keys() {
            if !visited.contains(&start_txn) {
                if let Some(cycle) = Self::dfs_cycle(&start_txn, &wait_for, &mut visited, &mut rec_stack) {
                    return Some(cycle);
                }
            }
        }

        None
    }

    fn dfs_cycle(
        node: &TxnId,
        adj: &HashMap<TxnId, Vec<TxnId>>,
        visited: &mut HashSet<TxnId>,
        stack: &mut Vec<TxnId>,
    ) -> Option<Vec<TxnId>> {
        visited.insert(*node);
        stack.push(*node);

        if let Some(neighbors) = adj.get(node) {
            for neighbor in neighbors {
                if !visited.contains(neighbor) {
                    if let Some(cycle) = Self::dfs_cycle(neighbor, adj, visited, stack) {
                        return Some(cycle);
                    }
                } else if stack.contains(neighbor) {
                    // Cycle detected!
                    let pos = stack.iter().position(|x| x == neighbor).unwrap();
                    let cycle = stack[pos..].to_vec();
                    return Some(cycle);
                }
            }
        }

        stack.pop();
        None
    }

    pub fn is_holding(&self, txn_id: TxnId, resource: &ResourceId) -> bool {
        let state = self.state.lock().unwrap();
        state
            .txn_locks
            .get(&txn_id)
            .map(|set| set.contains(resource))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_compatibility() {
        assert!(LockMode::Shared.is_compatible(&LockMode::Shared));
        assert!(LockMode::IntentShared.is_compatible(&LockMode::IntentExclusive));
        assert!(!LockMode::Exclusive.is_compatible(&LockMode::Exclusive));
        assert!(!LockMode::Shared.is_compatible(&LockMode::Exclusive));
    }

    #[test]
    fn test_concurrent_shared_locks() {
        let lm = LockManager::new();
        let table_res = ResourceId::Table("users".into());

        // Both Txn 1 and Txn 2 can acquire Shared lock concurrently
        assert!(lm.acquire(1, table_res.clone(), LockMode::Shared, Duration::from_millis(100)).is_ok());
        assert!(lm.acquire(2, table_res.clone(), LockMode::Shared, Duration::from_millis(100)).is_ok());

        assert!(lm.is_holding(1, &table_res));
        assert!(lm.is_holding(2, &table_res));

        lm.release_all(1);
        lm.release_all(2);
    }

    #[test]
    fn test_exclusive_lock_conflict_and_release() {
        let lm = LockManager::new();
        let row_res = ResourceId::Row {
            table: "orders".into(),
            key: "ord_101".into(),
        };

        // Txn 1 takes Exclusive Lock
        assert!(lm.acquire(1, row_res.clone(), LockMode::Exclusive, Duration::from_millis(100)).is_ok());

        // Txn 2 tries to take Exclusive Lock -> times out
        let res2 = lm.acquire(2, row_res.clone(), LockMode::Exclusive, Duration::from_millis(50));
        assert!(res2.is_err());

        // Txn 1 releases lock
        lm.release_all(1);

        // Txn 2 now succeeds
        assert!(lm.acquire(2, row_res.clone(), LockMode::Exclusive, Duration::from_millis(100)).is_ok());
        lm.release_all(2);
    }

    #[test]
    fn test_deadlock_cycle_detection() {
        let lm = LockManager::new();
        let res_a = ResourceId::Row { table: "t".into(), key: "a".into() };
        let res_b = ResourceId::Row { table: "t".into(), key: "b".into() };

        // Txn 1 acquires Res A
        lm.acquire(1, res_a.clone(), LockMode::Exclusive, Duration::from_millis(100)).unwrap();
        // Txn 2 acquires Res B
        lm.acquire(2, res_b.clone(), LockMode::Exclusive, Duration::from_millis(100)).unwrap();

        // Spawn Txn 1 attempting to acquire Res B in a background thread (will block waiting for Txn 2)
        let lm_clone = lm.clone();
        let res_b_clone = res_b.clone();
        let handle = std::thread::spawn(move || {
            lm_clone.acquire(1, res_b_clone, LockMode::Exclusive, Duration::from_millis(500))
        });

        // Small sleep so Txn 1 registers as waiter on Res B
        std::thread::sleep(Duration::from_millis(30));

        // Now Txn 2 attempts to acquire Res A -> creating cycle (1 -> 2 -> 1)
        let deadlock_result = lm.acquire(2, res_a.clone(), LockMode::Exclusive, Duration::from_millis(500));

        // Deadlock must be detected immediately without stalling
        assert!(deadlock_result.is_err());
        assert!(deadlock_result.unwrap_err().to_string().contains("Deadlock detected"));

        lm.release_all(2);
        let _ = handle.join();
        lm.release_all(1);
    }
}
