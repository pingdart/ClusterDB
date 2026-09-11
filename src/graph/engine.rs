use crate::graph::models::{GraphEdge, GraphNode, GraphPath};
use crate::storage::StorageEngine;
use anyhow::Result;
use std::collections::{HashSet, VecDeque};

#[derive(Clone)]
pub struct GraphEngine {
    storage: StorageEngine,
}

impl GraphEngine {
    pub fn new(storage: StorageEngine) -> Self {
        Self { storage }
    }

    /// Creates or updates a node
    pub fn create_node(&self, node: GraphNode) -> Result<GraphNode> {
        self.storage.set("_graph_nodes", &node.id, &node)?;
        Ok(node)
    }

    pub fn get_node(&self, id: &str) -> Result<Option<GraphNode>> {
        self.storage.get("_graph_nodes", id)
    }

    /// Creates a directed edge between two nodes and updates adjacency indices
    pub fn create_edge(&self, edge: GraphEdge) -> Result<GraphEdge> {
        // Ensure source and target exist
        if self.get_node(&edge.source)?.is_none() {
            return Err(anyhow::anyhow!("Source node '{}' not found", edge.source));
        }
        if self.get_node(&edge.target)?.is_none() {
            return Err(anyhow::anyhow!("Target node '{}' not found", edge.target));
        }

        self.storage.set("_graph_edges", &edge.id, &edge)?;

        // Update Outgoing Adjacency: source -> Vec<edge_id>
        let mut out_edges: Vec<String> = self.storage.get("_graph_out", &edge.source)?.unwrap_or_default();
        if !out_edges.contains(&edge.id) {
            out_edges.push(edge.id.clone());
            self.storage.set("_graph_out", &edge.source, &out_edges)?;
        }

        // Update Incoming Adjacency: target -> Vec<edge_id>
        let mut in_edges: Vec<String> = self.storage.get("_graph_in", &edge.target)?.unwrap_or_default();
        if !in_edges.contains(&edge.id) {
            in_edges.push(edge.id.clone());
            self.storage.set("_graph_in", &edge.target, &in_edges)?;
        }

        Ok(edge)
    }

    pub fn get_edge(&self, id: &str) -> Result<Option<GraphEdge>> {
        self.storage.get("_graph_edges", id)
    }

    /// Retrieves outgoing neighbors matching optional edge type and target label
    pub fn get_neighbors(
        &self,
        node_id: &str,
        edge_type: Option<&str>,
        target_label: Option<&str>,
    ) -> Result<Vec<(GraphEdge, GraphNode)>> {
        let edge_ids: Vec<String> = self.storage.get("_graph_out", node_id)?.unwrap_or_default();
        let mut neighbors = Vec::new();

        for eid in edge_ids {
            if let Some(edge) = self.get_edge(&eid)? {
                if let Some(req_type) = edge_type {
                    if !edge.r#type.eq_ignore_ascii_case(req_type) {
                        continue;
                    }
                }

                if let Some(target_node) = self.get_node(&edge.target)? {
                    if let Some(lbl) = target_label {
                        if !target_node.has_label(lbl) {
                            continue;
                        }
                    }
                    neighbors.push((edge, target_node));
                }
            }
        }

        Ok(neighbors)
    }

    /// Finds the shortest path between start_id and end_id using Breadth-First Search (BFS)
    pub fn find_path(&self, start_id: &str, end_id: &str, max_hops: usize) -> Result<Option<GraphPath>> {
        if start_id == end_id {
            if let Some(node) = self.get_node(start_id)? {
                return Ok(Some(GraphPath {
                    nodes: vec![node],
                    edges: vec![],
                    total_hops: 0,
                }));
            }
            return Ok(None);
        }

        let mut queue: VecDeque<(String, Vec<GraphEdge>)> = VecDeque::new();
        let mut visited: HashSet<String> = HashSet::new();

        queue.push_back((start_id.to_string(), Vec::new()));
        visited.insert(start_id.to_string());

        while let Some((curr_id, path_edges)) = queue.pop_front() {
            if path_edges.len() >= max_hops {
                continue;
            }

            let neighbors = self.get_neighbors(&curr_id, None, None)?;
            for (edge, neighbor_node) in neighbors {
                let neighbor_id = neighbor_node.id.clone();
                let mut new_edges = path_edges.clone();
                new_edges.push(edge);

                if neighbor_id == end_id {
                    // Reconstruct full path of nodes
                    let mut nodes = Vec::new();
                    if let Some(first) = self.get_node(start_id)? {
                        nodes.push(first);
                    }
                    for e in &new_edges {
                        if let Some(n) = self.get_node(&e.target)? {
                            nodes.push(n);
                        }
                    }
                    let hops = new_edges.len();
                    return Ok(Some(GraphPath {
                        nodes,
                        edges: new_edges,
                        total_hops: hops,
                    }));
                }

                if visited.insert(neighbor_id.clone()) {
                    queue.push_back((neighbor_id, new_edges));
                }
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_nodes_edges_and_path() {
        let storage = StorageEngine::new_temp().unwrap();
        let graph = GraphEngine::new(storage);

        // Create Nodes
        let alice = GraphNode::new("user_1".to_string(), vec!["User".to_string(), "Patient".to_string()], serde_json::json!({"name": "Alice"}));
        let dr_smith = GraphNode::new("doc_1".to_string(), vec!["Doctor".to_string()], serde_json::json!({"name": "Dr. Smith"}));
        let hospital = GraphNode::new("hosp_1".to_string(), vec!["Hospital".to_string()], serde_json::json!({"name": "City General"}));

        graph.create_node(alice).unwrap();
        graph.create_node(dr_smith).unwrap();
        graph.create_node(hospital).unwrap();

        // Create Edges
        let edge1 = GraphEdge::new("e1".to_string(), "user_1".to_string(), "doc_1".to_string(), "CONSULTS".to_string(), serde_json::json!({}), None);
        let edge2 = GraphEdge::new("e2".to_string(), "doc_1".to_string(), "hosp_1".to_string(), "WORKS_AT".to_string(), serde_json::json!({}), None);

        graph.create_edge(edge1).unwrap();
        graph.create_edge(edge2).unwrap();

        // Test neighbor lookup with filter
        let neighbors = graph.get_neighbors("user_1", Some("CONSULTS"), Some("Doctor")).unwrap();
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].1.id, "doc_1");

        // Test path finding
        let path = graph.find_path("user_1", "hosp_1", 3).unwrap();
        assert!(path.is_some());
        let p = path.unwrap();
        assert_eq!(p.total_hops, 2);
        assert_eq!(p.nodes.len(), 3);
    }
}
