use crate::cdc::log::{CDCLog, CDCRecord};
use anyhow::Result;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct CDCStreamer {
    log: CDCLog,
    tx: broadcast::Sender<CDCRecord>,
}

impl CDCStreamer {
    pub fn new(log: CDCLog) -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self { log, tx }
    }

    /// Appends to durable log, then broadcasts to active listeners
    pub fn emit(
        &self,
        database: &str,
        table: &str,
        operation: &str,
        primary_key: &str,
        before: Option<serde_json::Value>,
        after: Option<serde_json::Value>,
    ) -> Result<CDCRecord> {
        let record = self.log.append(database, table, operation, primary_key, before, after)?;
        // Non-blocking broadcast; ignore if there are no active receivers
        let _ = self.tx.send(record.clone());
        Ok(record)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CDCRecord> {
        self.tx.subscribe()
    }

    pub fn log(&self) -> &CDCLog {
        &self.log
    }
}
