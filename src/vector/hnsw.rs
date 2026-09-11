use crate::vector::distance::compute_distance;
use crate::vector::types::VectorMetric;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// Candidate node in search priority queue
#[derive(Debug, Clone, PartialEq)]
struct Candidate {
    id: String,
    distance: f32,
}

impl Eq for Candidate {}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap (smallest distance on top)
        other.distance.partial_cmp(&self.distance).unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Max-heap candidate (largest distance on top) for keeping best K
#[derive(Debug, Clone, PartialEq)]
struct MaxCandidate {
    id: String,
    distance: f32,
}

impl Eq for MaxCandidate {}

impl Ord for MaxCandidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.distance.partial_cmp(&other.distance).unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for MaxCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HnswNode {
    pub id: String,
    pub vector: Vec<f32>,
    /// Connections per layer: layer_idx -> Vec<neighbor_ids>
    pub neighbors: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HnswIndex {
    pub metric: VectorMetric,
    pub dimension: usize,
    pub m: usize,                  // Max outgoing connections per layer (e.g., 16)
    pub m0: usize,                 // Max outgoing connections at layer 0 (e.g., 32)
    pub ef_construction: usize,    // Candidate pool during insertion (e.g., 64)
    pub ef_search: usize,          // Candidate pool during query (e.g., 50)
    pub ml: f64,                   // Normalization factor for level generation
    pub enter_point: Option<String>,
    pub max_layer: usize,
    pub nodes: HashMap<String, HnswNode>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorSearchResult {
    pub id: String,
    pub distance: f32,
    pub similarity: f32,
}

impl HnswIndex {
    pub fn new(dimension: usize, metric: VectorMetric) -> Self {
        let m = 16;
        let m0 = 32;
        let ef_construction = 64;
        let ef_search = 50;
        let ml = 1.0 / (m as f64).ln();

        Self {
            metric,
            dimension,
            m,
            m0,
            ef_construction,
            ef_search,
            ml,
            enter_point: None,
            max_layer: 0,
            nodes: HashMap::new(),
            count: 0,
        }
    }

    /// Generates a random layer based on exponential distribution
    fn random_level(&self) -> usize {
        // Deterministic pseudo-randomness based on state count to avoid external rand dependency
        let mut seed = (self.count as u64).wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        let uniform = ((seed & 0xFFFFFFFF) as f64) / (0xFFFFFFFFu64 as f64);
        let uniform = if uniform <= 0.0 { 0.00001 } else { uniform };
        
        let level = (-uniform.ln() * self.ml).floor() as usize;
        level.min(16) // Upper bound to avoid overly deep graphs
    }

    pub fn insert(&mut self, id: String, vector: Vec<f32>) -> Result<(), String> {
        if vector.len() != self.dimension {
            return Err(format!(
                "Dimension mismatch: expected {}, received {}",
                self.dimension,
                vector.len()
            ));
        }

        // If node already exists, update vector and reset connections
        if self.nodes.contains_key(&id) {
            self.delete(&id);
        }

        let target_level = self.random_level();
        let mut new_node = HnswNode {
            id: id.clone(),
            vector: vector.clone(),
            neighbors: vec![Vec::new(); target_level + 1],
        };

        if self.enter_point.is_none() {
            self.enter_point = Some(id.clone());
            self.max_layer = target_level;
            self.nodes.insert(id, new_node);
            self.count += 1;
            return Ok(());
        }

        let mut curr_obj = self.enter_point.clone().unwrap();
        let curr_vec = &self.nodes[&curr_obj].vector;
        let mut curr_dist = compute_distance(&vector, curr_vec, &self.metric);

        // Phase 1: Greedy search through layers above target_level
        let max_l = self.max_layer;
        if max_l > target_level {
            for l in ((target_level + 1)..=max_l).rev() {
                let mut changed = true;
                while changed {
                    changed = false;
                    let node = &self.nodes[&curr_obj];
                    if l < node.neighbors.len() {
                        for neighbor_id in &node.neighbors[l] {
                            if let Some(neighbor) = self.nodes.get(neighbor_id) {
                                let dist = compute_distance(&vector, &neighbor.vector, &self.metric);
                                if dist < curr_dist {
                                    curr_dist = dist;
                                    curr_obj = neighbor_id.clone();
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Phase 2: From min(max_layer, target_level) down to 0, find neighbors and link
        let mut enter_points = vec![Candidate { id: curr_obj, distance: curr_dist }];
        let top_link_level = target_level.min(self.max_layer);

        for l in (0..=top_link_level).rev() {
            let candidates = self.search_layer(&vector, enter_points, self.ef_construction, l);
            enter_points = candidates.clone();

            let max_m = if l == 0 { self.m0 } else { self.m };
            let selected_neighbors = self.select_neighbors(&candidates, max_m);

            // Set outgoing links from new node
            new_node.neighbors[l] = selected_neighbors.clone();

            // Create bidirectional links from neighbors to new node
            for neighbor_id in selected_neighbors {
                if let Some(node) = self.nodes.get(&neighbor_id) {
                    if l < node.neighbors.len() {
                        let mut current_neighbors = node.neighbors[l].clone();
                        current_neighbors.push(id.clone());
                        let neighbor_vec = node.vector.clone();

                        let new_neighbors = if current_neighbors.len() > max_m {
                            let mut cand_list = Vec::new();
                            for nid in &current_neighbors {
                                if let Some(target) = self.nodes.get(nid) {
                                    let d = compute_distance(&neighbor_vec, &target.vector, &self.metric);
                                    cand_list.push(Candidate { id: nid.clone(), distance: d });
                                }
                            }
                            cand_list.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap_or(Ordering::Equal));
                            cand_list.into_iter().take(max_m).map(|c| c.id).collect()
                        } else {
                            current_neighbors
                        };

                        if let Some(node_mut) = self.nodes.get_mut(&neighbor_id) {
                            node_mut.neighbors[l] = new_neighbors;
                        }
                    }
                }
            }
        }

        if target_level > self.max_layer {
            self.max_layer = target_level;
            self.enter_point = Some(id.clone());
        }

        self.nodes.insert(id, new_node);
        self.count += 1;
        Ok(())
    }

    fn search_layer(
        &self,
        query: &[f32],
        enter_points: Vec<Candidate>,
        ef: usize,
        layer: usize,
    ) -> Vec<Candidate> {
        let mut visited = HashSet::new();
        let mut candidates = BinaryHeap::new(); // Min-heap
        let mut w = BinaryHeap::new();          // Max-heap (keeps best ef)

        for ep in enter_points {
            visited.insert(ep.id.clone());
            candidates.push(ep.clone());
            w.push(MaxCandidate { id: ep.id, distance: ep.distance });
        }

        while let Some(curr) = candidates.pop() {
            let furthest_w = w.peek().map(|x| x.distance).unwrap_or(f32::MAX);
            if curr.distance > furthest_w && w.len() >= ef {
                break;
            }

            if let Some(curr_node) = self.nodes.get(&curr.id) {
                if layer < curr_node.neighbors.len() {
                    for neighbor_id in &curr_node.neighbors[layer] {
                        if visited.insert(neighbor_id.clone()) {
                            if let Some(neighbor) = self.nodes.get(neighbor_id) {
                                let dist = compute_distance(query, &neighbor.vector, &self.metric);
                                let furthest = w.peek().map(|x| x.distance).unwrap_or(f32::MAX);
                                if dist < furthest || w.len() < ef {
                                    candidates.push(Candidate {
                                        id: neighbor_id.clone(),
                                        distance: dist,
                                    });
                                    w.push(MaxCandidate {
                                        id: neighbor_id.clone(),
                                        distance: dist,
                                    });
                                    if w.len() > ef {
                                        w.pop();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut results: Vec<Candidate> = w.into_iter()
            .map(|mc| Candidate { id: mc.id, distance: mc.distance })
            .collect();
        results.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap_or(Ordering::Equal));
        results
    }

    fn select_neighbors(&self, candidates: &[Candidate], m: usize) -> Vec<String> {
        let mut sorted = candidates.to_vec();
        sorted.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap_or(Ordering::Equal));
        sorted.into_iter().take(m).map(|c| c.id).collect()
    }

    /// Searches for the top-K nearest neighbors
    pub fn search(&self, query: &[f32], top_k: usize) -> Vec<VectorSearchResult> {
        if self.nodes.is_empty() || self.enter_point.is_none() || top_k == 0 {
            return Vec::new();
        }

        let mut curr_obj = self.enter_point.clone().unwrap();
        let curr_vec = &self.nodes[&curr_obj].vector;
        let mut curr_dist = compute_distance(query, curr_vec, &self.metric);

        // Greedy routing across upper layers
        for l in (1..=self.max_layer).rev() {
            let mut changed = true;
            while changed {
                changed = false;
                if let Some(node) = self.nodes.get(&curr_obj) {
                    if l < node.neighbors.len() {
                        for neighbor_id in &node.neighbors[l] {
                            if let Some(neighbor) = self.nodes.get(neighbor_id) {
                                let dist = compute_distance(query, &neighbor.vector, &self.metric);
                                if dist < curr_dist {
                                    curr_dist = dist;
                                    curr_obj = neighbor_id.clone();
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Layer 0 search with ef_search
        let ep = vec![Candidate { id: curr_obj, distance: curr_dist }];
        let ef = self.ef_search.max(top_k);
        let candidates = self.search_layer(query, ep, ef, 0);

        candidates
            .into_iter()
            .take(top_k)
            .map(|c| {
                let similarity = match self.metric {
                    VectorMetric::Cosine => 1.0 - c.distance,
                    VectorMetric::Euclidean => 1.0 / (1.0 + c.distance),
                    VectorMetric::DotProduct => -c.distance,
                };
                VectorSearchResult {
                    id: c.id,
                    distance: c.distance,
                    similarity,
                }
            })
            .collect()
    }

    pub fn delete(&mut self, id: &str) -> bool {
        if let Some(removed_node) = self.nodes.remove(id) {
            self.count = self.count.saturating_sub(1);
            // Remove incoming references from all neighbors
            for (l, neighbors) in removed_node.neighbors.into_iter().enumerate() {
                for nid in neighbors {
                    if let Some(n) = self.nodes.get_mut(&nid) {
                        if l < n.neighbors.len() {
                            n.neighbors[l].retain(|x| x != id);
                        }
                    }
                }
            }
            if self.enter_point.as_deref() == Some(id) {
                self.enter_point = self.nodes.keys().next().cloned();
                if let Some(new_ep) = &self.enter_point {
                    self.max_layer = self.nodes[new_ep].neighbors.len().saturating_sub(1);
                } else {
                    self.max_layer = 0;
                }
            }
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hnsw_insert_and_search() {
        let mut index = HnswIndex::new(3, VectorMetric::Cosine);
        
        index.insert("doc1".to_string(), vec![1.0, 0.0, 0.0]).unwrap();
        index.insert("doc2".to_string(), vec![0.0, 1.0, 0.0]).unwrap();
        index.insert("doc3".to_string(), vec![0.707, 0.707, 0.0]).unwrap();

        let query = vec![0.9, 0.1, 0.0];
        let results = index.search(&query, 2);

        assert!(!results.is_empty());
        assert_eq!(results[0].id, "doc1");
        assert!(results[0].similarity > 0.9);
    }
}
