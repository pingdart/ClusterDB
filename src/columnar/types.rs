use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ColumnType {
    Int64,
    Float64,
    Text,
    Bool,
    Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ColumnArray {
    Int64(Vec<i64>),
    Float64(Vec<f64>),
    Text(Vec<String>),
    Bool(Vec<bool>),
}

impl ColumnArray {
    pub fn len(&self) -> usize {
        match self {
            ColumnArray::Int64(v) => v.len(),
            ColumnArray::Float64(v) => v.len(),
            ColumnArray::Text(v) => v.len(),
            ColumnArray::Bool(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn push_from_str(&mut self, val: &str, col_type: &ColumnType) {
        match (self, col_type) {
            (ColumnArray::Int64(v), ColumnType::Int64 | ColumnType::Timestamp) => {
                v.push(val.parse::<i64>().unwrap_or(0));
            }
            (ColumnArray::Float64(v), ColumnType::Float64) => {
                v.push(val.parse::<f64>().unwrap_or(0.0));
            }
            (ColumnArray::Text(v), ColumnType::Text) => {
                v.push(val.to_string());
            }
            (ColumnArray::Bool(v), ColumnType::Bool) => {
                v.push(val.eq_ignore_ascii_case("true") || val == "1");
            }
            _ => {}
        }
    }

    pub fn new_empty(col_type: &ColumnType) -> Self {
        match col_type {
            ColumnType::Int64 | ColumnType::Timestamp => ColumnArray::Int64(Vec::new()),
            ColumnType::Float64 => ColumnArray::Float64(Vec::new()),
            ColumnType::Text => ColumnArray::Text(Vec::new()),
            ColumnType::Bool => ColumnArray::Bool(Vec::new()),
        }
    }

    pub fn get_as_string(&self, index: usize) -> Option<String> {
        match self {
            ColumnArray::Int64(v) => v.get(index).map(|n| n.to_string()),
            ColumnArray::Float64(v) => v.get(index).map(|n| n.to_string()),
            ColumnArray::Text(v) => v.get(index).cloned(),
            ColumnArray::Bool(v) => v.get(index).map(|b| b.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ColumnZoneMap {
    pub min_i64: Option<i64>,
    pub max_i64: Option<i64>,
    pub min_f64: Option<f64>,
    pub max_f64: Option<f64>,
    pub min_text: Option<String>,
    pub max_text: Option<String>,
    pub null_count: usize,
}

impl ColumnZoneMap {
    pub fn update(&mut self, val: &str, col_type: &ColumnType) {
        if val.is_empty() {
            self.null_count += 1;
            return;
        }

        match col_type {
            ColumnType::Int64 | ColumnType::Timestamp => {
                if let Ok(num) = val.parse::<i64>() {
                    self.min_i64 = Some(self.min_i64.map_or(num, |m| m.min(num)));
                    self.max_i64 = Some(self.max_i64.map_or(num, |m| m.max(num)));
                }
            }
            ColumnType::Float64 => {
                if let Ok(num) = val.parse::<f64>() {
                    self.min_f64 = Some(self.min_f64.map_or(num, |m| m.min(num)));
                    self.max_f64 = Some(self.max_f64.map_or(num, |m| m.max(num)));
                }
            }
            ColumnType::Text => {
                self.min_text = Some(self.min_text.as_ref().map_or_else(|| val.to_string(), |m| if val < m.as_str() { val.to_string() } else { m.clone() }));
                self.max_text = Some(self.max_text.as_ref().map_or_else(|| val.to_string(), |m| if val > m.as_str() { val.to_string() } else { m.clone() }));
            }
            _ => {}
        }
    }

    pub fn can_prune(&self, op: &str, target_val: &str) -> bool {
        if let (Some(min_val), Some(max_val)) = (self.min_i64, self.max_i64) {
            if let Ok(target) = target_val.parse::<i64>() {
                return match op {
                    ">" => max_val <= target,
                    ">=" => max_val < target,
                    "<" => min_val >= target,
                    "<=" => min_val > target,
                    "=" | "==" => target < min_val || target > max_val,
                    _ => false,
                };
            }
        }

        if let (Some(min_val), Some(max_val)) = (self.min_f64, self.max_f64) {
            if let Ok(target) = target_val.parse::<f64>() {
                return match op {
                    ">" => max_val <= target,
                    ">=" => max_val < target,
                    "<" => min_val >= target,
                    "<=" => min_val > target,
                    "=" | "==" => target < min_val || target > max_val,
                    _ => false,
                };
            }
        }

        if let (Some(min_val), Some(max_val)) = (&self.min_text, &self.max_text) {
            return match op {
                ">" => max_val.as_str() <= target_val,
                ">=" => max_val.as_str() < target_val,
                "<" => min_val.as_str() >= target_val,
                "<=" => min_val.as_str() > target_val,
                "=" | "==" => target_val < min_val.as_str() || target_val > max_val.as_str(),
                _ => false,
            };
        }

        false
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnChunk {
    pub chunk_id: usize,
    pub columns: std::collections::HashMap<String, ColumnArray>,
    #[serde(default)]
    pub zone_maps: std::collections::HashMap<String, ColumnZoneMap>,
    pub row_count: usize,
}

impl ColumnChunk {
    pub fn new(chunk_id: usize) -> Self {
        Self {
            chunk_id,
            columns: std::collections::HashMap::new(),
            zone_maps: std::collections::HashMap::new(),
            row_count: 0,
        }
    }

    pub fn add_column(&mut self, name: &str, array: ColumnArray) {
        if self.row_count == 0 {
            self.row_count = array.len();
        }
        self.columns.insert(name.to_string(), array);
    }

    pub fn update_zone_map(&mut self, col: &str, val: &str, col_type: &ColumnType) {
        self.zone_maps.entry(col.to_string()).or_default().update(val, col_type);
    }

    pub fn can_prune(&self, col: &str, op: &str, val: &str) -> bool {
        if let Some(zm) = self.zone_maps.get(col) {
            zm.can_prune(op, val)
        } else {
            false
        }
    }
}
