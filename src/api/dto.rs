use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Query parameters supported by PRD Section 6.4 for GET /v1/data/:table
#[derive(Deserialize, Debug, Clone, Default)]
pub struct RestDataListQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub page: Option<usize>,
    pub select: Option<String>,
    pub order: Option<String>,
    pub sort: Option<String>,
    pub q: Option<String>,
    pub search: Option<String>,
    pub database: Option<String>,
    #[serde(flatten)]
    pub filters: HashMap<String, String>,
}

/// Filter operators supported in query string (e.g., status=eq.active, age=gt.21)
#[derive(Debug, Clone, PartialEq)]
pub enum FilterOp {
    Eq(String),
    Neq(String),
    Gt(String),
    Gte(String),
    Lt(String),
    Lte(String),
    Like(String),
    ILike(String),
    In(Vec<String>),
    IsNull,
    NotNull,
}

impl FilterOp {
    pub fn parse(s: &str) -> Self {
        let trimmed = s.trim();
        if let Some(val) = trimmed.strip_prefix("eq.") {
            FilterOp::Eq(val.to_string())
        } else if let Some(val) = trimmed.strip_prefix("neq.") {
            FilterOp::Neq(val.to_string())
        } else if let Some(val) = trimmed.strip_prefix("gt.") {
            FilterOp::Gt(val.to_string())
        } else if let Some(val) = trimmed.strip_prefix("gte.") {
            FilterOp::Gte(val.to_string())
        } else if let Some(val) = trimmed.strip_prefix("lt.") {
            FilterOp::Lt(val.to_string())
        } else if let Some(val) = trimmed.strip_prefix("lte.") {
            FilterOp::Lte(val.to_string())
        } else if let Some(val) = trimmed.strip_prefix("like.") {
            FilterOp::Like(val.to_string())
        } else if let Some(val) = trimmed.strip_prefix("ilike.") {
            FilterOp::ILike(val.to_string())
        } else if let Some(val) = trimmed.strip_prefix("in.") {
            let list = val
                .trim_matches(|c| c == '(' || c == ')')
                .split(',')
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect();
            FilterOp::In(list)
        } else if trimmed == "is.null" {
            FilterOp::IsNull
        } else if trimmed == "not.null" {
            FilterOp::NotNull
        } else {
            // Default fallback is exact equality match
            FilterOp::Eq(trimmed.to_string())
        }
    }

    pub fn matches(&self, value: Option<&serde_json::Value>) -> bool {
        match self {
            FilterOp::IsNull => match value {
                None => true,
                Some(serde_json::Value::Null) => true,
                Some(serde_json::Value::String(s)) if s == "null" || s.is_empty() => true,
                _ => false,
            },
            FilterOp::NotNull => match value {
                None => false,
                Some(serde_json::Value::Null) => false,
                Some(serde_json::Value::String(s)) if s == "null" || s.is_empty() => false,
                _ => true,
            },
            _ => {
                let Some(val) = value else {
                    return false;
                };

                let val_str = match val {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Null => "null".to_string(),
                    other => other.to_string(),
                };

                match self {
                    FilterOp::Eq(target) => &val_str == target,
                    FilterOp::Neq(target) => &val_str != target,
                    FilterOp::Gt(target) => {
                        if let (Ok(n1), Ok(n2)) = (val_str.parse::<f64>(), target.parse::<f64>()) {
                            n1 > n2
                        } else {
                            &val_str > target
                        }
                    }
                    FilterOp::Gte(target) => {
                        if let (Ok(n1), Ok(n2)) = (val_str.parse::<f64>(), target.parse::<f64>()) {
                            n1 >= n2
                        } else {
                            &val_str >= target
                        }
                    }
                    FilterOp::Lt(target) => {
                        if let (Ok(n1), Ok(n2)) = (val_str.parse::<f64>(), target.parse::<f64>()) {
                            n1 < n2
                        } else {
                            &val_str < target
                        }
                    }
                    FilterOp::Lte(target) => {
                        if let (Ok(n1), Ok(n2)) = (val_str.parse::<f64>(), target.parse::<f64>()) {
                            n1 <= n2
                        } else {
                            &val_str <= target
                        }
                    }
                    FilterOp::Like(pattern) => Self::matches_wildcard(&val_str, pattern, false),
                    FilterOp::ILike(pattern) => Self::matches_wildcard(&val_str, pattern, true),
                    FilterOp::In(list) => list.iter().any(|item| item == &val_str),
                    FilterOp::IsNull | FilterOp::NotNull => unreachable!(),
                }
            }
        }
    }

    fn matches_wildcard(text: &str, pattern: &str, case_insensitive: bool) -> bool {
        let (t, p) = if case_insensitive {
            (text.to_lowercase(), pattern.to_lowercase())
        } else {
            (text.to_string(), pattern.to_string())
        };

        let trimmed_p = p.trim_matches('*');
        if p.starts_with('*') && p.ends_with('*') {
            t.contains(trimmed_p)
        } else if p.starts_with('*') {
            t.ends_with(trimmed_p)
        } else if p.ends_with('*') {
            t.starts_with(trimmed_p)
        } else {
            t == p || t.contains(&p)
        }
    }
}

/// Standard Pagination metadata returned in GET list responses
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PaginationMeta {
    pub total: usize,
    pub page: usize,
    pub limit: usize,
    pub offset: usize,
    pub has_more: bool,
}

/// Standard REST list response envelope
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RestListResponse {
    pub status: String,
    pub table: String,
    pub count: usize,
    pub data: Vec<serde_json::Value>,
    pub pagination: PaginationMeta,
}

/// Column definition for schema creation and inspection
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ColumnDefDto {
    pub name: String,
    #[serde(default = "default_data_type")]
    pub data_type: String,
    #[serde(default)]
    pub is_primary: bool,
    #[serde(default)]
    pub auto_increment: bool,
    #[serde(default)]
    pub is_unique: bool,
    #[serde(default)]
    pub is_not_null: bool,
    #[serde(default)]
    pub default_value: Option<String>,
}

fn default_data_type() -> String {
    "TEXT".to_string()
}

/// DTO to create a table as per PRD Section 6.2
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct CreateTableDto {
    pub name: String,
    #[serde(default)]
    pub columns: Vec<ColumnDefDto>,
}

/// Table summary info returned in schema list
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TableSummaryDto {
    pub name: String,
    pub full_name: String,
    pub database: String,
    pub row_count: usize,
    pub columns: Vec<ColumnDefDto>,
}

/// Structured SQL/Query request
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct QueryApiRequest {
    pub query: String,
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub tenant_id: Option<String>,
}
