pub mod tokenizer;
pub mod bm25;
pub mod hybrid;

use crate::storage::StorageEngine;
use anyhow::Result;
pub use bm25::{InvertedIndex, SearchHit};
pub use hybrid::{combine_hybrid_scores, HybridSearchResult};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct SearchManager {
    storage: StorageEngine,
    indexes: Arc<RwLock<HashMap<String, Arc<RwLock<InvertedIndex>>>>>,
}

impl SearchManager {
    pub fn new(storage: StorageEngine) -> Self {
        Self {
            storage,
            indexes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn index_key(table: &str, column: &str) -> String {
        format!("{}__{}", table, column)
    }

    pub fn get_or_create_index(&self, table: &str, column: &str) -> Result<Arc<RwLock<InvertedIndex>>> {
        let key = Self::index_key(table, column);
        {
            let read_guard = self.indexes.read().unwrap();
            if let Some(idx) = read_guard.get(&key) {
                return Ok(idx.clone());
            }
        }

        let existing: Option<InvertedIndex> = self.storage.get("_bm25_indexes", &key)?;
        let index = existing.unwrap_or_else(|| InvertedIndex::new(key.clone()));

        let arc_idx = Arc::new(RwLock::new(index));
        let mut write_guard = self.indexes.write().unwrap();
        write_guard.insert(key, arc_idx.clone());

        Ok(arc_idx)
    }

    pub fn index_document(&self, table: &str, column: &str, doc_id: &str, text: &str) -> Result<()> {
        let key = Self::index_key(table, column);
        let index_arc = self.get_or_create_index(table, column)?;
        {
            let mut index = index_arc.write().unwrap();
            index.add_document(doc_id, text);
            self.storage.set("_bm25_indexes", &key, &*index)?;
        }
        Ok(())
    }

    pub fn delete_document(&self, table: &str, column: &str, doc_id: &str) -> Result<()> {
        let key = Self::index_key(table, column);
        let index_arc = self.get_or_create_index(table, column)?;
        {
            let mut index = index_arc.write().unwrap();
            index.remove_document(doc_id);
            self.storage.set("_bm25_indexes", &key, &*index)?;
        }
        Ok(())
    }

    pub fn search(&self, table: &str, column: &str, query: &str, top_k: usize) -> Result<Vec<SearchHit>> {
        let index_arc = self.get_or_create_index(table, column)?;
        let index = index_arc.read().unwrap();
        Ok(index.search(query, top_k))
    }
}
