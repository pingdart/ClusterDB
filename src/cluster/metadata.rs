use crate::cluster::node::{NodeInfo, NodeRole, NodeStatus};
use crate::cluster::sharding::ShardManager;
use crate::storage::StorageEngine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterSummary {
    pub cluster_id: String,
    pub node_id: String,
    pub address: String,
    pub role: NodeRole,
    pub leader_id: Option<String>,
    pub term: u64,
    pub nodes: Vec<NodeInfo>,
    pub shard_count: u32,
    pub shard_mappings: std::collections::BTreeMap<u32, String>,
}

pub struct ClusterMetadataService {
    pub cluster_id: String,
    storage: StorageEngine,
    nodes: RwLock<HashMap<String, NodeInfo>>,
    pub shard_manager: Arc<ShardManager>,
}

impl ClusterMetadataService {
    pub fn new(cluster_id: String, storage: StorageEngine) -> Self {
        let shard_manager = Arc::new(ShardManager::new(16));

        // Load known nodes from storage
        let mut nodes = HashMap::new();
        if let Ok(Some(saved_nodes)) = storage.get::<HashMap<String, NodeInfo>>("_cluster_meta", "nodes") {
            for (id, _node) in &saved_nodes {
                shard_manager.register_node(id);
            }
            nodes = saved_nodes;
        }

        Self {
            cluster_id,
            storage,
            nodes: RwLock::new(nodes),
            shard_manager,
        }
    }

    pub async fn register_node(&self, mut node: NodeInfo) {
        let mut nodes = self.nodes.write().await;
        node.last_heartbeat_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        node.status = NodeStatus::Online;

        self.shard_manager.register_node(&node.id);
        node.shards = self.shard_manager.get_shards_for_node(&node.id);
        nodes.insert(node.id.clone(), node);

        let _ = self.storage.set("_cluster_meta", "nodes", &*nodes);
    }

    pub async fn heartbeat(&self, node_id: &str) {
        let mut nodes = self.nodes.write().await;
        if let Some(node) = nodes.get_mut(node_id) {
            node.last_heartbeat_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            node.status = NodeStatus::Online;
        }
    }

    pub async fn check_node_health(&self, timeout_ms: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut nodes = self.nodes.write().await;
        let mut changed = false;
        for node in nodes.values_mut() {
            if now.saturating_sub(node.last_heartbeat_ms) > timeout_ms && node.status == NodeStatus::Online {
                node.status = NodeStatus::Offline;
                changed = true;
            }
        }
        if changed {
            let _ = self.storage.set("_cluster_meta", "nodes", &*nodes);
        }
    }

    pub async fn get_all_nodes(&self) -> Vec<NodeInfo> {
        let nodes = self.nodes.read().await;
        nodes.values().cloned().collect()
    }

    pub async fn get_node(&self, node_id: &str) -> Option<NodeInfo> {
        let nodes = self.nodes.read().await;
        nodes.get(node_id).cloned()
    }
}
