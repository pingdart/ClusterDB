use crate::storage::StorageEngine;
use anyhow::Result;
use std::collections::HashSet;

#[derive(Clone)]
pub struct IndexManager {
    storage: StorageEngine,
}

impl IndexManager {
    pub fn new(storage: StorageEngine) -> Self {
        Self { storage }
    }

    /// Register a new index on a specific table and fields
    pub fn create_index(&self, table_name: &str, fields: &[&str]) -> Result<()> {
        let field_key = fields.join("|");
        let index_meta_key = format!("{}_{}", table_name, field_key);
        self.storage.set("_index_meta", &index_meta_key, &fields.iter().map(|s| s.to_string()).collect::<Vec<String>>())?;
        
        let storage_clone = self.storage.clone();
        let table_name_clone = table_name.to_string();
        let fields_clone: Vec<String> = fields.iter().map(|s| s.to_string()).collect();
        let field_key_clone = field_key.clone();
        
        // Spawn background index builder
        tokio::spawn(async move {
            let index_tree_name = format!("idx_{}_{}", table_name_clone, field_key_clone);
            if let Ok(iter) = storage_clone.iter::<crate::exec::RowData>(&table_name_clone) {
                for res in iter {
                    if let Ok(row) = res {
                        let mut vals = Vec::new();
                        let mut valid = true;
                        for f in &fields_clone {
                            if let Some(v) = row.columns.get(f) {
                                vals.push(v.clone());
                            } else {
                                valid = false;
                                break;
                            }
                        }
                        if valid {
                            let combined_val = vals.join("|");
                            let mut ids: HashSet<String> = storage_clone.get(&index_tree_name, &combined_val).unwrap_or_default().unwrap_or_default();
                            ids.insert(row._id.clone());
                            let _ = storage_clone.set(&index_tree_name, &combined_val, &ids);
                        }
                    }
                }
            }
            let _ = storage_clone.flush();
            tracing::info!("Background indexing completed for {}({})", table_name_clone, field_key_clone);
        });
        
        self.storage.flush()?;
        Ok(())
    }

    /// Check if an index exists for a given set of fields
    pub fn has_index(&self, table_name: &str, fields: &[&str]) -> bool {
        let field_key = fields.join("|");
        let index_meta_key = format!("{}_{}", table_name, field_key);
        if let Ok(Some(_)) = self.storage.get::<Vec<String>>("_index_meta", &index_meta_key) {
            return true;
        }
        false
    }
    
    /// Get all registered indexes for a table
    pub fn get_table_indexes(&self, table_name: &str) -> Result<Vec<Vec<String>>> {
        let prefix = format!("{}_", table_name);
        let keys = self.storage.get_keys_by_prefix("_index_meta", &prefix)?;
        let mut indexes = Vec::new();
        for key in keys {
            if let Ok(Some(fields)) = self.storage.get::<Vec<String>>("_index_meta", &key) {
                indexes.push(fields);
            }
        }
        Ok(indexes)
    }

    /// Drop an index
    pub fn drop_index(&self, table_name: &str, fields: &[&str]) -> Result<()> {
        let field_key = fields.join("|");
        let index_meta_key = format!("{}_{}", table_name, field_key);
        self.storage.delete("_index_meta", &index_meta_key)?;
        
        let index_tree_name = format!("idx_{}_{}", table_name, field_key);
        let _ = self.storage.drop_tree(&index_tree_name);
        
        self.storage.flush()?;
        Ok(())
    }

    /// Insert or update a document in the index
    pub fn update_document(&self, table_name: &str, doc_id: &str, fields: &[&str], new_val: &str, old_val: Option<&str>) -> Result<()> {
        let field_key = fields.join("|");
        let index_tree_name = format!("idx_{}_{}", table_name, field_key);
        
        // Remove old value if it exists
        if let Some(old) = old_val {
            if old != new_val {
                let mut ids: HashSet<String> = self.storage.get(&index_tree_name, old)?.unwrap_or_default();
                ids.remove(doc_id);
                if ids.is_empty() {
                    self.storage.delete(&index_tree_name, old)?;
                } else {
                    self.storage.set(&index_tree_name, old, &ids)?;
                }
            } else {
                // Value didn't change, no need to update index
                return Ok(());
            }
        }

        // Add new value
        let mut ids: HashSet<String> = self.storage.get(&index_tree_name, new_val)?.unwrap_or_default();
        ids.insert(doc_id.to_string());
        self.storage.set(&index_tree_name, new_val, &ids)?;

        Ok(())
    }

    /// Remove a document from the index
    pub fn remove_document(&self, table_name: &str, doc_id: &str, fields: &[&str], val: &str) -> Result<()> {
        let field_key = fields.join("|");
        let index_tree_name = format!("idx_{}_{}", table_name, field_key);
        let mut ids: HashSet<String> = self.storage.get(&index_tree_name, val)?.unwrap_or_default();
        ids.remove(doc_id);
        if ids.is_empty() {
            self.storage.delete(&index_tree_name, val)?;
        } else {
            self.storage.set(&index_tree_name, val, &ids)?;
        }
        Ok(())
    }

    /// Get all document IDs that match a specific indexed field value
    pub fn get_ids(&self, table_name: &str, fields: &[&str], value: &str) -> Result<Vec<String>> {
        let field_key = fields.join("|");
        let index_tree_name = format!("idx_{}_{}", table_name, field_key);
        let ids: HashSet<String> = self.storage.get(&index_tree_name, value)?.unwrap_or_default();
        Ok(ids.into_iter().collect())
    }
}
