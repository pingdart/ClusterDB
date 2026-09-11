pub mod types;
pub mod distance;
pub mod hnsw;

use crate::storage::StorageEngine;
use anyhow::Result;
pub use distance::{compute_distance, compute_similarity, cosine_similarity, euclidean_distance, dot_product};
pub use hnsw::{HnswIndex, VectorSearchResult};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
pub use types::{Vector, VectorMetric, VALID_DIMENSIONS};

#[derive(Clone)]
pub struct VectorManager {
    storage: StorageEngine,
    /// In-memory cache of active HNSW indexes: table_column -> Arc<RwLock<HnswIndex>>
    indexes: Arc<RwLock<HashMap<String, Arc<RwLock<HnswIndex>>>>>,
}

impl VectorManager {
    pub fn new(storage: StorageEngine) -> Self {
        Self {
            storage,
            indexes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn index_key(table: &str, column: &str) -> String {
        format!("{}__{}", table, column)
    }

    /// Loads or creates an HNSW index for a table's vector column
    pub fn get_or_create_index(
        &self,
        table: &str,
        column: &str,
        dimension: usize,
        metric: VectorMetric,
    ) -> Result<Arc<RwLock<HnswIndex>>> {
        let key = Self::index_key(table, column);
        
        {
            let read_guard = self.indexes.read().unwrap();
            if let Some(idx) = read_guard.get(&key) {
                return Ok(idx.clone());
            }
        }

        // Check if persistent snapshot exists in Sled tree `_hnsw_indexes`
        let existing: Option<HnswIndex> = self.storage.get("_hnsw_indexes", &key)?;
        let index = match existing {
            Some(mut idx) => {
                idx.dimension = dimension;
                idx.metric = metric;
                idx
            }
            None => HnswIndex::new(dimension, metric),
        };

        let arc_idx = Arc::new(RwLock::new(index));
        let mut write_guard = self.indexes.write().unwrap();
        write_guard.insert(key, arc_idx.clone());

        Ok(arc_idx)
    }

    /// Inserts or updates a vector into the index and persists it
    pub fn insert_vector(
        &self,
        table: &str,
        column: &str,
        doc_id: &str,
        vector: &[f32],
    ) -> Result<()> {
        let key = Self::index_key(table, column);
        let index_arc = {
            let read_guard = self.indexes.read().unwrap();
            if let Some(idx) = read_guard.get(&key) {
                idx.clone()
            } else {
                drop(read_guard);
                self.get_or_create_index(table, column, vector.len(), VectorMetric::Cosine)?
            }
        };

        {
            let mut index = index_arc.write().unwrap();
            index.insert(doc_id.to_string(), vector.to_vec())
                .map_err(|e| anyhow::anyhow!(e))?;
            // Persist to Sled
            self.storage.set("_hnsw_indexes", &key, &*index)?;
        }

        Ok(())
    }

    /// Searches for nearest neighbors in the HNSW index
    pub fn search(
        &self,
        table: &str,
        column: &str,
        query_vector: &[f32],
        top_k: usize,
    ) -> Result<Vec<VectorSearchResult>> {
        let key = Self::index_key(table, column);
        let index_arc = {
            let read_guard = self.indexes.read().unwrap();
            if let Some(idx) = read_guard.get(&key) {
                idx.clone()
            } else {
                drop(read_guard);
                self.get_or_create_index(table, column, query_vector.len(), VectorMetric::Cosine)?
            }
        };

        let index = index_arc.read().unwrap();
        Ok(index.search(query_vector, top_k))
    }

    /// Removes a vector when a row/document is deleted
    pub fn delete_vector(&self, table: &str, column: &str, doc_id: &str) -> Result<bool> {
        let key = Self::index_key(table, column);
        let index_arc = {
            let read_guard = self.indexes.read().unwrap();
            match read_guard.get(&key) {
                Some(idx) => idx.clone(),
                None => return Ok(false),
            }
        };

        let mut index = index_arc.write().unwrap();
        let deleted = index.delete(doc_id);
        if deleted {
            self.storage.set("_hnsw_indexes", &key, &*index)?;
        }
        Ok(deleted)
    }
}
