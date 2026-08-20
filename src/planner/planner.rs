use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum ExecutionType {
    PrimaryIndexLookup,
    SecondaryIndexScan,
    CompositeIndexScan,
    CollectionScan,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub collection: String,
    pub index_used: Option<String>,
    pub execution_type: ExecutionType,
    pub estimated_cost: usize,
    pub filter_values: Vec<String>,
}

pub struct QueryPlanner;

impl QueryPlanner {
    pub fn new() -> Self {
        Self
    }

    /// Evaluates the available indexes and chooses the best execution plan for the query.
    pub fn plan(
        &self,
        collection: &str,
        filters: &[(String, String, String)], // (field, operator, value)
        available_indexes: &[Vec<String>],
    ) -> ExecutionPlan {
        
        // 1. Check for Primary Key lookup
        for filter in filters {
            if filter.0 == "_id" && (filter.1 == "==" || filter.1 == "=") {
                return ExecutionPlan {
                    collection: collection.to_string(),
                    index_used: Some("PRIMARY".to_string()),
                    execution_type: ExecutionType::PrimaryIndexLookup,
                    estimated_cost: 1, // O(1)
                    filter_values: vec![filter.2.clone()],
                };
            }
        }

        // 2. Check for Composite Indexes (Exact match of all fields in the index)
        // We sort the available indexes by length descending to prioritize largest composite indexes
        let mut sorted_indexes = available_indexes.to_vec();
        sorted_indexes.sort_by(|a, b| b.len().cmp(&a.len()));

        for index_fields in &sorted_indexes {
            if index_fields.len() > 1 {
                let mut matched_values = Vec::new();
                let mut fully_matched = true;

                for field in index_fields {
                    let mut found = false;
                    for filter in filters {
                        if &filter.0 == field && (filter.1 == "==" || filter.1 == "=") {
                            matched_values.push(filter.2.clone());
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        fully_matched = false;
                        break;
                    }
                }

                if fully_matched {
                    return ExecutionPlan {
                        collection: collection.to_string(),
                        index_used: Some(index_fields.join(", ")),
                        execution_type: ExecutionType::CompositeIndexScan,
                        estimated_cost: 5, // O(1) index jump
                        filter_values: vec![matched_values.join("|")],
                    };
                }
            }
        }

        // 3. Check for Single Secondary Indexes
        for index_fields in &sorted_indexes {
            if index_fields.len() == 1 {
                for filter in filters {
                    if &filter.0 == &index_fields[0] && (filter.1 == "==" || filter.1 == "=") {
                        return ExecutionPlan {
                            collection: collection.to_string(),
                            index_used: Some(index_fields[0].clone()),
                            execution_type: ExecutionType::SecondaryIndexScan,
                            estimated_cost: 10,
                            filter_values: vec![filter.2.clone()],
                        };
                    }
                }
            }
        }

        // 4. Fallback to Full Collection Scan
        ExecutionPlan {
            collection: collection.to_string(),
            index_used: None,
            execution_type: ExecutionType::CollectionScan,
            estimated_cost: 10000, // O(N)
            filter_values: vec![],
        }
    }
}
