use crate::storage::StorageEngine;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CDCRecord {
    pub sequence_id: u64,
    pub timestamp_ms: i64,
    pub database: String,
    pub table: String,
    pub operation: String, // "INSERT", "UPDATE", "DELETE"
    pub primary_key: String,
    pub before_state: Option<String>,
    pub after_state: Option<String>,
}

impl CDCRecord {
    pub fn before_state_json(&self) -> Option<serde_json::Value> {
        self.before_state.as_deref().and_then(|s| serde_json::from_str(s).ok())
    }

    pub fn after_state_json(&self) -> Option<serde_json::Value> {
        self.after_state.as_deref().and_then(|s| serde_json::from_str(s).ok())
    }
}

#[derive(Clone)]
pub struct CDCLog {
    storage: StorageEngine,
    current_seq: Arc<AtomicU64>,
}

impl CDCLog {
    pub fn new(storage: StorageEngine) -> Result<Self> {
        let latest_seq: u64 = storage
            .get("_cdc_meta", "latest_seq")?
            .unwrap_or(0);

        Ok(Self {
            storage,
            current_seq: Arc::new(AtomicU64::new(latest_seq)),
        })
    }

    /// Appends an immutable change event into the durable CDC commit log
    pub fn append(
        &self,
        database: &str,
        table: &str,
        operation: &str,
        primary_key: &str,
        before_state: Option<serde_json::Value>,
        after_state: Option<serde_json::Value>,
    ) -> Result<CDCRecord> {
        let seq = self.current_seq.fetch_add(1, Ordering::SeqCst) + 1;
        let now = chrono::Utc::now().timestamp_millis();

        let record = CDCRecord {
            sequence_id: seq,
            timestamp_ms: now,
            database: database.to_string(),
            table: table.to_string(),
            operation: operation.to_uppercase(),
            primary_key: primary_key.to_string(),
            before_state: before_state.map(|v| v.to_string()),
            after_state: after_state.map(|v| v.to_string()),
        };

        // Big-endian key for natural lexical ordering in Sled
        let key = format!("{:020}", seq);
        self.storage.set("_cdc_log", &key, &record)?;
        self.storage.set("_cdc_meta", "latest_seq", &seq)?;

        Ok(record)
    }

    /// Replays events starting from `since_seq` up to `limit`
    pub fn replay(&self, since_seq: u64, limit: usize) -> Result<Vec<CDCRecord>> {
        let all_records: Vec<CDCRecord> = self.storage.get_all("_cdc_log")?;
        let mut filtered: Vec<CDCRecord> = all_records
            .into_iter()
            .filter(|r| r.sequence_id > since_seq)
            .collect();

        filtered.sort_by_key(|r| r.sequence_id);
        filtered.truncate(limit);
        Ok(filtered)
    }

    pub fn latest_sequence(&self) -> u64 {
        self.current_seq.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cdc_append_and_replay() {
        let storage = StorageEngine::new_temp().unwrap();
        let cdc = CDCLog::new(storage).unwrap();

        let rec1 = cdc.append("test_db", "users", "INSERT", "1", None, Some(serde_json::json!({"name": "Alice"}))).unwrap();
        let rec2 = cdc.append("test_db", "users", "UPDATE", "1", Some(serde_json::json!({"name": "Alice"})), Some(serde_json::json!({"name": "Alice Cooper"}))).unwrap();
        let rec3 = cdc.append("test_db", "orders", "INSERT", "101", None, Some(serde_json::json!({"total": 99.5}))).unwrap();

        assert_eq!(rec1.sequence_id, 1);
        assert_eq!(rec2.sequence_id, 2);
        assert_eq!(rec3.sequence_id, 3);

        // Replay all events after sequence 1
        let replay = cdc.replay(1, 10).unwrap();
        assert_eq!(replay.len(), 2);
        assert_eq!(replay[0].operation, "UPDATE");
        assert_eq!(replay[1].operation, "INSERT");
    }
}
