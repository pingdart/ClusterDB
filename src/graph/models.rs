use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub labels: Vec<String>, // e.g. ["User", "Doctor"]
    pub properties: String, // JSON string
    pub created_at: i64,
}

impl GraphNode {
    pub fn new(id: String, labels: Vec<String>, properties: serde_json::Value) -> Self {
        Self {
            id,
            labels,
            properties: properties.to_string(),
            created_at: chrono::Utc::now().timestamp(),
        }
    }

    pub fn has_label(&self, label: &str) -> bool {
        self.labels.iter().any(|l| l.eq_ignore_ascii_case(label))
    }

    pub fn properties_json(&self) -> serde_json::Value {
        serde_json::from_str(&self.properties).unwrap_or(serde_json::json!({}))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub r#type: String, // e.g. "KNOWS", "TREATS", "SUPERVISES"
    pub properties: String, // JSON string
    pub weight: f32,
    pub created_at: i64,
}

impl GraphEdge {
    pub fn new(
        id: String,
        source: String,
        target: String,
        r#type: String,
        properties: serde_json::Value,
        weight: Option<f32>,
    ) -> Self {
        Self {
            id,
            source,
            target,
            r#type,
            properties: properties.to_string(),
            weight: weight.unwrap_or(1.0),
            created_at: chrono::Utc::now().timestamp(),
        }
    }

    pub fn properties_json(&self) -> serde_json::Value {
        serde_json::from_str(&self.properties).unwrap_or(serde_json::json!({}))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphPath {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub total_hops: usize,
}
