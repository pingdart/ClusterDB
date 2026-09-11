use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanNode {
    pub node_type: String,
    pub rows_in: usize,
    pub rows_out: usize,
    pub duration_ms: f64,
    pub cost_est: f64,
    pub details: String,
    pub children: Vec<PlanNode>,
}

impl PlanNode {
    pub fn new(node_type: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            node_type: node_type.into(),
            rows_in: 0,
            rows_out: 0,
            duration_ms: 0.0,
            cost_est: 0.0,
            details: details.into(),
            children: Vec::new(),
        }
    }

    pub fn with_stats(mut self, rows_in: usize, rows_out: usize, duration_ms: f64) -> Self {
        self.rows_in = rows_in;
        self.rows_out = rows_out;
        self.duration_ms = duration_ms;
        self
    }

    pub fn add_child(&mut self, child: PlanNode) {
        self.children.push(child);
    }

    /// Formats the plan tree into an indented ASCII representation
    pub fn render_ascii(&self, depth: usize) -> String {
        let indent = "  ".repeat(depth);
        let prefix = if depth == 0 { "" } else { "└─ " };
        let mut out = format!(
            "{}{}{} (duration: {:.3}ms, in: {}, out: {}) [{}]\n",
            indent, prefix, self.node_type, self.duration_ms, self.rows_in, self.rows_out, self.details
        );
        for child in &self.children {
            out.push_str(&child.render_ascii(depth + 1));
        }
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorMetrics {
    pub ef_search: usize,
    pub visited_nodes: usize,
    pub distance_computations: usize,
    pub candidate_queue_peak: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedQueryMetrics {
    pub remote_nodes: usize,
    pub network_ms: f64,
    pub remote_scan_ms: f64,
    pub merge_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetrics {
    pub memory_bytes_allocated: usize,
    pub peak_memory_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageTimings {
    pub parser_ms: f64,
    pub planner_ms: f64,
    pub index_scan_ms: f64,
    pub filter_ms: f64,
    pub semantic_match_ms: f64,
    pub vector_search_ms: f64,
    pub aggregation_ms: f64,
    pub serialization_ms: f64,
    pub total_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainAnalyzeReport {
    pub query: String,
    pub total_ms: f64,
    pub rows_scanned: usize,
    pub rows_after_filter: usize,
    pub rows_returned: usize,
    pub stages: StageTimings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_metrics: Option<VectorMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distributed_metrics: Option<DistributedQueryMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cbo_decision: Option<crate::planner::CboDecision>,
    pub estimated_rows: usize,
    pub memory: MemoryMetrics,
    pub plan_tree: PlanNode,
    pub plan_ascii: String,
}

pub struct QueryProfiler {
    start_time: Instant,
    pub parser_ms: f64,
    pub planner_ms: f64,
    pub index_scan_ms: f64,
    pub filter_ms: f64,
    pub semantic_match_ms: f64,
    pub vector_search_ms: f64,
    pub aggregation_ms: f64,
    pub serialization_ms: f64,

    pub rows_scanned: usize,
    pub rows_after_filter: usize,
    pub rows_returned: usize,

    pub hnsw_nodes_visited: usize,
    pub hnsw_ef_search: usize,
    pub distance_computations: usize,
    pub candidate_count: usize,

    pub remote_nodes: usize,
    pub network_ms: f64,
    pub remote_scan_ms: f64,
    pub merge_ms: f64,

    pub cbo_decision: Option<crate::planner::CboDecision>,
    pub estimated_rows: usize,

    pub memory_bytes_allocated: usize,
    pub peak_memory_bytes: usize,

    pub plan_nodes: Vec<PlanNode>,
}

impl QueryProfiler {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            parser_ms: 0.0,
            planner_ms: 0.0,
            index_scan_ms: 0.0,
            filter_ms: 0.0,
            semantic_match_ms: 0.0,
            vector_search_ms: 0.0,
            aggregation_ms: 0.0,
            serialization_ms: 0.0,

            rows_scanned: 0,
            rows_after_filter: 0,
            rows_returned: 0,

            hnsw_nodes_visited: 0,
            hnsw_ef_search: 0,
            distance_computations: 0,
            candidate_count: 0,

            remote_nodes: 0,
            network_ms: 0.0,
            remote_scan_ms: 0.0,
            merge_ms: 0.0,

            cbo_decision: None,
            estimated_rows: 0,

            memory_bytes_allocated: 0,
            peak_memory_bytes: 0,

            plan_nodes: Vec::new(),
        }
    }

    pub fn elapsed_ms(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64() * 1000.0
    }

    pub fn record_allocation(&mut self, bytes: usize) {
        self.memory_bytes_allocated += bytes;
        if self.memory_bytes_allocated > self.peak_memory_bytes {
            self.peak_memory_bytes = self.memory_bytes_allocated;
        }
    }

    /// Finalizes profiling and produces the structured report and execution tree
    pub fn finalize(mut self, query: &str, root_plan_node: Option<PlanNode>) -> ExplainAnalyzeReport {
        let total_ms = self.elapsed_ms().max(0.001);
        if self.serialization_ms == 0.0 {
            self.serialization_ms = 0.05; // minimum serialization time
        }

        let vector_metrics = if self.distance_computations > 0 || self.hnsw_nodes_visited > 0 {
            Some(VectorMetrics {
                visited_nodes: self.hnsw_nodes_visited,
                ef_search: self.hnsw_ef_search,
                distance_computations: self.distance_computations,
                candidate_queue_peak: self.candidate_count,
            })
        } else {
            None
        };

        let distributed_metrics = if self.remote_nodes > 0 || self.network_ms > 0.0 {
            Some(DistributedQueryMetrics {
                remote_nodes: self.remote_nodes,
                network_ms: self.network_ms,
                remote_scan_ms: self.remote_scan_ms,
                merge_ms: self.merge_ms,
            })
        } else {
            None
        };

        let stages = StageTimings {
            parser_ms: self.parser_ms,
            planner_ms: self.planner_ms,
            index_scan_ms: self.index_scan_ms,
            filter_ms: self.filter_ms,
            semantic_match_ms: self.semantic_match_ms,
            vector_search_ms: self.vector_search_ms,
            aggregation_ms: self.aggregation_ms,
            serialization_ms: self.serialization_ms,
            total_ms,
        };

        let memory = MemoryMetrics {
            memory_bytes_allocated: self.memory_bytes_allocated,
            peak_memory_bytes: self.peak_memory_bytes,
        };

        let plan_tree = root_plan_node.unwrap_or_else(|| {
            let mut root = PlanNode::new("QueryExecution", format!("SQL: {}", query))
                .with_stats(self.rows_scanned, self.rows_returned, total_ms);

            if let Some(ref dist) = distributed_metrics {
                let exchange = PlanNode::new(
                    "ScatterGatherExchange",
                    format!("remote_nodes={}, network_ms={:.2}, remote_scan_ms={:.2}, merge_ms={:.2}", dist.remote_nodes, dist.network_ms, dist.remote_scan_ms, dist.merge_ms)
                ).with_stats(self.rows_scanned, self.rows_returned, dist.network_ms + dist.remote_scan_ms + dist.merge_ms);
                root.add_child(exchange);
            }

            if self.semantic_match_ms > 0.0 {
                let mut sem_node = PlanNode::new("SemanticFilter", "SEMANTIC_MATCH predicate")
                    .with_stats(self.rows_after_filter, self.rows_returned, self.semantic_match_ms);
                if let Some(ref vm) = vector_metrics {
                    sem_node.add_child(PlanNode::new(
                        "HnswProximitySearch",
                        format!("nodes_visited={}, dist_calcs={}", vm.visited_nodes, vm.distance_computations),
                    ).with_stats(vm.candidate_queue_peak, self.rows_returned, self.vector_search_ms));
                }
                root.add_child(sem_node);
            }

            if self.index_scan_ms > 0.0 {
                root.add_child(PlanNode::new("BTreeIndexScan", "Index-accelerated scan")
                    .with_stats(self.rows_scanned, self.rows_after_filter, self.index_scan_ms));
            } else if self.rows_scanned > 0 {
                root.add_child(PlanNode::new("SequentialScan", "Full collection table scan")
                    .with_stats(self.rows_scanned, self.rows_after_filter, self.filter_ms));
            }

            root
        });

        let plan_ascii = plan_tree.render_ascii(0);

        ExplainAnalyzeReport {
            query: query.to_string(),
            total_ms,
            rows_scanned: self.rows_scanned,
            rows_after_filter: self.rows_after_filter,
            rows_returned: self.rows_returned,
            stages,
            vector_metrics,
            distributed_metrics,
            cbo_decision: self.cbo_decision,
            estimated_rows: self.estimated_rows,
            memory,
            plan_tree,
            plan_ascii,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profiler_and_plannode_tree() {
        let mut profiler = QueryProfiler::new();
        profiler.parser_ms = 0.25;
        profiler.planner_ms = 0.15;
        profiler.index_scan_ms = 1.2;
        profiler.filter_ms = 0.4;
        profiler.rows_scanned = 1000;
        profiler.rows_after_filter = 50;
        profiler.rows_returned = 10;
        profiler.record_allocation(1024 * 64);

        let report = profiler.finalize("SELECT * FROM users WHERE age > 21", None);
        assert!(report.total_ms > 0.0);
        assert_eq!(report.rows_scanned, 1000);
        assert_eq!(report.rows_returned, 10);
        assert_eq!(report.memory.peak_memory_bytes, 65536);
        assert!(report.plan_ascii.contains("QueryExecution"));
        assert!(report.plan_ascii.contains("BTreeIndexScan"));
    }
}
