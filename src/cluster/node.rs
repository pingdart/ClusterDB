use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeRole {
    Leader,
    Follower,
    Candidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    Online,
    Offline,
    CatchingUp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub id: String,
    pub address: String, // e.g. "127.0.0.1:4650"
    pub role: NodeRole,
    pub status: NodeStatus,
    pub last_heartbeat_ms: u64,
    pub shards: Vec<u32>,
}

impl NodeInfo {
    pub fn new(id: String, address: String) -> Self {
        Self {
            id,
            address,
            role: NodeRole::Follower,
            status: NodeStatus::Online,
            last_heartbeat_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            shards: Vec::new(),
        }
    }
}
