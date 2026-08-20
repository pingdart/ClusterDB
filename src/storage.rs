use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct StorageEngine {
    db: sled::Db,
}

impl StorageEngine {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = sled::Config::new()
            .path(path)
            .cache_capacity(1_024 * 1_024 * 1_024) // 1GB Memory Cache for O(1) Lookups
            .open()
            .context("Failed to open sled database")?;
        Ok(Self { db })
    }

    pub fn new_temp() -> Result<Self> {
        let db = sled::Config::new()
            .temporary(true)
            .open()
            .context("Failed to open temporary sled database")?;
        Ok(Self { db })
    }

    pub fn tree_names(&self) -> Vec<String> {
        self.db.tree_names()
            .into_iter()
            .filter_map(|v| String::from_utf8(v.to_vec()).ok())
            .collect()
    }

    /// Sets a key-value pair in a specific collection/table (tree)
    pub fn set<T: Serialize>(&self, table: &str, key: &str, value: &T) -> Result<()> {
        let tree = self.db.open_tree(table)?;
        let encoded: Vec<u8> = bincode::serialize(value).context("Failed to serialize value")?;
        tree.insert(key.as_bytes(), encoded)?;
        Ok(())
    }

    /// Gets a key-value pair from a specific collection/table
    pub fn get<T: for<'de> Deserialize<'de>>(&self, table: &str, key: &str) -> Result<Option<T>> {
        let tree = self.db.open_tree(table)?;
        if let Some(bytes) = tree.get(key.as_bytes())? {
            let decoded: T = bincode::deserialize(&bytes).context("Failed to deserialize value")?;
            Ok(Some(decoded))
        } else {
            Ok(None)
        }
    }

    pub fn get_all<T: for<'de> Deserialize<'de>>(&self, table: &str) -> Result<Vec<T>> {
        let tree = self.db.open_tree(table)?;
        let mut results = Vec::new();
        for item in tree.iter() {
            let (_, bytes) = item?;
            let decoded: T = bincode::deserialize(&bytes).context("Failed to deserialize value")?;
            results.push(decoded);
        }
        Ok(results)
    }

    pub fn get_all_with_keys<T: for<'de> Deserialize<'de>>(&self, table: &str) -> Result<Vec<(String, T)>> {
        let tree = self.db.open_tree(table)?;
        let mut results = Vec::new();
        for item in tree.iter() {
            let (key, bytes) = item?;
            let decoded: T = bincode::deserialize(&bytes).context("Failed to deserialize value")?;
            if let Ok(k) = String::from_utf8(key.to_vec()) {
                results.push((k, decoded));
            }
        }
        Ok(results)
    }

    pub fn iter<T: for<'de> Deserialize<'de>>(&self, table: &str) -> Result<impl Iterator<Item = Result<T>>> {
        let tree = self.db.open_tree(table)?;
        let iter = tree.into_iter().map(|res| {
            let (_, bytes) = res.context("Failed to read from tree")?;
            let decoded: T = bincode::deserialize(&bytes).context("Failed to deserialize value")?;
            Ok(decoded)
        });
        Ok(iter)
    }

    pub fn get_keys_by_prefix(&self, table: &str, prefix: &str) -> Result<Vec<String>> {
        let tree = self.db.open_tree(table)?;
        let mut results = Vec::new();
        for item in tree.scan_prefix(prefix.as_bytes()) {
            let (key, _) = item?;
            if let Ok(k) = String::from_utf8(key.to_vec()) {
                results.push(k);
            }
        }
        Ok(results)
    }

    /// Deletes a key from a specific table
    pub fn delete(&self, table: &str, key: &str) -> Result<()> {
        let tree = self.db.open_tree(table)?;
        tree.remove(key.as_bytes())?;
        Ok(())
    }

    /// Gets the number of rows (items) in a specific tree
    pub fn count_rows(&self, table: &str) -> Result<usize> {
        let tree = self.db.open_tree(table)?;
        Ok(tree.len())
    }

    /// Drops an entire tree
    pub fn drop_tree(&self, table: &str) -> Result<bool> {
        self.db.drop_tree(table).context("Failed to drop tree")
    }

    /// Flushes all changes to disk
    pub fn flush(&self) -> Result<usize> {
        self.db.flush().context("Failed to flush sled database")
    }
}
