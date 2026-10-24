use crate::wal::{Lsn, WalRecord};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeReplicationRole {
    Primary,
    ReadReplica,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationHeartbeat {
    pub sender_id: String,
    pub current_lsn: Lsn,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplicationPacket {
    WalBatch {
        start_lsn: Lsn,
        records: Vec<WalRecord>,
    },
    Snapshot {
        checkpoint_lsn: Lsn,
        table_snapshots: Vec<(String, Vec<u8>)>,
    },
    Ack {
        replica_id: String,
        acknowledged_lsn: Lsn,
    },
}

pub struct ReplicationManager {
    node_id: String,
    role: RwLock<NodeReplicationRole>,
    applied_lsn: AtomicU64,
    replica_progress: RwLock<HashMap<String, Lsn>>, // replica_id -> last_ack_lsn
    standby_data: RwLock<HashMap<(String, String), Vec<u8>>>, // (table, key) -> data
}

impl ReplicationManager {
    pub fn new_primary(node_id: String) -> Self {
        Self {
            node_id,
            role: RwLock::new(NodeReplicationRole::Primary),
            applied_lsn: AtomicU64::new(0),
            replica_progress: RwLock::new(HashMap::new()),
            standby_data: RwLock::new(HashMap::new()),
        }
    }

    pub fn new_read_replica(node_id: String) -> Self {
        Self {
            node_id,
            role: RwLock::new(NodeReplicationRole::ReadReplica),
            applied_lsn: AtomicU64::new(0),
            replica_progress: RwLock::new(HashMap::new()),
            standby_data: RwLock::new(HashMap::new()),
        }
    }

    pub fn role(&self) -> NodeReplicationRole {
        *self.role.read().unwrap()
    }

    pub fn applied_lsn(&self) -> Lsn {
        self.applied_lsn.load(Ordering::SeqCst)
    }

    /// Checks if writes are permitted on this node
    pub fn ensure_writable(&self) -> Result<()> {
        if self.role() == NodeReplicationRole::ReadReplica {
            bail!("Cannot execute write: node '{}' is a read-only replica. Route writes to primary.", self.node_id);
        }
        Ok(())
    }

    /// Primary: Tracks replica progress
    pub fn record_replica_ack(&self, replica_id: &str, ack_lsn: Lsn) {
        let mut progress = self.replica_progress.write().unwrap();
        progress.insert(replica_id.to_string(), ack_lsn);
    }

    /// Primary: Calculates the minimum replication watermark across all active read replicas
    pub fn min_replica_watermark(&self) -> Lsn {
        let progress = self.replica_progress.read().unwrap();
        progress.values().copied().min().unwrap_or(0)
    }

    /// Read Replica: Ingests and replays a WAL batch streamed from primary
    pub fn apply_streamed_wal_batch(&self, _start_lsn: Lsn, records: &[WalRecord]) -> Result<Lsn> {
        let mut last_applied = self.applied_lsn.load(Ordering::SeqCst);
        let mut data = self.standby_data.write().unwrap();

        for r in records {
            if r.lsn <= last_applied {
                continue; // Skip duplicate / already applied
            }

            match &r.payload {
                crate::wal::WalPayload::Insert { table, key, data: bytes }
                | crate::wal::WalPayload::Update { table, key, new_data: bytes, .. } => {
                    data.insert((table.clone(), key.clone()), bytes.clone());
                }
                crate::wal::WalPayload::Delete { table, key, .. } => {
                    data.remove(&(table.clone(), key.clone()));
                }
                _ => {}
            }
            last_applied = r.lsn;
        }

        self.applied_lsn.store(last_applied, Ordering::SeqCst);
        Ok(last_applied)
    }

    /// Read Replica: Point lookup on hot standby data
    pub fn read_standby_data(&self, table: &str, key: &str) -> Option<Vec<u8>> {
        let data = self.standby_data.read().unwrap();
        data.get(&(table.to_string(), key.to_string())).cloned()
    }

    /// Primary / Standby: Creates a full state snapshot transfer package
    pub fn create_snapshot_package(&self, checkpoint_lsn: Lsn) -> ReplicationPacket {
        let data = self.standby_data.read().unwrap();
        let mut table_snapshots = Vec::new();
        for ((table, key), bytes) in data.iter() {
            let entry = format!("{}:{}", table, key).into_bytes();
            let mut payload = entry;
            payload.push(0); // delimiter
            payload.extend_from_slice(bytes);
            table_snapshots.push((table.clone(), payload));
        }
        ReplicationPacket::Snapshot {
            checkpoint_lsn,
            table_snapshots,
        }
    }

    /// Read Replica: Ingests and applies a full snapshot transfer from Primary
    pub fn apply_snapshot_package(&self, packet: &ReplicationPacket) -> Result<Lsn> {
        match packet {
            ReplicationPacket::Snapshot { checkpoint_lsn, table_snapshots } => {
                let mut data = self.standby_data.write().unwrap();
                data.clear();
                for (table, payload) in table_snapshots {
                    if let Some(delim_idx) = payload.iter().position(|&b| b == 0) {
                        let header = String::from_utf8_lossy(&payload[..delim_idx]);
                        if let Some((t, k)) = header.split_once(':') {
                            let value = payload[delim_idx + 1..].to_vec();
                            data.insert((t.to_string(), k.to_string()), value);
                        }
                    } else {
                        data.insert((table.clone(), "default".to_string()), payload.clone());
                    }
                }
                self.applied_lsn.store(*checkpoint_lsn, Ordering::SeqCst);
                Ok(*checkpoint_lsn)
            }
            _ => bail!("Expected ReplicationPacket::Snapshot"),
        }
    }

    /// Replica Catch-up: Replays missing WAL logs to bring replica up to current Primary LSN
    pub fn catchup_from_wal(&self, wal_records: &[WalRecord]) -> Result<Lsn> {
        let current_lsn = self.applied_lsn.load(Ordering::SeqCst);
        let eligible_records: Vec<WalRecord> = wal_records
            .iter()
            .filter(|r| r.lsn > current_lsn)
            .cloned()
            .collect();
        self.apply_streamed_wal_batch(current_lsn + 1, &eligible_records)
    }

    /// Automatic Failover: Promotes this read replica to new Primary
    pub fn promote_to_primary(&self) -> Result<Lsn> {
        let mut role = self.role.write().unwrap();
        *role = NodeReplicationRole::Primary;
        let lsn = self.applied_lsn.load(Ordering::SeqCst);
        Ok(lsn)
    }
}

/// Multi-node Enterprise Replication Coordinator (Primary + N Read Replicas)
pub struct ReplicationCluster {
    pub primary: ReplicationManager,
    pub replicas: HashMap<String, ReplicationManager>,
}

impl ReplicationCluster {
    pub fn new(primary_id: &str, replica_ids: &[&str]) -> Self {
        let primary = ReplicationManager::new_primary(primary_id.to_string());
        let mut replicas = HashMap::new();
        for &r_id in replica_ids {
            replicas.insert(r_id.to_string(), ReplicationManager::new_read_replica(r_id.to_string()));
        }
        Self { primary, replicas }
    }

    /// Streams a WAL batch from Primary to all active Read Replicas
    pub fn broadcast_wal_batch(&self, start_lsn: Lsn, records: &[WalRecord]) -> Result<()> {
        for (r_id, replica) in &self.replicas {
            let ack = replica.apply_streamed_wal_batch(start_lsn, records)?;
            self.primary.record_replica_ack(r_id, ack);
        }
        Ok(())
    }

    /// Automatic Failover: If Primary goes down, elects the Read Replica with highest LSN and promotes it
    pub fn failover_to_best_replica(&mut self) -> Result<String> {
        if self.replicas.is_empty() {
            bail!("Cannot failover: No available read replicas");
        }

        // Elect replica with highest applied LSN
        let best_replica_id = self.replicas.iter()
            .max_by_key(|(_, r)| r.applied_lsn())
            .map(|(id, _)| id.clone())
            .unwrap();

        let promoted_manager = self.replicas.remove(&best_replica_id).unwrap();
        promoted_manager.promote_to_primary()?;
        self.primary = promoted_manager;
        Ok(best_replica_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wal::{WalPayload, WalRecord};

    #[test]
    fn test_primary_replica_streaming_replication_and_promotion() {
        let primary = ReplicationManager::new_primary("primary_node".into());
        let replica = ReplicationManager::new_read_replica("replica_1".into());

        // 1. Replica rejects write operations
        assert!(replica.ensure_writable().is_err());
        assert!(primary.ensure_writable().is_ok());

        // 2. Stream WAL records from primary to replica
        let r1 = WalRecord::new(
            1,
            0,
            10,
            WalPayload::Insert {
                table: "users".into(),
                key: "u1".into(),
                data: b"Alice".to_vec(),
            },
        );
        let r2 = WalRecord::new(
            2,
            1,
            10,
            WalPayload::Insert {
                table: "users".into(),
                key: "u2".into(),
                data: b"Bob".to_vec(),
            },
        );

        let ack = replica.apply_streamed_wal_batch(1, &[r1, r2]).unwrap();
        assert_eq!(ack, 2);
        assert_eq!(replica.applied_lsn(), 2);

        // Verify replica can serve read-only queries
        assert_eq!(replica.read_standby_data("users", "u1"), Some(b"Alice".to_vec()));
        assert_eq!(replica.read_standby_data("users", "u2"), Some(b"Bob".to_vec()));

        // Primary tracks replica ack
        primary.record_replica_ack("replica_1", ack);
        assert_eq!(primary.min_replica_watermark(), 2);

        // 3. Promote replica on primary failure
        let promo_lsn = replica.promote_to_primary().unwrap();
        assert_eq!(promo_lsn, 2);
        assert_eq!(replica.role(), NodeReplicationRole::Primary);
        // Newly promoted replica now accepts writes!
        assert!(replica.ensure_writable().is_ok());
    }

    #[test]
    fn test_enterprise_multi_replica_cluster_and_failover() {
        // Topology: 1 Primary and 3 Replicas (Replica 1, 2, 3)
        let mut cluster = ReplicationCluster::new("primary_master", &["replica_1", "replica_2", "replica_3"]);
        assert_eq!(cluster.replicas.len(), 3);

        let r1 = WalRecord::new(
            1, 0, 10,
            WalPayload::Insert {
                table: "accounts".into(),
                key: "acc_1".into(),
                data: b"balance:1000".to_vec(),
            },
        );
        let r2 = WalRecord::new(
            2, 1, 10,
            WalPayload::Insert {
                table: "accounts".into(),
                key: "acc_2".into(),
                data: b"balance:2500".to_vec(),
            },
        );

        cluster.broadcast_wal_batch(1, &[r1, r2]).unwrap();

        for r_id in &["replica_1", "replica_2", "replica_3"] {
            let replica = cluster.replicas.get(*r_id).unwrap();
            assert_eq!(replica.applied_lsn(), 2);
            assert_eq!(replica.read_standby_data("accounts", "acc_1"), Some(b"balance:1000".to_vec()));
        }

        // Replica 3 receives an additional WAL record before primary fails
        let r3 = WalRecord::new(
            3, 2, 11,
            WalPayload::Insert {
                table: "accounts".into(),
                key: "acc_3".into(),
                data: b"balance:5000".to_vec(),
            },
        );
        cluster.replicas.get("replica_3").unwrap().apply_streamed_wal_batch(3, &[r3]).unwrap();

        // Primary fails: Automatic failover elects replica_3 as the new Primary
        let elected = cluster.failover_to_best_replica().unwrap();
        assert_eq!(elected, "replica_3");
        assert_eq!(cluster.primary.role(), NodeReplicationRole::Primary);
        assert_eq!(cluster.primary.applied_lsn(), 3);
        assert_eq!(cluster.replicas.len(), 2);
    }

    #[test]
    fn test_replica_snapshot_transfer_and_catchup() {
        let primary = ReplicationManager::new_primary("primary".into());
        let replica = ReplicationManager::new_read_replica("replica_catchup".into());

        let r1 = WalRecord::new(
            100, 99, 1,
            WalPayload::Insert {
                table: "config".into(),
                key: "theme".into(),
                data: b"dark".to_vec(),
            },
        );
        primary.apply_streamed_wal_batch(100, &[r1]).unwrap();

        // 1. Snapshot Transfer
        let snapshot = primary.create_snapshot_package(100);
        let applied_snapshot_lsn = replica.apply_snapshot_package(&snapshot).unwrap();
        assert_eq!(applied_snapshot_lsn, 100);
        assert_eq!(replica.applied_lsn(), 100);
        assert_eq!(replica.read_standby_data("config", "theme"), Some(b"dark".to_vec()));

        // 2. Replica Catch-up with WAL delta
        let r2 = WalRecord::new(
            101, 100, 2,
            WalPayload::Insert {
                table: "config".into(),
                key: "font".into(),
                data: b"monospace".to_vec(),
            },
        );
        let catchup_lsn = replica.catchup_from_wal(&[r2]).unwrap();
        assert_eq!(catchup_lsn, 101);
        assert_eq!(replica.applied_lsn(), 101);
        assert_eq!(replica.read_standby_data("config", "font"), Some(b"monospace".to_vec()));
    }
}
