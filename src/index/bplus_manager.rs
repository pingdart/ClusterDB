use crate::index::btree::{BPlusTree, CompositeKey, IndexValue};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuplePointer {
    pub page_id: u32,
    pub slot_id: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexDescriptor {
    pub table_name: String,
    pub index_name: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
    pub is_clustered: bool,
}

pub struct BPlusIndexManager {
    descriptors: RwLock<HashMap<String, IndexDescriptor>>, // index_name -> descriptor
    // Index storage: index_name -> BPlusTree<CompositeKey, Vec<TuplePointer>>
    trees: RwLock<HashMap<String, BPlusTree<CompositeKey, Vec<TuplePointer>>>>,
}

impl BPlusIndexManager {
    pub fn new() -> Self {
        Self {
            descriptors: RwLock::new(HashMap::new()),
            trees: RwLock::new(HashMap::new()),
        }
    }

    /// Registers a new B+Tree index (Clustered or Secondary, Unique or Non-Unique, Single or Composite)
    pub fn create_btree_index(
        &self,
        table_name: &str,
        index_name: &str,
        columns: Vec<String>,
        is_unique: bool,
        is_clustered: bool,
    ) -> Result<()> {
        let mut desc_map = self.descriptors.write().unwrap();
        if desc_map.contains_key(index_name) {
            bail!("Index '{}' already exists", index_name);
        }

        let desc = IndexDescriptor {
            table_name: table_name.to_string(),
            index_name: index_name.to_string(),
            columns,
            is_unique,
            is_clustered,
        };

        desc_map.insert(index_name.to_string(), desc);
        let mut tree_map = self.trees.write().unwrap();
        tree_map.insert(index_name.to_string(), BPlusTree::new());

        Ok(())
    }

    /// Inserts an entry into the B+Tree index.
    /// Strictly verifies uniqueness constraints if configured.
    pub fn insert_entry(
        &self,
        index_name: &str,
        key: CompositeKey,
        ptr: TuplePointer,
    ) -> Result<()> {
        let desc = {
            let desc_map = self.descriptors.read().unwrap();
            desc_map.get(index_name).cloned().context("Index descriptor not found")?
        };

        let mut tree_map = self.trees.write().unwrap();
        let tree = tree_map.get_mut(index_name).context("Index B+Tree not found")?;

        if desc.is_unique {
            if let Some(existing) = tree.get(&key) {
                if !existing.is_empty() {
                    bail!("Unique constraint violation: key {:?} already exists in index '{}'", key, index_name);
                }
            }
            tree.insert(key, vec![ptr])?;
        } else {
            let mut ptrs = tree.get(&key).cloned().unwrap_or_default();
            ptrs.push(ptr);
            tree.insert(key, ptrs)?;
        }

        Ok(())
    }

    /// Index Point Scan: O(log_B N)
    pub fn point_lookup(&self, index_name: &str, key: &CompositeKey) -> Option<Vec<TuplePointer>> {
        let tree_map = self.trees.read().unwrap();
        let tree = tree_map.get(index_name)?;
        tree.get(key).cloned()
    }

    /// Range Scan: Seeks to start leaf in O(log_B N), then scans sibling leaves directly in O(K)
    pub fn range_scan(
        &self,
        index_name: &str,
        start: Option<&CompositeKey>,
        end: Option<&CompositeKey>,
    ) -> Vec<(CompositeKey, Vec<TuplePointer>)> {
        let tree_map = self.trees.read().unwrap();
        let tree = match tree_map.get(index_name) {
            Some(t) => t,
            None => return Vec::new(),
        };

        tree.range_scan(start, end)
            .into_iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Index-Only Scan: Satisfies the query strictly from index composite key values
    /// without ever reading the underlying heap/table data pages.
    pub fn index_only_scan(
        &self,
        index_name: &str,
        target_columns: &[&str],
        start: Option<&CompositeKey>,
        end: Option<&CompositeKey>,
    ) -> Result<Vec<HashMap<String, String>>> {
        let desc = {
            let desc_map = self.descriptors.read().unwrap();
            desc_map.get(index_name).cloned().context("Index not found")?
        };

        // Verify that all target columns exist in the index
        for &tc in target_columns {
            if !desc.columns.iter().any(|c| c == tc) {
                bail!("Index-only scan not possible: column '{}' is not part of index '{}'", tc, index_name);
            }
        }

        let scanned_entries = self.range_scan(index_name, start, end);
        let mut results = Vec::new();

        for (comp_key, _) in scanned_entries {
            let mut row = HashMap::new();
            for (col_idx, col_name) in desc.columns.iter().enumerate() {
                if target_columns.contains(&col_name.as_str()) {
                    if let Some(val) = comp_key.values.get(col_idx) {
                        let str_val = match val {
                            IndexValue::Int(n) => n.to_string(),
                            IndexValue::Float(f) => f.0.to_string(),
                            IndexValue::Text(s) => s.clone(),
                            IndexValue::Bytes(b) => hex::encode(b),
                        };
                        row.insert(col_name.clone(), str_val);
                    }
                }
            }
            results.push(row);
        }

        Ok(results)
    }

    pub fn get_descriptor(&self, index_name: &str) -> Option<IndexDescriptor> {
        let desc_map = self.descriptors.read().unwrap();
        desc_map.get(index_name).cloned()
    }
}

impl Default for BPlusIndexManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bplus_unique_and_secondary_index() {
        let mgr = BPlusIndexManager::new();

        // 1. Create unique index on users(email)
        mgr.create_btree_index("users", "idx_users_email", vec!["email".into()], true, false).unwrap();

        let k1 = CompositeKey::single_text("alice@example.com");
        let ptr1 = TuplePointer { page_id: 1, slot_id: 0 };
        mgr.insert_entry("idx_users_email", k1.clone(), ptr1).unwrap();

        // Point lookup
        let found = mgr.point_lookup("idx_users_email", &k1).unwrap();
        assert_eq!(found, vec![ptr1]);

        // Duplicate insert must fail with unique constraint violation
        let ptr2 = TuplePointer { page_id: 2, slot_id: 1 };
        let dup_err = mgr.insert_entry("idx_users_email", k1.clone(), ptr2);
        assert!(dup_err.is_err());
        assert!(dup_err.unwrap_err().to_string().contains("Unique constraint violation"));
    }

    #[test]
    fn test_bplus_composite_range_and_index_only_scan() {
        let mgr = BPlusIndexManager::new();

        // Composite index on orders(dept, amount)
        mgr.create_btree_index(
            "orders",
            "idx_orders_dept_amt",
            vec!["dept".into(), "amount".into()],
            false,
            false,
        ).unwrap();

        for i in 1..=20 {
            let key = CompositeKey::new(vec![
                IndexValue::Text("Finance".into()),
                IndexValue::Int(i * 100),
            ]);
            mgr.insert_entry("idx_orders_dept_amt", key, TuplePointer { page_id: 5, slot_id: i as u16 }).unwrap();
        }

        // Range scan: Finance orders between 500 and 900
        let start = CompositeKey::new(vec![IndexValue::Text("Finance".into()), IndexValue::Int(500)]);
        let end = CompositeKey::new(vec![IndexValue::Text("Finance".into()), IndexValue::Int(900)]);

        let range = mgr.range_scan("idx_orders_dept_amt", Some(&start), Some(&end));
        assert_eq!(range.len(), 5);

        // Index-only scan: read directly from index without touching data pages
        let index_only_res = mgr.index_only_scan(
            "idx_orders_dept_amt",
            &["amount"],
            Some(&start),
            Some(&end),
        ).unwrap();

        assert_eq!(index_only_res.len(), 5);
        assert_eq!(index_only_res[0].get("amount"), Some(&"500".to_string()));
        assert_eq!(index_only_res[4].get("amount"), Some(&"900".to_string()));
    }
}
