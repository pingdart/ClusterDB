pub mod distributed_search;
pub mod metadata;
pub mod node;
pub mod raft;
pub mod sharding;

use crate::cluster::distributed_search::DistributedSearchCoordinator;
use crate::cluster::metadata::{ClusterMetadataService, ClusterSummary};
use crate::cluster::node::{NodeInfo, NodeRole};
use crate::cluster::raft::{ClusterCommand, RaftEngine};
use crate::exec::Executor;
use crate::storage::StorageEngine;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

pub struct ClusterEngine {
    pub raft: Arc<RaftEngine>,
    pub metadata: Arc<ClusterMetadataService>,
    pub search_coordinator: Arc<DistributedSearchCoordinator>,
    storage: StorageEngine,
    executor: Arc<Executor>,
}

impl ClusterEngine {
    pub fn new(
        node_id: String,
        address: String,
        cluster_id: String,
        storage: StorageEngine,
        executor: Arc<Executor>,
    ) -> Self {
        let raft = Arc::new(RaftEngine::new(node_id.clone(), address.clone(), storage.clone()));
        let metadata = Arc::new(ClusterMetadataService::new(cluster_id, storage.clone()));
        let search_coordinator = Arc::new(DistributedSearchCoordinator::new(
            executor.clone(),
            metadata.clone(),
        ));

        Self {
            raft,
            metadata,
            search_coordinator,
            storage,
            executor,
        }
    }

    pub fn start_background_tasks(self: &Arc<Self>) {
        let cluster = self.clone();

        // 1. Raft Heartbeat & Election Loop
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(50));
            let mut last_election_check = std::time::Instant::now();
            let mut election_timeout = Duration::from_millis(300);

            loop {
                ticker.tick().await;

                let role = cluster.raft.get_role().await;
                if role == NodeRole::Leader {
                    cluster.raft.send_heartbeats().await;
                } else {
                    // Check if election timeout elapsed
                    if last_election_check.elapsed() >= election_timeout {
                        cluster.raft.start_election().await;
                        last_election_check = std::time::Instant::now();
                        // Randomize next timeout between 200ms and 400ms
                        let mut rng = rand::thread_rng();
                        election_timeout = Duration::from_millis(rand::Rng::gen_range(&mut rng, 200..400));
                    }
                }

                // Apply any committed log entries to local state machine
                let pending = cluster.raft.get_pending_applied_entries().await;
                for entry in pending {
                    match entry.command {
                        ClusterCommand::ExecuteSql { query, database } => {
                            // Only followers apply replicated SQL; Leader already executed it upon proposal
                            if role != NodeRole::Leader {
                                let _ = cluster.executor.execute(&query, database.as_deref());
                            }
                        }
                        ClusterCommand::ApplyCDC { change } => {
                            if role != NodeRole::Leader {
                                let _ = cluster.storage.set("_cdc_events", &change.sequence_id.to_string(), &change);
                            }
                        }
                        ClusterCommand::RegisterNode { node } => {
                            cluster.metadata.register_node(node).await;
                        }
                        ClusterCommand::UpdateShardMap { shard_id: _, node_id } => {
                            cluster.metadata.shard_manager.register_node(&node_id);
                        }
                        ClusterCommand::Noop => {}
                    }
                }

                // Check node health
                cluster.metadata.check_node_health(2000).await;
            }
        });
    }

    pub async fn get_summary(&self) -> ClusterSummary {
        let role = self.raft.get_role().await;
        let leader_id = self.raft.get_leader_id().await;
        let term = self.raft.get_current_term();
        let nodes = self.metadata.get_all_nodes().await;
        let shard_mappings = self.metadata.shard_manager.get_all_shard_mappings();

        ClusterSummary {
            cluster_id: self.metadata.cluster_id.clone(),
            node_id: self.raft.node_id.clone(),
            address: self.raft.address.clone(),
            role,
            leader_id,
            term,
            nodes,
            shard_count: 16,
            shard_mappings,
        }
    }

    pub async fn join_cluster(&self, seed_addr: &str) -> Result<()> {
        let node_info = NodeInfo::new(self.raft.node_id.clone(), self.raft.address.clone());
        let client = reqwest::Client::new();
        let url = format!("http://{}/cluster/join", seed_addr);

        let resp = client.post(&url).json(&node_info).send().await?;
        if resp.status().is_success() {
            let data: serde_json::Value = resp.json().await.unwrap_or_default();
            if let Some(nodes) = data.get("nodes").and_then(|n| n.as_array()) {
                for n_val in nodes {
                    if let Ok(n) = serde_json::from_value::<NodeInfo>(n_val.clone()) {
                        if n.id != self.raft.node_id {
                            self.raft.add_peer(n.id.clone(), n.address.clone()).await;
                        }
                        self.metadata.register_node(n).await;
                    }
                }
            }
            if let Some(leader) = data.get("leader_id").and_then(|l| l.as_str()) {
                self.raft.set_leader(leader).await;
            }
            info!("Successfully joined cluster via seed node {}", seed_addr);
            Ok(())
        } else {
            warn!("Failed to join cluster via seed node {}: status {}", seed_addr, resp.status());
            anyhow::bail!("Failed to join cluster: {}", resp.status());
        }
    }
}
