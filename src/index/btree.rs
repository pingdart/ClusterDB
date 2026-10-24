use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

const DEFAULT_ORDER: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum IndexValue {
    Int(i64),
    Float(ordered_float::OrderedFloat<f64>),
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CompositeKey {
    pub values: Vec<IndexValue>,
}

impl CompositeKey {
    pub fn new(values: Vec<IndexValue>) -> Self {
        Self { values }
    }

    pub fn single_int(val: i64) -> Self {
        Self { values: vec![IndexValue::Int(val)] }
    }

    pub fn single_text(val: &str) -> Self {
        Self { values: vec![IndexValue::Text(val.to_string())] }
    }
}

// Support ordered float comparison
mod ordered_float {
    use serde::{Deserialize, Serialize};
    use std::cmp::Ordering;

    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
    pub struct OrderedFloat<T>(pub T);

    impl Eq for OrderedFloat<f64> {}

    impl PartialOrd for OrderedFloat<f64> {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    impl Ord for OrderedFloat<f64> {
        fn cmp(&self, other: &Self) -> Ordering {
            self.0.partial_cmp(&other.0).unwrap_or(Ordering::Equal)
        }
    }
}

pub use ordered_float::OrderedFloat;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BTreeNode<K: Ord + Clone, V: Clone> {
    Leaf(LeafNode<K, V>),
    Internal(InternalNode<K>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeafNode<K: Ord + Clone, V: Clone> {
    pub keys: Vec<K>,
    pub values: Vec<V>,
    pub next_leaf_id: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalNode<K: Ord + Clone> {
    pub keys: Vec<K>,
    pub child_ids: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BPlusTree<K: Ord + Clone + Debug, V: Clone + Debug> {
    nodes: Vec<BTreeNode<K, V>>,
    root_id: usize,
    order: usize,
    size: usize,
}

impl<K: Ord + Clone + Debug, V: Clone + Debug> BPlusTree<K, V> {
    pub fn new() -> Self {
        Self::with_order(DEFAULT_ORDER)
    }

    pub fn with_order(order: usize) -> Self {
        let root = BTreeNode::Leaf(LeafNode {
            keys: Vec::new(),
            values: Vec::new(),
            next_leaf_id: None,
        });

        Self {
            nodes: vec![root],
            root_id: 0,
            order: order.max(3),
            size: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.size
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Point lookup: Searches for key in O(log_B N)
    pub fn get(&self, key: &K) -> Option<&V> {
        let leaf = self.find_leaf(self.root_id, key);
        match &self.nodes[leaf] {
            BTreeNode::Leaf(l) => {
                match l.keys.binary_search(key) {
                    Ok(idx) => Some(&l.values[idx]),
                    Err(_) => None,
                }
            }
            _ => unreachable!(),
        }
    }

    /// Range scan: Finds all (key, value) pairs where start <= key <= end.
    /// Traverses directly along leaf pointers in O(log_B N + K).
    pub fn range_scan<'a>(&'a self, start: Option<&K>, end: Option<&K>) -> Vec<(&'a K, &'a V)> {
        let mut results = Vec::new();

        // 1. Locate starting leaf
        let mut current_leaf_id = if let Some(s) = start {
            self.find_leaf(self.root_id, s)
        } else {
            self.find_first_leaf(self.root_id)
        };

        // 2. Scan sequentially across linked leaf chain
        loop {
            match &self.nodes[current_leaf_id] {
                BTreeNode::Leaf(l) => {
                    for (i, k) in l.keys.iter().enumerate() {
                        if let Some(s) = start {
                            if k < s {
                                continue;
                            }
                        }
                        if let Some(e) = end {
                            if k > e {
                                return results;
                            }
                        }
                        results.push((k, &l.values[i]));
                    }

                    match l.next_leaf_id {
                        Some(next_id) => current_leaf_id = next_id,
                        None => break,
                    }
                }
                _ => break,
            }
        }

        results
    }

    /// Inserts a key-value pair, splitting nodes on overflow
    pub fn insert(&mut self, key: K, value: V) -> Result<()> {
        if let Some((split_key, right_child_id)) = self.insert_recursive(self.root_id, key, value)? {
            // Root was split -> create new root
            let new_root = BTreeNode::Internal(InternalNode {
                keys: vec![split_key],
                child_ids: vec![self.root_id, right_child_id],
            });
            self.nodes.push(new_root);
            self.root_id = self.nodes.len() - 1;
        }
        self.size += 1;
        Ok(())
    }

    fn insert_recursive(
        &mut self,
        node_id: usize,
        key: K,
        value: V,
    ) -> Result<Option<(K, usize)>> {
        match &mut self.nodes[node_id] {
            BTreeNode::Leaf(leaf) => {
                let idx = match leaf.keys.binary_search(&key) {
                    Ok(i) => {
                        // Key exists: replace value
                        leaf.values[i] = value;
                        return Ok(None);
                    }
                    Err(i) => i,
                };

                leaf.keys.insert(idx, key);
                leaf.values.insert(idx, value);

                // Check overflow
                if leaf.keys.len() >= self.order {
                    let mid = leaf.keys.len() / 2;
                    let right_keys = leaf.keys.split_off(mid);
                    let right_values = leaf.values.split_off(mid);
                    let split_key = right_keys[0].clone();
                    let next_leaf = leaf.next_leaf_id;
                    let left_keys = std::mem::take(&mut leaf.keys);
                    let left_values = std::mem::take(&mut leaf.values);

                    let right_node_id = self.nodes.len();

                    let right_leaf = BTreeNode::Leaf(LeafNode {
                        keys: right_keys,
                        values: right_values,
                        next_leaf_id: next_leaf,
                    });

                    // Mutate existing leaf's next pointer
                    self.nodes[node_id] = BTreeNode::Leaf(LeafNode {
                        keys: left_keys,
                        values: left_values,
                        next_leaf_id: Some(right_node_id),
                    });

                    self.nodes.push(right_leaf);
                    Ok(Some((split_key, right_node_id)))
                } else {
                    Ok(None)
                }
            }
            BTreeNode::Internal(internal) => {
                let child_idx = match internal.keys.binary_search(&key) {
                    Ok(i) => i + 1,
                    Err(i) => i,
                };
                let child_id = internal.child_ids[child_idx];

                if let Some((split_key, right_id)) = self.insert_recursive(child_id, key, value)? {
                    // Child split: insert split_key and new right_id into this internal node
                    match &mut self.nodes[node_id] {
                        BTreeNode::Internal(curr) => {
                            let k_idx = match curr.keys.binary_search(&split_key) {
                                Ok(i) => i,
                                Err(i) => i,
                            };
                            curr.keys.insert(k_idx, split_key);
                            curr.child_ids.insert(k_idx + 1, right_id);

                            if curr.keys.len() >= self.order {
                                let mid = curr.keys.len() / 2;
                                let parent_promoted_key = curr.keys[mid].clone();

                                let right_keys = curr.keys.split_off(mid + 1);
                                curr.keys.pop(); // Remove promoted key from left node
                                let right_children = curr.child_ids.split_off(mid + 1);

                                let right_internal = BTreeNode::Internal(InternalNode {
                                    keys: right_keys,
                                    child_ids: right_children,
                                });

                                self.nodes.push(right_internal);
                                let actual_right_id = self.nodes.len() - 1;

                                Ok(Some((parent_promoted_key, actual_right_id)))
                            } else {
                                Ok(None)
                            }
                        }
                        _ => unreachable!(),
                    }
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn find_leaf(&self, start_id: usize, key: &K) -> usize {
        let mut curr = start_id;
        loop {
            match &self.nodes[curr] {
                BTreeNode::Leaf(_) => return curr,
                BTreeNode::Internal(internal) => {
                    let idx = match internal.keys.binary_search(key) {
                        Ok(i) => i + 1,
                        Err(i) => i,
                    };
                    curr = internal.child_ids[idx];
                }
            }
        }
    }

    fn find_first_leaf(&self, start_id: usize) -> usize {
        let mut curr = start_id;
        loop {
            match &self.nodes[curr] {
                BTreeNode::Leaf(_) => return curr,
                BTreeNode::Internal(internal) => {
                    curr = internal.child_ids[0];
                }
            }
        }
    }
}

impl<K: Ord + Clone + Debug, V: Clone + Debug> Default for BPlusTree<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bplus_tree_insert_point_lookup_and_node_splits() {
        // Order 4: splits occur frequently
        let mut tree = BPlusTree::with_order(4);

        for i in (1..=50).rev() {
            tree.insert(i, format!("val_{}", i)).unwrap();
        }

        assert_eq!(tree.len(), 50);

        for i in 1..=50 {
            assert_eq!(tree.get(&i), Some(&format!("val_{}", i)));
        }

        assert_eq!(tree.get(&100), None);
    }

    #[test]
    fn test_bplus_tree_range_scan_over_leaf_chain() {
        let mut tree = BPlusTree::with_order(4);

        for i in 1..=100 {
            tree.insert(i, i * 10).unwrap();
        }

        // Range scan [25, 35]
        let range = tree.range_scan(Some(&25), Some(&35));
        assert_eq!(range.len(), 11);
        assert_eq!(*range[0].0, 25);
        assert_eq!(*range[0].1, 250);
        assert_eq!(*range[10].0, 35);
        assert_eq!(*range[10].1, 350);

        // Open lower bound: (<= 10)
        let lower_range = tree.range_scan(None, Some(&10));
        assert_eq!(lower_range.len(), 10);
        assert_eq!(*lower_range[0].0, 1);
        assert_eq!(*lower_range[9].0, 10);
    }

    #[test]
    fn test_composite_key_ordering() {
        let mut tree = BPlusTree::with_order(4);

        let k1 = CompositeKey::new(vec![IndexValue::Text("Engineering".into()), IndexValue::Int(101)]);
        let k2 = CompositeKey::new(vec![IndexValue::Text("Engineering".into()), IndexValue::Int(102)]);
        let k3 = CompositeKey::new(vec![IndexValue::Text("Sales".into()), IndexValue::Int(101)]);

        tree.insert(k1.clone(), "Alice").unwrap();
        tree.insert(k2.clone(), "Bob").unwrap();
        tree.insert(k3.clone(), "Charlie").unwrap();

        assert_eq!(tree.get(&k1), Some(&"Alice"));
        assert_eq!(tree.get(&k2), Some(&"Bob"));
        assert_eq!(tree.get(&k3), Some(&"Charlie"));
    }
}
