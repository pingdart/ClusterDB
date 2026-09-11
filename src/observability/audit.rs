use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub timestamp: u64,
    pub client_ip: String,
    pub user: String,
    pub tenant: String,
    pub action: String,
    pub query: Option<String>,
    pub status: String,
    pub duration_ms: f64,
}

pub struct AuditLogger {
    recent_events: RwLock<Vec<AuditEvent>>,
    max_in_memory: usize,
}

impl AuditLogger {
    pub fn new(max_in_memory: usize) -> Self {
        Self {
            recent_events: RwLock::new(Vec::new()),
            max_in_memory,
        }
    }

    pub fn log(
        &self,
        client_ip: &str,
        user: &str,
        tenant: &str,
        action: &str,
        query: Option<&str>,
        status: &str,
        duration_ms: f64,
    ) {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let event = AuditEvent {
            timestamp: now,
            client_ip: client_ip.to_string(),
            user: user.to_string(),
            tenant: tenant.to_string(),
            action: action.to_string(),
            query: query.map(|s| s.to_string()),
            status: status.to_string(),
            duration_ms,
        };

        // Also print structured tracing info
        tracing::info!(
            target: "audit",
            ip = %client_ip,
            user = %user,
            tenant = %tenant,
            action = %action,
            status = %status,
            latency_ms = duration_ms,
            "Security Audit Event"
        );

        let mut events = self.recent_events.write().unwrap();
        events.push(event);
        if events.len() > self.max_in_memory {
            events.remove(0);
        }
    }

    pub fn get_recent_events(&self, limit: usize) -> Vec<AuditEvent> {
        let events = self.recent_events.read().unwrap();
        events.iter().rev().take(limit).cloned().collect()
    }
}
