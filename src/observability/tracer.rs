use anyhow::{anyhow, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceContext {
    pub version: String,
    pub trace_id: String,     // 32-hex
    pub span_id: String,      // 16-hex
    pub trace_flags: String,  // 2-hex
}

impl TraceContext {
    /// Generates a new root trace context complying with W3C standards
    pub fn new_root() -> Self {
        let mut trace_bytes = [0u8; 16];
        let mut span_bytes = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut trace_bytes);
        rand::thread_rng().fill_bytes(&mut span_bytes);

        Self {
            version: "00".to_string(),
            trace_id: hex::encode(trace_bytes),
            span_id: hex::encode(span_bytes),
            trace_flags: "01".to_string(),
        }
    }

    /// Parses a W3C `traceparent` header: `00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01`
    pub fn from_traceparent(header: &str) -> Result<Self> {
        let parts: Vec<&str> = header.trim().split('-').collect();
        if parts.len() != 4 {
            return Err(anyhow!("Invalid traceparent header: expected 4 segments"));
        }

        if parts[0] != "00" {
            return Err(anyhow!("Unsupported traceparent version: {}", parts[0]));
        }

        if parts[1].len() != 32 || parts[2].len() != 16 {
            return Err(anyhow!("Invalid trace_id or span_id length in traceparent"));
        }

        Ok(Self {
            version: parts[0].to_string(),
            trace_id: parts[1].to_string(),
            span_id: parts[2].to_string(),
            trace_flags: parts[3].to_string(),
        })
    }

    /// Serializes context to W3C `traceparent` string
    pub fn to_traceparent(&self) -> String {
        format!("{}-{}-{}-{}", self.version, self.trace_id, self.span_id, self.trace_flags)
    }

    /// Creates a child span context under this trace, returning (new_child_ctx, parent_span_id)
    pub fn child_span(&self) -> (Self, String) {
        let mut span_bytes = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut span_bytes);
        let child_span_id = hex::encode(span_bytes);
        let parent = self.span_id.clone();

        let child = Self {
            version: self.version.clone(),
            trace_id: self.trace_id.clone(),
            span_id: child_span_id,
            trace_flags: self.trace_flags.clone(),
        };

        (child, parent)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp_epoch_ms: u64,
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub start_time_epoch_ms: u64,
    pub duration_ms: f64,
    pub status: String, // "OK" or "ERROR"
    pub attributes: HashMap<String, String>,
    pub events: Vec<SpanEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSummary {
    pub trace_id: String,
    pub root_span_name: String,
    pub span_count: usize,
    pub duration_ms: f64,
    pub start_time_epoch_ms: u64,
}

pub struct DistributedTracer {
    spans: RwLock<VecDeque<Span>>,
    max_spans: usize,
}

impl DistributedTracer {
    pub fn new(max_spans: usize) -> Self {
        Self {
            spans: RwLock::new(VecDeque::with_capacity(max_spans.min(100_000))),
            max_spans,
        }
    }

    pub fn new_default() -> Self {
        let retention = std::env::var("TRACE_RETENTION_SPANS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(100_000);
        Self::new(retention)
    }
}

impl Default for DistributedTracer {
    fn default() -> Self {
        Self::new_default()
    }
}

impl DistributedTracer {

    /// Records a completed span into the trace buffer
    pub fn record_span(&self, span: Span) {
        let mut buffer = self.spans.write().unwrap();
        if buffer.len() >= self.max_spans {
            buffer.pop_front();
        }
        buffer.push_back(span);
    }

    /// Retrieves all spans associated with a given trace_id
    pub fn get_trace(&self, trace_id: &str) -> Vec<Span> {
        let buffer = self.spans.read().unwrap();
        buffer
            .iter()
            .filter(|s| s.trace_id == trace_id)
            .cloned()
            .collect()
    }

    /// Lists recent traces summarized by root span and total duration
    pub fn list_recent_traces(&self, limit: usize) -> Vec<TraceSummary> {
        let buffer = self.spans.read().unwrap();
        let mut trace_map: HashMap<String, Vec<&Span>> = HashMap::new();

        for span in buffer.iter().rev() {
            trace_map.entry(span.trace_id.clone()).or_default().push(span);
        }

        let mut summaries = Vec::new();
        for (trace_id, span_group) in trace_map {
            if span_group.is_empty() {
                continue;
            }

            // Find root span (no parent_span_id or earliest start time)
            let root = span_group
                .iter()
                .find(|s| s.parent_span_id.is_none())
                .unwrap_or(&span_group[0]);

            let total_dur = span_group.iter().map(|s| s.duration_ms).fold(0.0, f64::max);
            let start = root.start_time_epoch_ms;

            summaries.push(TraceSummary {
                trace_id,
                root_span_name: root.name.clone(),
                span_count: span_group.len(),
                duration_ms: total_dur,
                start_time_epoch_ms: start,
            });

            if summaries.len() >= limit {
                break;
            }
        }

        summaries.sort_by(|a, b| b.start_time_epoch_ms.cmp(&a.start_time_epoch_ms));
        summaries
    }

    pub fn clear(&self) {
        let mut buffer = self.spans.write().unwrap();
        buffer.clear();
    }
}

pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_w3c_traceparent_parse_and_format() {
        let original_header = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let ctx = TraceContext::from_traceparent(original_header).unwrap();
        assert_eq!(ctx.version, "00");
        assert_eq!(ctx.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(ctx.span_id, "00f067aa0ba902b7");
        assert_eq!(ctx.trace_flags, "01");

        let formatted = ctx.to_traceparent();
        assert_eq!(formatted, original_header);
    }

    #[test]
    fn test_distributed_tracer_span_lifecycle() {
        let tracer = DistributedTracer::new(100);
        let root_ctx = TraceContext::new_root();
        let (child_ctx, parent_id) = root_ctx.child_span();

        let mut attrs = HashMap::new();
        attrs.insert("db.statement".to_string(), "SELECT * FROM doctors".to_string());

        let span1 = Span {
            trace_id: root_ctx.trace_id.clone(),
            span_id: root_ctx.span_id.clone(),
            parent_span_id: None,
            name: "QueryExecution".to_string(),
            start_time_epoch_ms: now_epoch_ms(),
            duration_ms: 12.5,
            status: "OK".to_string(),
            attributes: attrs,
            events: vec![],
        };

        let span2 = Span {
            trace_id: child_ctx.trace_id.clone(),
            span_id: child_ctx.span_id.clone(),
            parent_span_id: Some(parent_id),
            name: "HnswVectorSearch".to_string(),
            start_time_epoch_ms: now_epoch_ms(),
            duration_ms: 4.8,
            status: "OK".to_string(),
            attributes: HashMap::new(),
            events: vec![],
        };

        tracer.record_span(span1);
        tracer.record_span(span2);

        let retrieved = tracer.get_trace(&root_ctx.trace_id);
        assert_eq!(retrieved.len(), 2);

        let summaries = tracer.list_recent_traces(10);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].trace_id, root_ctx.trace_id);
    }
}
