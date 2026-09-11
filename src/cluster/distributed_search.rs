use crate::cluster::metadata::ClusterMetadataService;
use crate::exec::Executor;
use crate::vector::hnsw::VectorSearchResult;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardVectorSearchRequest {
    pub database: String,
    pub table: String,
    pub column: String,
    pub vector: Vec<f32>,
    pub top_k: usize,
}

pub struct DistributedSearchCoordinator {
    executor: Arc<Executor>,
    metadata: Arc<ClusterMetadataService>,
    http_client: reqwest::Client,
}

impl DistributedSearchCoordinator {
    pub fn new(executor: Arc<Executor>, metadata: Arc<ClusterMetadataService>) -> Self {
        Self {
            executor,
            metadata,
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_millis(1500))
                .build()
                .unwrap_or_default(),
        }
    }

    pub async fn scatter_gather_vector_search(
        &self,
        database: &str,
        table: &str,
        column: &str,
        vector: &[f32],
        top_k: usize,
    ) -> Result<Vec<VectorSearchResult>> {
        let full_table_name = format!("{}_{}", database, table);

        // 1. Run local search on this node's shard
        let mut local_results = self
            .executor
            .vector_manager
            .search(&full_table_name, column, vector, top_k)
            .unwrap_or_default();

        // 2. Fetch all online cluster nodes
        let nodes = self.metadata.get_all_nodes().await;
        if nodes.len() <= 1 {
            return Ok(local_results);
        }

        let shard_req = ShardVectorSearchRequest {
            database: database.to_string(),
            table: table.to_string(),
            column: column.to_string(),
            vector: vector.to_vec(),
            top_k,
        };

        // 3. Scatter query to remote peer nodes concurrently
        let mut tasks = Vec::new();
        for node in nodes {
            // Skip searching our own address twice
            if node.id == self.metadata.cluster_id || node.address.is_empty() {
                continue;
            }

            let client = self.http_client.clone();
            let req_body = shard_req.clone();
            let target_url = format!("http://{}/cluster/shard/vector_search", node.address);

            tasks.push(tokio::spawn(async move {
                match client.post(&target_url).json(&req_body).send().await {
                    Ok(resp) => {
                        if let Ok(results) = resp.json::<Vec<VectorSearchResult>>().await {
                            Some(results)
                        } else {
                            None
                        }
                    }
                    Err(e) => {
                        debug!("Scatter search to {} failed: {}", target_url, e);
                        None
                    }
                }
            }));
        }

        // 4. Gather candidate results from all shards
        let mut gathered_results = Vec::new();
        gathered_results.append(&mut local_results);

        for t in tasks {
            if let Ok(Some(res)) = t.await {
                gathered_results.extend(res);
            }
        }

        // 5. Merge Top-K: deduplicate by ID and sort descending by similarity
        let mut best_by_id: HashMap<String, VectorSearchResult> = HashMap::new();
        for item in gathered_results {
            best_by_id
                .entry(item.id.clone())
                .and_modify(|e| {
                    if item.similarity > e.similarity {
                        *e = item.clone();
                    }
                })
                .or_insert(item);
        }

        let mut merged: Vec<VectorSearchResult> = best_by_id.into_values().collect();
        merged.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap_or(std::cmp::Ordering::Equal));
        merged.truncate(top_k);

        Ok(merged)
    }
}
