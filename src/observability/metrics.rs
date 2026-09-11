use std::sync::atomic::{AtomicU64, Ordering};

pub struct MetricsCollector {
    pub total_queries: AtomicU64,
    pub successful_queries: AtomicU64,
    pub failed_queries: AtomicU64,
    pub vector_searches: AtomicU64,
    pub memory_recalls: AtomicU64,
    pub graph_queries: AtomicU64,
    pub total_query_duration_us: AtomicU64,
    pub active_connections: AtomicU64,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            total_queries: AtomicU64::new(0),
            successful_queries: AtomicU64::new(0),
            failed_queries: AtomicU64::new(0),
            vector_searches: AtomicU64::new(0),
            memory_recalls: AtomicU64::new(0),
            graph_queries: AtomicU64::new(0),
            total_query_duration_us: AtomicU64::new(0),
            active_connections: AtomicU64::new(0),
        }
    }

    pub fn record_query(&self, success: bool, duration_us: u64) {
        self.total_queries.fetch_add(1, Ordering::Relaxed);
        if success {
            self.successful_queries.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_queries.fetch_add(1, Ordering::Relaxed);
        }
        self.total_query_duration_us.fetch_add(duration_us, Ordering::Relaxed);
    }

    pub fn record_vector_search(&self) {
        self.vector_searches.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_memory_recall(&self) {
        self.memory_recalls.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_graph_query(&self) {
        self.graph_queries.fetch_add(1, Ordering::Relaxed);
    }

    /// Renders standard Prometheus exposition format text
    pub fn render_prometheus(&self) -> String {
        let total = self.total_queries.load(Ordering::Relaxed);
        let success = self.successful_queries.load(Ordering::Relaxed);
        let failed = self.failed_queries.load(Ordering::Relaxed);
        let vectors = self.vector_searches.load(Ordering::Relaxed);
        let memories = self.memory_recalls.load(Ordering::Relaxed);
        let graphs = self.graph_queries.load(Ordering::Relaxed);
        let duration_us = self.total_query_duration_us.load(Ordering::Relaxed);
        let avg_latency_ms = if total > 0 {
            (duration_us as f64 / total as f64) / 1000.0
        } else {
            0.0
        };

        let mut out = String::new();
        out.push_str("# HELP pingdart_queries_total Total number of SQL and Document queries processed\n");
        out.push_str("# TYPE pingdart_queries_total counter\n");
        out.push_str(&format!("pingdart_queries_total{{status=\"success\"}} {}\n", success));
        out.push_str(&format!("pingdart_queries_total{{status=\"failed\"}} {}\n", failed));
        out.push_str(&format!("pingdart_queries_total{{status=\"all\"}} {}\n\n", total));

        out.push_str("# HELP pingdart_query_duration_ms_avg Average query execution latency in milliseconds\n");
        out.push_str("# TYPE pingdart_query_duration_ms_avg gauge\n");
        out.push_str(&format!("pingdart_query_duration_ms_avg {:.3}\n\n", avg_latency_ms));

        out.push_str("# HELP pingdart_vector_searches_total Total number of HNSW vector searches\n");
        out.push_str("# TYPE pingdart_vector_searches_total counter\n");
        out.push_str(&format!("pingdart_vector_searches_total {}\n\n", vectors));

        out.push_str("# HELP pingdart_memory_recalls_total Total number of AI cognitive memory recalls\n");
        out.push_str("# TYPE pingdart_memory_recalls_total counter\n");
        out.push_str(&format!("pingdart_memory_recalls_total {}\n\n", memories));

        out.push_str("# HELP pingdart_graph_queries_total Total number of knowledge graph traversals\n");
        out.push_str("# TYPE pingdart_graph_queries_total counter\n");
        out.push_str(&format!("pingdart_graph_queries_total {}\n\n", graphs));

        out
    }
}
