use crate::columnar::types::{ColumnArray, ColumnChunk, ColumnType};
use crate::storage::StorageEngine;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

const CHUNK_ROW_LIMIT: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AggFunc {
    Count(Option<String>),
    Sum(String),
    Avg(String),
    Min(String),
    Max(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateQuery {
    pub table: String,
    pub group_by: Option<String>,
    pub aggregates: Vec<(String, AggFunc)>, // (alias, func)
    pub filter: Option<(String, String, String)>, // (column, op, value)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnarTable {
    pub name: String,
    pub column_types: HashMap<String, ColumnType>,
    pub chunks: Vec<ColumnChunk>,
    pub current_chunk: ColumnChunk,
}

impl ColumnarTable {
    pub fn new(name: String, column_types: HashMap<String, ColumnType>) -> Self {
        Self {
            name,
            column_types,
            chunks: Vec::new(),
            current_chunk: ColumnChunk::new(0),
        }
    }

    pub fn insert_row(&mut self, row: &HashMap<String, String>) {
        if self.current_chunk.columns.is_empty() {
            for (col_name, col_type) in &self.column_types {
                self.current_chunk
                    .columns
                    .insert(col_name.clone(), ColumnArray::new_empty(col_type));
            }
        }

        for (col_name, col_type) in &self.column_types {
            let val = row.get(col_name).map(|s| s.as_str()).unwrap_or("");
            if let Some(arr) = self.current_chunk.columns.get_mut(col_name) {
                arr.push_from_str(val, col_type);
            }
            self.current_chunk.update_zone_map(col_name, val, col_type);
        }
        self.current_chunk.row_count += 1;

        if self.current_chunk.row_count >= CHUNK_ROW_LIMIT {
            let new_chunk = ColumnChunk::new(self.chunks.len() + 1);
            let finished = std::mem::replace(&mut self.current_chunk, new_chunk);
            self.chunks.push(finished);
        }
    }

    pub fn insert_batch(&mut self, rows: &[HashMap<String, String>]) {
        for row in rows {
            self.insert_row(row);
        }
    }

    pub fn all_chunks(&self) -> Vec<&ColumnChunk> {
        let mut list: Vec<&ColumnChunk> = self.chunks.iter().collect();
        if self.current_chunk.row_count > 0 {
            list.push(&self.current_chunk);
        }
        list
    }

    pub fn row_count(&self) -> usize {
        self.chunks.iter().map(|c| c.row_count).sum::<usize>() + self.current_chunk.row_count
    }
}

pub struct ColumnarEngine {
    storage: StorageEngine,
    tables: RwLock<HashMap<String, ColumnarTable>>,
}

impl ColumnarEngine {
    pub fn new(storage: StorageEngine) -> Self {
        let mut tables = HashMap::new();
        // Load persisted columnar metadata if available
        if let Ok(Some(saved_tables)) = storage.get::<HashMap<String, ColumnarTable>>("_system", "columnar_tables") {
            tables = saved_tables;
        }

        Self {
            storage,
            tables: RwLock::new(tables),
        }
    }

    pub fn register_table(&self, name: &str, column_types: HashMap<String, ColumnType>) {
        let mut tables = self.tables.write().unwrap();
        tables.insert(
            name.to_string(),
            ColumnarTable::new(name.to_string(), column_types),
        );
        let _ = self.storage.set("_system", "columnar_tables", &*tables);
    }

    pub fn reload_from_storage(&self, table_name: &str) {
        let us_name = format!("default_{}", table_name);
        let dot_name = format!("default.{}", table_name);
        let rows = if let Ok(rows) = self.storage.get_all::<crate::exec::RowData>(&us_name) {
            if !rows.is_empty() {
                Some(rows)
            } else {
                self.storage.get_all::<crate::exec::RowData>(&dot_name).ok()
                    .filter(|r| !r.is_empty())
                    .or_else(|| self.storage.get_all::<crate::exec::RowData>(table_name).ok())
            }
        } else {
            self.storage.get_all::<crate::exec::RowData>(&dot_name).ok()
                .filter(|r| !r.is_empty())
                .or_else(|| self.storage.get_all::<crate::exec::RowData>(table_name).ok())
        };

        if let Some(rows) = rows {
            let mut tables = self.tables.write().unwrap();
            tables.remove(table_name);
            tables.remove(&us_name);
            tables.remove(&dot_name);
            if !rows.is_empty() {
                let mut schema = HashMap::new();
                for (k, v) in &rows[0].columns {
                    let inferred = if v.parse::<i64>().is_ok() {
                        ColumnType::Int64
                    } else if v.parse::<f64>().is_ok() {
                        ColumnType::Float64
                    } else if v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("false") {
                        ColumnType::Bool
                    } else {
                        ColumnType::Text
                    };
                    schema.insert(k.clone(), inferred);
                }
                let mut col_table = ColumnarTable::new(table_name.to_string(), schema);
                for r in &rows {
                    col_table.insert_row(&r.columns);
                }
                tables.insert(table_name.to_string(), col_table.clone());
                tables.insert(us_name, col_table.clone());
                tables.insert(dot_name, col_table);
            }
        }
    }

    pub fn insert_row(&self, table: &str, row: &HashMap<String, String>) {
        let mut tables = self.tables.write().unwrap();
        if let Some(col_table) = tables.get_mut(table) {
            col_table.insert_row(row);
        } else {
            // Auto-infer schema if not explicitly registered
            let mut schema = HashMap::new();
            for (k, v) in row {
                let inferred_type = if v.parse::<i64>().is_ok() {
                    ColumnType::Int64
                } else if v.parse::<f64>().is_ok() {
                    ColumnType::Float64
                } else if v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("false") {
                    ColumnType::Bool
                } else {
                    ColumnType::Text
                };
                schema.insert(k.clone(), inferred_type);
            }
            let mut new_table = ColumnarTable::new(table.to_string(), schema);
            new_table.insert_row(row);
            tables.insert(table.to_string(), new_table);
        }
    }

    pub fn insert_batch(&self, table: &str, rows: &[HashMap<String, String>]) {
        let mut tables = self.tables.write().unwrap();
        if let Some(col_table) = tables.get_mut(table) {
            col_table.insert_batch(rows);
        } else if let Some(first) = rows.first() {
            let mut schema = HashMap::new();
            for (k, v) in first {
                let inferred_type = if v.parse::<i64>().is_ok() {
                    ColumnType::Int64
                } else if v.parse::<f64>().is_ok() {
                    ColumnType::Float64
                } else if v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("false") {
                    ColumnType::Bool
                } else {
                    ColumnType::Text
                };
                schema.insert(k.clone(), inferred_type);
            }
            let mut new_table = ColumnarTable::new(table.to_string(), schema);
            new_table.insert_batch(rows);
            tables.insert(table.to_string(), new_table);
        }
    }

    pub fn has_table(&self, table: &str) -> bool {
        let tables = self.tables.read().unwrap();
        tables.contains_key(table)
    }

    /// Vectorized execution of aggregate queries (COUNT, SUM, AVG, MIN, MAX, GROUP BY)
    pub fn execute_aggregate(&self, query: &AggregateQuery) -> Result<Vec<HashMap<String, String>>> {
        let c1 = self.storage.count_rows(&query.table).unwrap_or(0);
        let c2 = self.storage.count_rows(&format!("default_{}", query.table)).unwrap_or(0);
        let c3 = self.storage.count_rows(&format!("default.{}", query.table)).unwrap_or(0);
        let expected_count = std::cmp::max(c1, std::cmp::max(c2, c3));

        if expected_count > 0 {
            let tables = self.tables.read().unwrap();
            let current_count = tables.get(&query.table).map(|t| t.row_count()).unwrap_or(0);
            if current_count != expected_count {
                drop(tables);
                self.reload_from_storage(&query.table);
            }
        }

        let tables = self.tables.read().unwrap();
        let table = tables
            .get(&query.table)
            .ok_or_else(|| anyhow!("Columnar table '{}' not found", query.table))?;

        // Internal accumulators for aggregation
        #[derive(Default, Clone)]
        struct Accumulator {
            count: i64,
            sum_f64: f64,
            sum_i64: i64,
            min: Option<f64>,
            max: Option<f64>,
        }

        let mut groups: HashMap<String, Vec<Accumulator>> = HashMap::new();

        for chunk in table.all_chunks() {
            let row_count = chunk.row_count;
            if row_count == 0 {
                continue;
            }

            // Phase 11C: Zone Map Pruning Optimization (O(1) chunk skipping)
            if let Some((filter_col, filter_op, filter_val)) = &query.filter {
                if chunk.can_prune(filter_col, filter_op, filter_val) {
                    continue;
                }
            }

            // Vectorized filter evaluation creating a boolean selection mask
            let mut selection_mask = vec![true; row_count];
            if let Some((filter_col, filter_op, filter_val)) = &query.filter {
                if let Some(col_arr) = chunk.columns.get(filter_col) {
                    match col_arr {
                        ColumnArray::Int64(vec) => {
                            if let Ok(target) = filter_val.parse::<i64>() {
                                for i in 0..row_count {
                                    let v = vec[i];
                                    selection_mask[i] = match filter_op.as_str() {
                                        "=" | "==" => v == target,
                                        "!=" => v != target,
                                        ">" => v > target,
                                        ">=" => v >= target,
                                        "<" => v < target,
                                        "<=" => v <= target,
                                        _ => false,
                                    };
                                }
                            }
                        }
                        ColumnArray::Float64(vec) => {
                            if let Ok(target) = filter_val.parse::<f64>() {
                                for i in 0..row_count {
                                    let v = vec[i];
                                    selection_mask[i] = match filter_op.as_str() {
                                        "=" | "==" => (v - target).abs() < f64::EPSILON,
                                        "!=" => (v - target).abs() >= f64::EPSILON,
                                        ">" => v > target,
                                        ">=" => v >= target,
                                        "<" => v < target,
                                        "<=" => v <= target,
                                        _ => false,
                                    };
                                }
                            }
                        }
                        ColumnArray::Text(vec) => {
                            for i in 0..row_count {
                                let v = &vec[i];
                                selection_mask[i] = match filter_op.as_str() {
                                    "=" | "==" => v == filter_val,
                                    "!=" => v != filter_val,
                                    _ => false,
                                };
                            }
                        }
                        ColumnArray::Bool(vec) => {
                            let target = filter_val.eq_ignore_ascii_case("true") || filter_val == "1";
                            for i in 0..row_count {
                                selection_mask[i] = (vec[i] == target) == (filter_op == "=" || filter_op == "==");
                            }
                        }
                    }
                }
            }

            // Extract group keys
            let group_keys: Vec<String> = if let Some(ref gb_col) = query.group_by {
                if let Some(col_arr) = chunk.columns.get(gb_col) {
                    (0..row_count)
                        .map(|i| col_arr.get_as_string(i).unwrap_or_else(|| "null".to_string()))
                        .collect()
                } else {
                    vec!["default".to_string(); row_count]
                }
            } else {
                vec!["*".to_string(); row_count]
            };

            // Vectorized accumulation pass
            for i in 0..row_count {
                if !selection_mask[i] {
                    continue;
                }

                let key = &group_keys[i];
                let accs = groups
                    .entry(key.clone())
                    .or_insert_with(|| vec![Accumulator::default(); query.aggregates.len()]);

                for (agg_idx, (_, agg_func)) in query.aggregates.iter().enumerate() {
                    let acc = &mut accs[agg_idx];
                    match agg_func {
                        AggFunc::Count(_) => {
                            acc.count += 1;
                        }
                        AggFunc::Sum(target_col) | AggFunc::Avg(target_col) => {
                            if let Some(col_arr) = chunk.columns.get(target_col) {
                                match col_arr {
                                    ColumnArray::Int64(v) => {
                                        let val = v[i];
                                        acc.sum_i64 += val;
                                        acc.sum_f64 += val as f64;
                                        acc.count += 1;
                                    }
                                    ColumnArray::Float64(v) => {
                                        let val = v[i];
                                        acc.sum_f64 += val;
                                        acc.count += 1;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        AggFunc::Min(target_col) => {
                            if let Some(col_arr) = chunk.columns.get(target_col) {
                                let val_opt = match col_arr {
                                    ColumnArray::Int64(v) => Some(v[i] as f64),
                                    ColumnArray::Float64(v) => Some(v[i]),
                                    _ => None,
                                };
                                if let Some(v) = val_opt {
                                    acc.min = Some(match acc.min {
                                        Some(cur) => cur.min(v),
                                        None => v,
                                    });
                                }
                            }
                        }
                        AggFunc::Max(target_col) => {
                            if let Some(col_arr) = chunk.columns.get(target_col) {
                                let val_opt = match col_arr {
                                    ColumnArray::Int64(v) => Some(v[i] as f64),
                                    ColumnArray::Float64(v) => Some(v[i]),
                                    _ => None,
                                };
                                if let Some(v) = val_opt {
                                    acc.max = Some(match acc.max {
                                        Some(cur) => cur.max(v),
                                        None => v,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // Format results
        let mut results = Vec::new();
        for (grp_key, accs) in groups {
            let mut row_res = HashMap::new();
            if let Some(ref gb_col) = query.group_by {
                row_res.insert(gb_col.clone(), grp_key);
            }

            for (agg_idx, (alias, agg_func)) in query.aggregates.iter().enumerate() {
                let acc = &accs[agg_idx];
                let formatted = match agg_func {
                    AggFunc::Count(_) => acc.count.to_string(),
                    AggFunc::Sum(target_col) => {
                        let is_float = table
                            .column_types
                            .get(target_col)
                            .map(|t| *t == ColumnType::Float64)
                            .unwrap_or(false);
                        if is_float {
                            format!("{:.2}", acc.sum_f64)
                        } else {
                            acc.sum_i64.to_string()
                        }
                    }
                    AggFunc::Avg(_) => {
                        if acc.count > 0 {
                            format!("{:.2}", acc.sum_f64 / acc.count as f64)
                        } else {
                            "0.00".to_string()
                        }
                    }
                    AggFunc::Min(_) => acc.min.map(|n| n.to_string()).unwrap_or_else(|| "null".to_string()),
                    AggFunc::Max(_) => acc.max.map(|n| n.to_string()).unwrap_or_else(|| "null".to_string()),
                };
                row_res.insert(alias.clone(), formatted);
            }
            results.push(row_res);
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vectorized_group_by_aggregation() {
        let storage = StorageEngine::new_temp().unwrap();
        let engine = ColumnarEngine::new(storage);

        let mut schema = HashMap::new();
        schema.insert("region".to_string(), ColumnType::Text);
        schema.insert("amount".to_string(), ColumnType::Int64);
        schema.insert("score".to_string(), ColumnType::Float64);
        engine.register_table("sales", schema);

        // Insert 10 rows across 2 regions
        for i in 1..=10 {
            let mut row = HashMap::new();
            row.insert("region".to_string(), if i % 2 == 0 { "North".to_string() } else { "South".to_string() });
            row.insert("amount".to_string(), (i * 100).to_string());
            row.insert("score".to_string(), (i as f64 * 1.5).to_string());
            engine.insert_row("sales", &row);
        }

        let query = AggregateQuery {
            table: "sales".to_string(),
            group_by: Some("region".to_string()),
            aggregates: vec![
                ("count".to_string(), AggFunc::Count(None)),
                ("total_amount".to_string(), AggFunc::Sum("amount".to_string())),
                ("avg_score".to_string(), AggFunc::Avg("score".to_string())),
            ],
            filter: None,
        };

        let res = engine.execute_aggregate(&query).unwrap();
        assert_eq!(res.len(), 2);

        for row in res {
            let region = row.get("region").unwrap();
            let count = row.get("count").unwrap().parse::<i64>().unwrap();
            assert_eq!(count, 5);
            if region == "North" {
                // 2, 4, 6, 8, 10 -> sum = 3000
                assert_eq!(row.get("total_amount").unwrap(), "3000");
            } else if region == "South" {
                // 1, 3, 5, 7, 9 -> sum = 2500
                assert_eq!(row.get("total_amount").unwrap(), "2500");
            }
        }
    }

    #[test]
    fn test_zone_map_pruning_and_batch_insert() {
        let mut chunk1 = ColumnChunk::new(1);
        let mut chunk2 = ColumnChunk::new(2);

        // Chunk 1: values 1..=50
        for i in 1..=50 {
            chunk1.update_zone_map("id", &i.to_string(), &ColumnType::Int64);
            chunk1.update_zone_map("price", &(i as f64 * 10.0).to_string(), &ColumnType::Float64);
        }

        // Chunk 2: values 51..=100
        for i in 51..=100 {
            chunk2.update_zone_map("id", &i.to_string(), &ColumnType::Int64);
            chunk2.update_zone_map("price", &(i as f64 * 10.0).to_string(), &ColumnType::Float64);
        }

        // Query: id > 80 -> Chunk 1 must be pruned (max is 50 <= 80)
        assert!(chunk1.can_prune("id", ">", "80"));
        assert!(!chunk2.can_prune("id", ">", "80"));

        // Query: id < 30 -> Chunk 2 must be pruned (min is 51 >= 30)
        assert!(!chunk1.can_prune("id", "<", "30"));
        assert!(chunk2.can_prune("id", "<", "30"));

        // Query: price > 600.0 -> Chunk 1 must be pruned (max is 500.0 <= 600.0)
        assert!(chunk1.can_prune("price", ">", "600.0"));
        assert!(!chunk2.can_prune("price", ">", "600.0"));

        // Test ColumnarTable batch insertion
        let storage = StorageEngine::new_temp().unwrap();
        let engine = ColumnarEngine::new(storage);

        let mut batch = Vec::new();
        for i in 1..=100 {
            let mut row = HashMap::new();
            row.insert("id".to_string(), i.to_string());
            row.insert("amount".to_string(), (i * 10).to_string());
            batch.push(row);
        }

        engine.insert_batch("batch_table", &batch);
        assert!(engine.has_table("batch_table"));

        let q = AggregateQuery {
            table: "batch_table".to_string(),
            group_by: None,
            aggregates: vec![("total".to_string(), AggFunc::Sum("amount".to_string()))],
            filter: Some(("id".to_string(), ">".to_string(), "50".to_string())),
        };

        let res = engine.execute_aggregate(&q).unwrap();
        assert_eq!(res.len(), 1);
        // Sum of 510 + 520 + ... + 1000 = (51 + ... + 100) * 10 = 3775 * 10 = 37750
        assert_eq!(res[0].get("total").unwrap(), "37750");
    }
}
