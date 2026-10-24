use crate::cluster::node::{NodeInfo, NodeRole};
use crate::storage::StorageEngine;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClusterCommand {
    ExecuteSql {
        query: String,
        database: Option<String>,
    },
    ApplyCDC {
        change: crate::cdc::CDCRecord,
    },
    RegisterNode {
        node: NodeInfo,
    },
    UpdateShardMap {
        shard_id: u32,
        node_id: String,
    },
    Noop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftLogEntry {
    pub term: u64,
    pub index: u64,
    pub command: ClusterCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestVoteArgs {
    pub term: u64,
    pub candidate_id: String,
    pub last_log_index: u64,
    pub last_log_term: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestVoteReply {
    pub term: u64,
    pub vote_granted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntriesArgs {
    pub term: u64,
    pub leader_id: String,
    pub prev_log_index: u64,
    pub prev_log_term: u64,
    pub entries: Vec<RaftLogEntry>,
    pub leader_commit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntriesReply {
    pub term: u64,
    pub success: bool,
    pub match_index: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistentRaftState {
    current_term: u64,
    voted_for: Option<String>,
}

pub struct RaftEngine {
    pub node_id: String,
    pub address: String,
    storage: StorageEngine,
    current_term: AtomicU64,
    voted_for: RwLock<Option<String>>,
    role: RwLock<NodeRole>,
    leader_id: RwLock<Option<String>>,
    peers: RwLock<HashMap<String, String>>, // node_id -> address
    log: RwLock<Vec<RaftLogEntry>>,
    commit_index: AtomicU64,
    last_applied: AtomicU64,
    last_heartbeat_time: Mutex<Instant>,
    http_client: reqwest::Client,
}

impl RaftEngine {
    pub fn new(node_id: String, address: String, storage: StorageEngine) -> Self {
        // Load persistent state if available
        let saved_state: Option<PersistentRaftState> = storage
            .get("_raft_meta", "state")
            .unwrap_or(None);

        let (term, voted) = match saved_state {
            Some(s) => (s.current_term, s.voted_for),
            None => (0, None),
        };

        // Load log entries from storage if available
        let mut log = Vec::new();
        let log_count: u64 = storage.get("_raft_meta", "log_count").unwrap_or(None).unwrap_or(0);
        for i in 1..=log_count {
            if let Ok(Some(entry)) = storage.get::<RaftLogEntry>("_raft_log", &i.to_string()) {
                log.push(entry);
            }
        }

        Self {
            node_id,
            address,
            storage,
            current_term: AtomicU64::new(term),
            voted_for: RwLock::new(voted),
            role: RwLock::new(NodeRole::Follower),
            leader_id: RwLock::new(None),
            peers: RwLock::new(HashMap::new()),
            log: RwLock::new(log),
            commit_index: AtomicU64::new(0),
            last_applied: AtomicU64::new(0),
            last_heartbeat_time: Mutex::new(Instant::now()),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_millis(250))
                .build()
                .unwrap_or_default(),
        }
    }

    fn persist_state(&self, term: u64, voted_for: Option<String>) {
        let state = PersistentRaftState {
            current_term: term,
            voted_for,
        };
        let _ = self.storage.set("_raft_meta", "state", &state);
    }

    pub async fn add_peer(&self, id: String, address: String) {
        let mut peers = self.peers.write().await;
        peers.insert(id, address);
    }

    pub async fn remove_peer(&self, id: &str) {
        let mut peers = self.peers.write().await;
        peers.remove(id);
    }

    pub async fn get_peers(&self) -> HashMap<String, String> {
        self.peers.read().await.clone()
    }

    pub async fn get_role(&self) -> NodeRole {
        *self.role.read().await
    }

    pub async fn get_leader_id(&self) -> Option<String> {
        self.leader_id.read().await.clone()
    }

    pub async fn set_leader(&self, leader: &str) {
        *self.leader_id.write().await = Some(leader.to_string());
    }

    pub fn get_current_term(&self) -> u64 {
        self.current_term.load(Ordering::SeqCst)
    }

    pub fn get_commit_index(&self) -> u64 {
        self.commit_index.load(Ordering::SeqCst)
    }

    pub async fn time_since_last_heartbeat(&self) -> Duration {
        self.last_heartbeat_time.lock().await.elapsed()
    }

    pub async fn reset_heartbeat_time(&self) {
        *self.last_heartbeat_time.lock().await = Instant::now();
    }

    pub async fn get_last_log_index_and_term(&self) -> (u64, u64) {
        let log = self.log.read().await;
        if let Some(last) = log.last() {
            (last.index, last.term)
        } else {
            (0, 0)
        }
    }

    pub async fn handle_request_vote(&self, args: RequestVoteArgs) -> RequestVoteReply {
        let mut current_term = self.current_term.load(Ordering::SeqCst);

        if args.term > current_term {
            self.current_term.store(args.term, Ordering::SeqCst);
            *self.role.write().await = NodeRole::Follower;
            *self.voted_for.write().await = None;
            *self.leader_id.write().await = None;
            current_term = args.term;
            self.persist_state(current_term, None);
        }

        if args.term < current_term {
            return RequestVoteReply {
                term: current_term,
                vote_granted: false,
            };
        }

        let voted_for = self.voted_for.read().await.clone();
        let can_vote = voted_for.is_none() || voted_for == Some(args.candidate_id.clone());

        let (last_idx, last_term) = self.get_last_log_index_and_term().await;
        let log_up_to_date = args.last_log_term > last_term
            || (args.last_log_term == last_term && args.last_log_index >= last_idx);

        if can_vote && log_up_to_date {
            *self.voted_for.write().await = Some(args.candidate_id.clone());
            self.persist_state(current_term, Some(args.candidate_id));
            *self.last_heartbeat_time.lock().await = Instant::now();
            RequestVoteReply {
                term: current_term,
                vote_granted: true,
            }
        } else {
            RequestVoteReply {
                term: current_term,
                vote_granted: false,
            }
        }
    }

    pub async fn handle_append_entries(&self, args: AppendEntriesArgs) -> AppendEntriesReply {
        let mut current_term = self.current_term.load(Ordering::SeqCst);

        if args.term > current_term {
            self.current_term.store(args.term, Ordering::SeqCst);
            *self.role.write().await = NodeRole::Follower;
            *self.voted_for.write().await = None;
            current_term = args.term;
            self.persist_state(current_term, None);
        }

        if args.term < current_term {
            return AppendEntriesReply {
                term: current_term,
                success: false,
                match_index: 0,
            };
        }

        // Valid leader heartbeat/append
        *self.last_heartbeat_time.lock().await = Instant::now();
        *self.leader_id.write().await = Some(args.leader_id.clone());
        *self.role.write().await = NodeRole::Follower;

        let mut log = self.log.write().await;

        // Check if prev_log_index matches only when there are entries to append
        if !args.entries.is_empty() && args.prev_log_index > 0 {
            if args.prev_log_index > log.len() as u64 {
                return AppendEntriesReply {
                    term: current_term,
                    success: false,
                    match_index: log.len() as u64,
                };
            }
            if log[(args.prev_log_index - 1) as usize].term != args.prev_log_term {
                return AppendEntriesReply {
                    term: current_term,
                    success: false,
                    match_index: args.prev_log_index - 1,
                };
            }
        }

        // Insert new entries
        let mut insert_idx = args.prev_log_index as usize;
        for entry in args.entries {
            if insert_idx < log.len() {
                if log[insert_idx].term != entry.term {
                    log.truncate(insert_idx);
                    log.push(entry.clone());
                    let _ = self.storage.set("_raft_log", &entry.index.to_string(), &entry);
                }
            } else {
                log.push(entry.clone());
                let _ = self.storage.set("_raft_log", &entry.index.to_string(), &entry);
            }
            insert_idx += 1;
        }
        let _ = self.storage.set("_raft_meta", "log_count", &(log.len() as u64));

        let match_index = log.len() as u64;

        if args.leader_commit > self.commit_index.load(Ordering::SeqCst) {
            let new_commit = std::cmp::min(args.leader_commit, match_index);
            self.commit_index.store(new_commit, Ordering::SeqCst);
        }

        AppendEntriesReply {
            term: current_term,
            success: true,
            match_index,
        }
    }

    pub async fn propose(&self, command: ClusterCommand) -> Result<u64> {
        let role = self.get_role().await;
        if role != NodeRole::Leader {
            if let Some(leader) = self.get_leader_id().await {
                bail!("Not the leader. Current leader is {}", leader);
            } else {
                bail!("No leader currently elected in cluster");
            }
        }

        let term = self.get_current_term();
        let index = {
            let mut log = self.log.write().await;
            let index = log.len() as u64 + 1;
            let entry = RaftLogEntry {
                term,
                index,
                command,
            };
            log.push(entry.clone());
            let _ = self.storage.set("_raft_log", &index.to_string(), &entry);
            let _ = self.storage.set("_raft_meta", "log_count", &index);
            index
        };

        // If single-node cluster (no peers), immediately commit
        let peers = self.get_peers().await;
        if peers.is_empty() {
            self.commit_index.store(index, Ordering::SeqCst);
            return Ok(index);
        }

        // Replicate to followers
        let quorum = (peers.len() + 1) / 2 + 1;
        let mut match_count = 1; // Self matches

        let (prev_idx, prev_term) = {
            let log = self.log.read().await;
            if index > 1 {
                (index - 1, log[(index - 2) as usize].term)
            } else {
                (0, 0)
            }
        };

        let entry = {
            let log = self.log.read().await;
            log[(index - 1) as usize].clone()
        };

        for (_peer_id, peer_addr) in &peers {
            let args = AppendEntriesArgs {
                term,
                leader_id: self.node_id.clone(),
                prev_log_index: prev_idx,
                prev_log_term: prev_term,
                entries: vec![entry.clone()],
                leader_commit: self.commit_index.load(Ordering::SeqCst),
            };

            let url = format!("http://{}/raft/append_entries", peer_addr);
            if let Ok(resp) = self.http_client.post(&url).json(&args).send().await {
                if let Ok(reply) = resp.json::<AppendEntriesReply>().await {
                    if reply.success {
                        match_count += 1;
                    }
                }
            }
        }

        if match_count >= quorum {
            self.commit_index.store(index, Ordering::SeqCst);
            // Immediately broadcast commit update to all followers
            for (_peer_id, peer_addr) in &peers {
                let args = AppendEntriesArgs {
                    term,
                    leader_id: self.node_id.clone(),
                    prev_log_index: index,
                    prev_log_term: term,
                    entries: Vec::new(),
                    leader_commit: index,
                };
                let client = self.http_client.clone();
                let url = format!("http://{}/raft/append_entries", peer_addr);
                tokio::spawn(async move {
                    let _ = client.post(&url).json(&args).send().await;
                });
            }
            Ok(index)
        } else {
            warn!("Quorum not reached for proposal index {}: {}/{}", index, match_count, quorum);
            Ok(index)
        }
    }

    pub async fn start_election(&self) {
        let new_term = self.current_term.fetch_add(1, Ordering::SeqCst) + 1;
        *self.role.write().await = NodeRole::Candidate;
        *self.voted_for.write().await = Some(self.node_id.clone());
        *self.leader_id.write().await = None;
        self.persist_state(new_term, Some(self.node_id.clone()));

        info!("Node {} starting election for term {}", self.node_id, new_term);

        let peers = self.get_peers().await;
        if peers.is_empty() {
            // Single node auto-promotes to leader
            *self.role.write().await = NodeRole::Leader;
            *self.leader_id.write().await = Some(self.node_id.clone());
            info!("Node {} became leader (single-node cluster)", self.node_id);
            return;
        }

        let (last_idx, last_term) = self.get_last_log_index_and_term().await;
        let args = RequestVoteArgs {
            term: new_term,
            candidate_id: self.node_id.clone(),
            last_log_index: last_idx,
            last_log_term: last_term,
        };

        let mut votes = 1; // Self vote
        let quorum = (peers.len() + 1) / 2 + 1;

        for (peer_id, peer_addr) in peers {
            let url = format!("http://{}/raft/request_vote", peer_addr);
            match self.http_client.post(&url).json(&args).send().await {
                Ok(resp) => {
                    if let Ok(reply) = resp.json::<RequestVoteReply>().await {
                        if reply.term > new_term {
                            self.current_term.store(reply.term, Ordering::SeqCst);
                            *self.role.write().await = NodeRole::Follower;
                            *self.voted_for.write().await = None;
                            self.persist_state(reply.term, None);
                            return;
                        }
                        if reply.vote_granted {
                            votes += 1;
                        }
                    }
                }
                Err(e) => {
                    debug!("Failed to request vote from peer {} ({}): {}", peer_id, peer_addr, e);
                }
            }
        }

        if votes >= quorum && *self.role.read().await == NodeRole::Candidate {
            *self.role.write().await = NodeRole::Leader;
            *self.leader_id.write().await = Some(self.node_id.clone());
            info!("Node {} elected as LEADER for term {} with {}/{} votes", self.node_id, new_term, votes, quorum);
            // Propose Noop to assert leadership
            let _ = self.propose(ClusterCommand::Noop).await;
        } else {
            *self.role.write().await = NodeRole::Follower;
            info!("Node {} failed to win election (got {}/{} votes)", self.node_id, votes, quorum);
        }
    }

    pub async fn send_heartbeats(&self) {
        if self.get_role().await != NodeRole::Leader {
            return;
        }

        let term = self.get_current_term();
        let (last_idx, last_term) = self.get_last_log_index_and_term().await;
        let leader_commit = self.commit_index.load(Ordering::SeqCst);
        let peers = self.get_peers().await;

        for (_peer_id, peer_addr) in peers {
            let args = AppendEntriesArgs {
                term,
                leader_id: self.node_id.clone(),
                prev_log_index: last_idx,
                prev_log_term: last_term,
                entries: Vec::new(), // Heartbeat contains empty entries
                leader_commit,
            };

            let url = format!("http://{}/raft/append_entries", peer_addr);
            let client = self.http_client.clone();
            tokio::spawn(async move {
                let _ = client.post(&url).json(&args).send().await;
            });
        }
    }

    pub async fn get_pending_applied_entries(&self) -> Vec<RaftLogEntry> {
        let commit = self.commit_index.load(Ordering::SeqCst);
        let mut last = self.last_applied.load(Ordering::SeqCst);
        let mut pending = Vec::new();

        if commit > last {
            let log = self.log.read().await;
            while last < commit && (last as usize) < log.len() {
                pending.push(log[last as usize].clone());
                last += 1;
            }
            self.last_applied.store(last, Ordering::SeqCst);
        }
        pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_storage(name: &str) -> StorageEngine {
        let temp_dir = format!("test_data_raft_{}", name);
        let _ = std::fs::remove_dir_all(&temp_dir);
        StorageEngine::new(&temp_dir).expect("Failed to create test storage")
    }

    #[tokio::test]
    async fn test_raft_voting_logic() {
        let storage = create_test_storage("voting");
        let raft = RaftEngine::new("node-1".to_string(), "127.0.0.1:4650".to_string(), storage);

        // Candidate with term 1 requests vote
        let req = RequestVoteArgs {
            term: 1,
            candidate_id: "node-2".to_string(),
            last_log_index: 0,
            last_log_term: 0,
        };

        let reply = raft.handle_request_vote(req).await;
        assert!(reply.vote_granted, "Should grant vote for first candidate in term 1");
        assert_eq!(reply.term, 1);

        // Another candidate for term 1 requests vote -> rejected (already voted for node-2)
        let req2 = RequestVoteArgs {
            term: 1,
            candidate_id: "node-3".to_string(),
            last_log_index: 0,
            last_log_term: 0,
        };
        let reply2 = raft.handle_request_vote(req2).await;
        assert!(!reply2.vote_granted, "Should reject second candidate in same term");

        // Cleanup
        let _ = std::fs::remove_dir_all("test_data_raft_voting");
    }

    #[tokio::test]
    async fn test_raft_append_entries_and_commit() {
        let storage = create_test_storage("append");
        let raft = RaftEngine::new("node-2".to_string(), "127.0.0.1:4651".to_string(), storage);

        let entry = RaftLogEntry {
            term: 1,
            index: 1,
            command: ClusterCommand::ExecuteSql {
                query: "CREATE TABLE test (id INT);".to_string(),
                database: Some("default".to_string()),
            },
        };

        let args = AppendEntriesArgs {
            term: 1,
            leader_id: "node-1".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![entry],
            leader_commit: 1,
        };

        let reply = raft.handle_append_entries(args).await;
        assert!(reply.success, "AppendEntries should succeed");
        assert_eq!(reply.match_index, 1);
        assert_eq!(raft.get_commit_index(), 1);

        // Verify applied entries
        let pending = raft.get_pending_applied_entries().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].index, 1);

        // Cleanup
        let _ = std::fs::remove_dir_all("test_data_raft_append");
    }

    #[tokio::test]
    async fn test_raft_single_node_proposal() {
        let storage = create_test_storage("single");
        let raft = RaftEngine::new("node-1".to_string(), "127.0.0.1:4650".to_string(), storage);

        // Single node becomes leader automatically on election
        raft.start_election().await;
        assert_eq!(raft.get_role().await, NodeRole::Leader);

        let proposal_idx = raft.propose(ClusterCommand::ExecuteSql {
            query: "INSERT INTO users VALUES (1);".to_string(),
            database: None,
        }).await.expect("Proposal should succeed");

        assert!(proposal_idx >= 1);
        assert_eq!(raft.get_commit_index(), proposal_idx);

        // Cleanup
        let _ = std::fs::remove_dir_all("test_data_raft_single");
    }
}
