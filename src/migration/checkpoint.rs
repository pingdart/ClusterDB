use crate::storage::StorageEngine;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationCheckpoint {
    pub job_id: String,
    pub source_offset: u64,
    pub last_primary_key: Option<String>,
    pub batch_number: u64,
    pub records_committed: usize,
    pub checksum_so_far: String,
    pub updated_at: u64,
}

impl MigrationCheckpoint {
    pub fn new(job_id: String) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            job_id,
            source_offset: 0,
            last_primary_key: None,
            batch_number: 0,
            records_committed: 0,
            checksum_so_far: String::new(),
            updated_at: now,
        }
    }

    pub fn save(&self, storage: &StorageEngine) -> Result<()> {
        storage.set("_system_migration_checkpoints", &self.job_id, self)?;
        storage.flush()?;
        Ok(())
    }

    pub fn load(storage: &StorageEngine, job_id: &str) -> Result<Option<Self>> {
        storage.get("_system_migration_checkpoints", job_id)
    }

    pub fn delete(storage: &StorageEngine, job_id: &str) -> Result<()> {
        let _ = storage.delete("_system_migration_checkpoints", job_id);
        storage.flush()?;
        Ok(())
    }
}
