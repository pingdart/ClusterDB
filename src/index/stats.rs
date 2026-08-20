use crate::storage::StorageEngine;
use anyhow::Result;


#[derive(Clone)]
pub struct IndexStats {
    storage: StorageEngine,
}

impl IndexStats {
    pub fn new(storage: StorageEngine) -> Self {
        Self { storage }
    }

    pub fn increment_usage(&self, index_name: &str) -> Result<()> {
        let current: usize = self.storage.get("_index_stats", index_name)?.unwrap_or(0);
        self.storage.set("_index_stats", index_name, &(current + 1))?;
        Ok(())
    }

    pub fn get_all_stats(&self) -> Result<std::collections::HashMap<String, usize>> {
        let keys = self.storage.get_keys_by_prefix("_index_stats", "")?;
        let mut stats = std::collections::HashMap::new();
        for key in keys {
            if let Ok(Some(count)) = self.storage.get::<usize>("_index_stats", &key) {
                stats.insert(key, count);
            }
        }
        Ok(stats)
    }
}
