use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::RwLock;

pub const DEFAULT_VNODES_PER_NODE: usize = 64;
pub const DEFAULT_PARTITION_COUNT: u32 = 16;

#[derive(Debug, Clone)]
pub struct ConsistentHashRing {
    vnodes_per_node: usize,
    ring: BTreeMap<u32, String>, // hash -> node_id
}

impl ConsistentHashRing {
    pub fn new(vnodes_per_node: usize) -> Self {
        Self {
            vnodes_per_node,
            ring: BTreeMap::new(),
        }
    }

    fn hash_bytes(data: &[u8]) -> u32 {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();
        u32::from_be_bytes([result[0], result[1], result[2], result[3]])
    }

    pub fn add_node(&mut self, node_id: &str) {
        for i in 0..self.vnodes_per_node {
            let vnode_key = format!("{}:vnode_{}", node_id, i);
            let hash = Self::hash_bytes(vnode_key.as_bytes());
            self.ring.insert(hash, node_id.to_string());
        }
    }

    pub fn remove_node(&mut self, node_id: &str) {
        self.ring.retain(|_, v| v != node_id);
    }

    pub fn get_node(&self, key: &str) -> Option<String> {
        if self.ring.is_empty() {
            return None;
        }

        let hash = Self::hash_bytes(key.as_bytes());
        // Find first node with hash >= key_hash
        if let Some((_, node_id)) = self.ring.range(hash..).next() {
            Some(node_id.clone())
        } else {
            // Wrap around to beginning of ring
            self.ring.values().next().cloned()
        }
    }
}

pub struct ShardManager {
    partition_count: u32,
    ring: RwLock<ConsistentHashRing>,
    shard_to_node: RwLock<BTreeMap<u32, String>>,
}

impl ShardManager {
    pub fn new(partition_count: u32) -> Self {
        Self {
            partition_count,
            ring: RwLock::new(ConsistentHashRing::new(DEFAULT_VNODES_PER_NODE)),
            shard_to_node: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn register_node(&self, node_id: &str) {
        let mut ring = self.ring.write().unwrap();
        ring.add_node(node_id);

        // Rebalance shards across ring
        let mut shards = self.shard_to_node.write().unwrap();
        for shard_id in 0..self.partition_count {
            let shard_key = format!("shard_{}", shard_id);
            if let Some(target_node) = ring.get_node(&shard_key) {
                shards.insert(shard_id, target_node);
            }
        }
    }

    pub fn unregister_node(&self, node_id: &str) {
        let mut ring = self.ring.write().unwrap();
        ring.remove_node(node_id);

        let mut shards = self.shard_to_node.write().unwrap();
        for shard_id in 0..self.partition_count {
            let shard_key = format!("shard_{}", shard_id);
            if let Some(target_node) = ring.get_node(&shard_key) {
                shards.insert(shard_id, target_node);
            }
        }
    }

    pub fn get_node_for_shard(&self, shard_id: u32) -> Option<String> {
        let shards = self.shard_to_node.read().unwrap();
        shards.get(&shard_id).cloned()
    }

    pub fn get_shard_for_key(&self, key: &str) -> u32 {
        let hash = ConsistentHashRing::hash_bytes(key.as_bytes());
        hash % self.partition_count
    }

    pub fn get_node_for_key(&self, key: &str) -> Option<String> {
        let shard = self.get_shard_for_key(key);
        self.get_node_for_shard(shard)
    }

    pub fn get_shards_for_node(&self, target_node: &str) -> Vec<u32> {
        let shards = self.shard_to_node.read().unwrap();
        shards
            .iter()
            .filter_map(|(&shard_id, node)| if node == target_node { Some(shard_id) } else { None })
            .collect()
    }

    pub fn get_all_shard_mappings(&self) -> BTreeMap<u32, String> {
        self.shard_to_node.read().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consistent_hash_ring() {
        let mut ring = ConsistentHashRing::new(64);
        ring.add_node("node-1");
        ring.add_node("node-2");
        ring.add_node("node-3");

        let node_a = ring.get_node("user_1001").expect("Must find node");
        let node_b = ring.get_node("user_1001").expect("Must find same node");
        assert_eq!(node_a, node_b, "Consistent hash routing must be deterministic");

        // Key should map to one of the 3 nodes
        assert!(["node-1", "node-2", "node-3"].contains(&node_a.as_str()));
    }

    #[test]
    fn test_shard_manager_rebalance() {
        let manager = ShardManager::new(16);
        manager.register_node("node-1");

        // All 16 shards must belong to node-1 initially
        let shards_1 = manager.get_shards_for_node("node-1");
        assert_eq!(shards_1.len(), 16);

        // Add node-2
        manager.register_node("node-2");
        let shards_after_1 = manager.get_shards_for_node("node-1");
        let shards_after_2 = manager.get_shards_for_node("node-2");

        assert_eq!(shards_after_1.len() + shards_after_2.len(), 16);
        assert!(!shards_after_1.is_empty());
        assert!(!shards_after_2.is_empty());

        // Remove node-2
        manager.unregister_node("node-2");
        let shards_final = manager.get_shards_for_node("node-1");
        assert_eq!(shards_final.len(), 16);
    }
}
