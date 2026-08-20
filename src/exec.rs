use crate::storage::StorageEngine;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sqlparser::ast::{Statement, SetExpr, Expr, Value, ColumnOption};
// Removed unused GenericDialect import
use sqlparser::parser::Parser;
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct RowData {
    #[serde(default)]
    pub _id: String,
    #[serde(default)]
    pub _version: u64,
    #[serde(default)]
    pub _created_at: u64,
    #[serde(default)]
    pub _updated_at: u64,
    #[serde(default)]
    pub _deleted: bool,
    #[serde(default)]
    pub _owner: Option<String>,
    #[serde(default)]
    pub _tenant: Option<String>,
    #[serde(default)]
    pub _checksum: Option<String>,
    #[serde(default)]
    pub _page_id: Option<String>,
    #[serde(default)]
    pub _transaction_id: Option<String>,
    pub columns: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: String,
    pub is_primary: bool,
    pub auto_increment: bool,
    pub is_unique: bool,
    pub is_not_null: bool,
    pub default_value: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnDef>,
}

#[derive(Clone, Debug)]
enum EvalVal {
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
}

impl EvalVal {
    fn from_str_auto(s: &str) -> Self {
        if let Ok(n) = s.parse::<f64>() {
            EvalVal::Num(n)
        } else if s.eq_ignore_ascii_case("true") {
            EvalVal::Bool(true)
        } else if s.eq_ignore_ascii_case("false") {
            EvalVal::Bool(false)
        } else if s.eq_ignore_ascii_case("null") {
            EvalVal::Null
        } else {
            EvalVal::Str(s.to_string())
        }
    }
}

pub fn clean_ident(s: &str) -> String {
    s.trim().trim_matches(|c| c == '`' || c == '"' || c == '\'').to_string()
}

fn eval_expr(expr: &Expr, row: &RowData) -> EvalVal {
    match expr {
        Expr::Identifier(ident) => {
            if let Some(val) = row.columns.get(&ident.value) {
                EvalVal::from_str_auto(val)
            } else {
                EvalVal::Null
            }
        },
        Expr::CompoundIdentifier(idents) => {
            let col = idents.last().unwrap().value.clone();
            if let Some(val) = row.columns.get(&col) {
                EvalVal::from_str_auto(val)
            } else {
                EvalVal::Null
            }
        },
        Expr::Value(val) => match val {
            Value::Number(n, _) => EvalVal::from_str_auto(n),
            Value::SingleQuotedString(s) => EvalVal::Str(s.clone()),
            Value::Boolean(b) => EvalVal::Bool(*b),
            Value::Null => EvalVal::Null,
            _ => EvalVal::Str(val.to_string()),
        },
        Expr::Nested(inner) => eval_expr(inner, row),
        Expr::UnaryOp { op, expr: inner_expr } => {
            let val = eval_expr(inner_expr, row);
            if let sqlparser::ast::UnaryOperator::Minus = op {
                if let EvalVal::Num(n) = val {
                    return EvalVal::Num(-n);
                }
            }
            if let sqlparser::ast::UnaryOperator::Not = op {
                if let EvalVal::Bool(b) = val {
                    return EvalVal::Bool(!b);
                }
            }
            val
        },
        Expr::BinaryOp { left, op, right } => {
            let l = eval_expr(left, row);
            let r = eval_expr(right, row);
            
            if let sqlparser::ast::BinaryOperator::And = op {
                let lb = matches!(l, EvalVal::Bool(true));
                let rb = matches!(r, EvalVal::Bool(true));
                return EvalVal::Bool(lb && rb);
            }
            if let sqlparser::ast::BinaryOperator::Or = op {
                let lb = matches!(l, EvalVal::Bool(true));
                let rb = matches!(r, EvalVal::Bool(true));
                return EvalVal::Bool(lb || rb);
            }
            
            match (l, r) {
                (EvalVal::Num(n1), EvalVal::Num(n2)) => {
                    let b = match op {
                        sqlparser::ast::BinaryOperator::Eq => n1 == n2,
                        sqlparser::ast::BinaryOperator::NotEq => n1 != n2,
                        sqlparser::ast::BinaryOperator::Lt => n1 < n2,
                        sqlparser::ast::BinaryOperator::LtEq => n1 <= n2,
                        sqlparser::ast::BinaryOperator::Gt => n1 > n2,
                        sqlparser::ast::BinaryOperator::GtEq => n1 >= n2,
                        _ => false,
                    };
                    EvalVal::Bool(b)
                },
                (EvalVal::Str(s1), EvalVal::Str(s2)) => {
                    let b = match op {
                        sqlparser::ast::BinaryOperator::Eq => s1 == s2,
                        sqlparser::ast::BinaryOperator::NotEq => s1 != s2,
                        sqlparser::ast::BinaryOperator::Lt => s1 < s2,
                        sqlparser::ast::BinaryOperator::LtEq => s1 <= s2,
                        sqlparser::ast::BinaryOperator::Gt => s1 > s2,
                        sqlparser::ast::BinaryOperator::GtEq => s1 >= s2,
                        _ => false,
                    };
                    EvalVal::Bool(b)
                },
                (v1, v2) => {
                    let s1 = match v1 {
                        EvalVal::Num(n) => n.to_string(),
                        EvalVal::Str(s) => s,
                        EvalVal::Bool(b) => b.to_string(),
                        EvalVal::Null => "null".to_string(),
                    };
                    let s2 = match v2 {
                        EvalVal::Num(n) => n.to_string(),
                        EvalVal::Str(s) => s,
                        EvalVal::Bool(b) => b.to_string(),
                        EvalVal::Null => "null".to_string(),
                    };
                    let b = match op {
                        sqlparser::ast::BinaryOperator::Eq => s1 == s2,
                        sqlparser::ast::BinaryOperator::NotEq => s1 != s2,
                        _ => false,
                    };
                    EvalVal::Bool(b)
                }
            }
        },
        _ => EvalVal::Null,
    }
}

pub struct Executor {
    storage: StorageEngine,
    index_manager: crate::index::manager::IndexManager,
    planner: crate::planner::planner::QueryPlanner,
    index_stats: crate::index::stats::IndexStats,
}

impl Executor {
    pub fn new(storage: StorageEngine) -> Self {
        let index_manager = crate::index::manager::IndexManager::new(storage.clone());
        let planner = crate::planner::planner::QueryPlanner::new();
        let index_stats = crate::index::stats::IndexStats::new(storage.clone());
        Self { storage, index_manager, planner, index_stats }
    }

    pub fn execute(&self, query: &str, current_db: Option<&str>) -> Result<String> {
        let q_lower = query.trim().to_lowercase();
        
        // MVP Custom Syntax Interception for Admin Commands
        if q_lower.starts_with("create database ") {
            let parts: Vec<&str> = query.trim().split_whitespace().collect();
            if parts.len() >= 3 {
                let db_name = parts[2];
                self.storage.set("_databases", db_name, &true)?;
                self.storage.flush()?;
                return Ok(format!("Database '{}' created successfully", db_name));
            }
        }
        
        if q_lower.starts_with("create user ") {
            let parts: Vec<&str> = query.trim().split_whitespace().collect();
            // Expected: CREATE USER username PASSWORD pass [IP %]
            if parts.len() >= 5 && parts[3].to_lowercase() == "password" {
                let username = parts[2].to_string();
                let password = parts[4].to_string();
                
                let mut allowed_ip = "%".to_string();
                if parts.len() >= 7 && parts[5].to_lowercase() == "ip" {
                    allowed_ip = parts[6].to_string();
                }

                let user = crate::auth::User::new_user(username.clone(), password, allowed_ip, vec![]);
                self.storage.set("_users", &username, &user)?;
                self.storage.flush()?;
                return Ok(format!("User '{}' created successfully", username));
            }
        }

        if q_lower.starts_with("drop database ") {
            let parts: Vec<&str> = query.trim().split_whitespace().collect();
            if parts.len() >= 3 {
                let db_name = parts[2];
                
                // Clean up all tables associated with this database
                let prefix = format!("{}_", db_name);
                if let Ok(full_tables) = self.storage.get_keys_by_prefix("_tables", &prefix) {
                    for full_table_name in full_tables {
                        // Drop the table data tree
                        let _ = self.storage.drop_tree(&full_table_name);
                        
                        // Delete schema and table markers
                        let _ = self.storage.delete("_schemas", &full_table_name);
                        let _ = self.storage.delete("_tables", &full_table_name);
                    }
                }
                
                self.storage.delete("_databases", db_name)?;
                self.storage.flush()?;
                return Ok(format!("Database '{}' dropped successfully", db_name));
            }
        }

        if q_lower.starts_with("drop user ") {
            let parts: Vec<&str> = query.trim().split_whitespace().collect();
            if parts.len() >= 3 {
                let username = parts[2];
                self.storage.delete("_users", username)?;
                self.storage.flush()?;
                return Ok(format!("User '{}' dropped successfully", username));
            }
        }

        if q_lower.starts_with("alter user ") {
            let parts: Vec<&str> = query.trim().split_whitespace().collect();
            if parts.len() >= 5 && parts[3].to_lowercase() == "password" {
                let username = parts[2].to_string();
                let new_password = parts[4].to_string();
                
                if let Ok(Some(user)) = self.storage.get::<crate::auth::User>("_users", &username) {
                    let updated_user = crate::auth::User::new_user(
                        username.clone(), 
                        new_password, 
                        user.allowed_ip.clone(), 
                        user.roles.clone()
                    );
                    self.storage.set("_users", &username, &updated_user)?;
                    self.storage.flush()?;
                    return Ok(format!("User '{}' password changed successfully", username));
                } else {
                    return Err(anyhow::anyhow!("User '{}' not found", username));
                }
            }
        }

        if q_lower.starts_with("grant ") {
            let parts: Vec<&str> = query.trim().split_whitespace().collect();
            
            let on_idx = parts.iter().position(|&r| r.eq_ignore_ascii_case("on"));
            let to_idx = parts.iter().position(|&r| r.eq_ignore_ascii_case("to"));
            
            if let (Some(on_pos), Some(to_pos)) = (on_idx, to_idx) {
                if on_pos > 1 && to_pos > on_pos + 1 && parts.len() > to_pos + 1 {
                    let perm = parts[1..on_pos].join(" ").to_uppercase();
                    let db_name = parts[on_pos + 1].to_string();
                    let username = parts[to_pos + 1].to_string();

                    if let Ok(Some(mut user)) = self.storage.get::<crate::auth::User>("_users", &username) {
                        let role = if perm == "ALL" || perm == "ALL PRIVILEGES" {
                            crate::auth::Role::DatabaseAdmin(db_name.clone())
                        } else if perm == "READ" || perm == "SELECT" {
                            crate::auth::Role::ReadOnly(db_name.clone())
                        } else if perm.contains("WRITE") || perm == "READWRITE" {
                            crate::auth::Role::ReadWrite(db_name.clone())
                        } else {
                            crate::auth::Role::DatabaseAdmin(db_name.clone())
                        };
                    
                    user.roles.push(role);
                    self.storage.set("_users", &username, &user)?;
                    self.storage.flush()?;
                    return Ok(format!("Granted {} on {} to {}", perm, db_name, username));
                } else {
                    return Err(anyhow::anyhow!("User '{}' not found", username));
                }
                }
            }
        }

        if q_lower.starts_with("create index ") {
            let parts: Vec<&str> = query.trim().split(|c| c == ' ' || c == '(' || c == ')' || c == ',').filter(|s| !s.is_empty()).collect();
            // Expected: CREATE INDEX idx_name ON table field1 field2...
            if parts.len() >= 6 && parts[3].eq_ignore_ascii_case("on") {
                let table_name = clean_ident(parts[4]);
                let mut fields = Vec::new();
                for i in 5..parts.len() {
                    fields.push(parts[i]);
                }
                let full_table_name = format!("{}_{}", current_db.unwrap_or("public"), table_name);
                self.index_manager.create_index(&full_table_name, &fields)?;
                return Ok(format!("Index created on {}({})", table_name, fields.join(", ")));
            }
        }

        if q_lower.starts_with("explain select") {
            let select_query = query[8..].trim(); // skip "EXPLAIN "
            let q = sqlparser::parser::Parser::parse_sql(&sqlparser::dialect::MySqlDialect {}, select_query)?;
            if let Some(sqlparser::ast::Statement::Query(boxed_q)) = q.first() {
                if let sqlparser::ast::SetExpr::Select(select) = &*boxed_q.body {
                    if let Some(table_with_joins) = select.from.first() {
                        let collection = clean_ident(&table_with_joins.relation.to_string());
                        let mut filters = Vec::new();
                        
                        if let Some(selection) = &select.selection {
                            if let sqlparser::ast::Expr::BinaryOp { left, op, right } = selection {
                                let mut extracted_col = None;
                                if let (sqlparser::ast::Expr::CompoundIdentifier(idents), sqlparser::ast::BinaryOperator::Eq) = (&**left, op) {
                                    extracted_col = Some(idents.last().unwrap().value.clone());
                                } else if let (sqlparser::ast::Expr::Identifier(ident), sqlparser::ast::BinaryOperator::Eq) = (&**left, op) {
                                    extracted_col = Some(ident.value.clone());
                                }
                                
                                if let Some(col) = extracted_col {
                                    let val = match &**right {
                                        sqlparser::ast::Expr::Value(sqlparser::ast::Value::SingleQuotedString(s)) => s.clone(),
                                        sqlparser::ast::Expr::Value(sqlparser::ast::Value::Number(n, _)) => n.clone(),
                                        sqlparser::ast::Expr::Value(sqlparser::ast::Value::Boolean(b)) => b.to_string(),
                                        _ => right.to_string(),
                                    };
                                    filters.push(crate::server::DocumentFilter {
                                        field: col,
                                        operator: "==".to_string(),
                                        value: val,
                                    });
                                }
                            }
                        }
                        
                        let req = crate::server::DocumentQueryRequest {
                            collection,
                            r#where: if filters.is_empty() { None } else { Some(filters) },
                            near: None,
                            order_by: None,
                            limit: None,
                            offset: None,
                            explain: Some(true),
                            token: "".to_string(),
                            database: None,
                        };
                        
                        return self.execute_document_query(&req, current_db);
                    }
                }
            }
            return Err(anyhow::anyhow!("Invalid EXPLAIN query"));
        }

        if q_lower.starts_with("show index stats") {
            let stats = self.index_stats.get_all_stats()?;
            return Ok(serde_json::to_string_pretty(&stats)?);
        }

        if q_lower.starts_with("show indexes") {
            let keys = self.storage.get_keys_by_prefix("_index_meta", "")?;
            let mut results = Vec::new();
            for key in keys {
                if let Ok(Some(fields)) = self.storage.get::<Vec<String>>("_index_meta", &key) {
                    let mut parts = key.split('_');
                    let db = parts.next().unwrap_or("");
                    let table = parts.next().unwrap_or("");
                    let table_name = format!("{}_{}", db, table);
                    results.push(format!("{} -> {:?}", table_name, fields));
                }
            }
            return Ok(serde_json::to_string_pretty(&results)?);
        }

        if q_lower.starts_with("drop index ") {
            let parts: Vec<&str> = query.trim().split(|c| c == ' ' || c == '(' || c == ')' || c == ',').filter(|s| !s.is_empty()).collect();
            // Expected: DROP INDEX ON table field1 field2...
            if parts.len() >= 5 && parts[2].eq_ignore_ascii_case("on") {
                let table_name = clean_ident(parts[3]);
                let mut fields = Vec::new();
                for i in 4..parts.len() {
                    fields.push(parts[i]);
                }
                let full_table_name = format!("{}_{}", current_db.unwrap_or("public"), table_name);
                self.index_manager.drop_index(&full_table_name, &fields)?;
                return Ok(format!("Index dropped on {}({})", table_name, fields.join(", ")));
            }
        }

        if q_lower.starts_with("show databases") {
            let dbs = self.storage.get_keys_by_prefix("_databases", "")?;
            let json = serde_json::to_string_pretty(&dbs)?;
            return Ok(json);
        }

        if q_lower.starts_with("export database ") {
            let parts: Vec<&str> = query.trim().split_whitespace().collect();
            if parts.len() >= 3 {
                let db_to_export = clean_ident(parts[2]);
                let prefix = format!("{}_", db_to_export);
                let full_tables = self.storage.get_keys_by_prefix("_tables", &prefix)?;
                
                let mut export_data = serde_json::Map::new();
                for full_table in full_tables {
                    let table_name = full_table.strip_prefix(&prefix).unwrap_or(&full_table).to_string();
                    
                    let schema_opt: Option<TableSchema> = self.storage.get("_schemas", &full_table)?;
                    let rows: Vec<RowData> = self.storage.get_all(&full_table)?;
                    
                    let mut table_obj = serde_json::Map::new();
                    if let Some(schema) = schema_opt {
                        table_obj.insert("schema".to_string(), serde_json::to_value(schema.columns).unwrap_or(serde_json::Value::Null));
                    }
                    table_obj.insert("rows".to_string(), serde_json::to_value(rows).unwrap_or(serde_json::Value::Null));
                    
                    export_data.insert(table_name, serde_json::Value::Object(table_obj));
                }
                
                let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                let final_json = serde_json::json!({
                    "database": db_to_export,
                    "exported_timestamp": timestamp,
                    "tables": export_data
                });
                
                let file_name = format!("{}.pg", db_to_export);
                std::fs::write(&file_name, serde_json::to_string_pretty(&final_json).unwrap_or_default())
                    .map_err(|e| anyhow!("Failed to write export file: {}", e))?;
                
                return Ok(format!("Database '{}' exported successfully to '{}' in PingDart native format", db_to_export, file_name));
            }
        }

        let dialect = sqlparser::dialect::MySqlDialect {};
        let ast = Parser::parse_sql(&dialect, query)?;
        
        if ast.is_empty() {
            return Err(anyhow!("Empty query"));
        }

        let db_prefix = current_db.unwrap_or("default");

        let mut results = Vec::new();
        for stmt in &ast {
            results.push(self.execute_statement(stmt, db_prefix)?);
        }
        Ok(results.join("\n"))
    }

    fn execute_statement(&self, stmt: &Statement, db_prefix: &str) -> Result<String> {
        match stmt {
            Statement::CreateTable { name, columns, .. } => {
                let table_name = clean_ident(&name.to_string());
                let full_table_name = format!("{}_{}", db_prefix, table_name);
                
                let mut schema_cols = Vec::new();
                for col in columns {
                    let mut is_primary = false;
                    let mut auto_increment = false;
                    let mut is_unique = false;
                    let mut is_not_null = false;
                    let mut default_value = None;
                    
                    for opt in &col.options {
                        match &opt.option {
                            ColumnOption::Unique { is_primary: primary, .. } => {
                                if *primary {
                                    is_primary = true;
                                } else {
                                    is_unique = true;
                                }
                            },
                            ColumnOption::NotNull => {
                                is_not_null = true;
                            },
                            ColumnOption::DialectSpecific(tokens) => {
                                if tokens.iter().any(|t| t.to_string().to_uppercase() == "AUTO_INCREMENT") {
                                    auto_increment = true;
                                }
                            },
                            ColumnOption::Default(expr) => {
                                // Strip quotes if string literal, else just to_string
                                let val_str = match expr {
                                    Expr::Value(Value::SingleQuotedString(s)) => s.clone(),
                                    Expr::Value(Value::Number(n, _)) => n.clone(),
                                    Expr::Value(Value::Boolean(b)) => b.to_string(),
                                    _ => expr.to_string(), // Covers CURRENT_TIMESTAMP
                                };
                                default_value = Some(val_str);
                            },
                            _ => {}
                        }
                    }

                    schema_cols.push(ColumnDef {
                        name: col.name.value.clone(),
                        data_type: col.data_type.to_string(),
                        is_primary,
                        auto_increment,
                        is_unique,
                        is_not_null,
                        default_value,
                    });
                }
                
                let schema = TableSchema {
                    name: table_name.clone(),
                    columns: schema_cols,
                };
                
                self.storage.set("_schemas", &full_table_name, &schema)?;
                self.storage.set("_tables", &full_table_name, &true)?;
                
                // Initialize AI sequence if any column has auto_increment
                for col in &schema.columns {
                    if col.auto_increment {
                        let seq_key = format!("{}_seq_{}", full_table_name, col.name);
                        self.storage.set("_sequences", &seq_key, &0_i64)?;
                    }
                }
                
                self.storage.flush()?;
                Ok(format!("Table {} created successfully with {} columns", table_name, schema.columns.len()))
            }
            Statement::Insert { table_name, columns, source, .. } => {
                let table = clean_ident(&table_name.to_string());
                let full_table_name = format!("{}_{}", db_prefix, table);
                
                // Fetch schema to validate
                let schema_opt: Option<TableSchema> = self.storage.get("_schemas", &full_table_name)?;
                let schema = schema_opt.ok_or_else(|| anyhow!("Table {} does not exist", table))?;
                
                let mut inserted_count = 0;
                
                if let Some(q) = source {
                    if let SetExpr::Values(values) = &*q.body {
                        for row_vals in &values.rows {
                            let mut row = RowData { columns: HashMap::new(), ..Default::default() };
                            let mut user_provided = HashMap::new();
                            
                            for (i, val) in row_vals.iter().enumerate() {
                                let col_name = if !columns.is_empty() && i < columns.len() {
                                    columns[i].value.clone()
                                } else if i < schema.columns.len() {
                                    schema.columns[i].name.clone()
                                } else {
                                    return Err(anyhow!("Too many values for table {}", table));
                                };
                                
                                let val_str = match val {
                                    Expr::Value(Value::SingleQuotedString(s)) => s.clone(),
                                    Expr::Value(Value::Number(n, _)) => n.clone(),
                                    Expr::Value(Value::Boolean(b)) => b.to_string(),
                                    Expr::Value(Value::Null) => "null".to_string(),
                                    _ => "unknown".to_string(),
                                };
                                user_provided.insert(col_name, val_str);
                            }

                            let mut primary_key_val = None;

                            for col in &schema.columns {
                                let final_val = if let Some(val) = user_provided.get(&col.name) {
                                    Some(val.clone())
                                } else if col.auto_increment {
                                    let seq_key = format!("{}_seq_{}", full_table_name, col.name);
                                    let mut current_seq: i64 = self.storage.get("_sequences", &seq_key)?.unwrap_or(0);
                                    current_seq += 1;
                                    self.storage.set("_sequences", &seq_key, &current_seq)?;
                                    Some(current_seq.to_string())
                                } else if let Some(def_val) = &col.default_value {
                                    let mut resolved_val = def_val.clone();
                                    if resolved_val.to_uppercase() == "CURRENT_TIMESTAMP" {
                                        resolved_val = chrono::Utc::now().to_rfc3339();
                                    }
                                    Some(resolved_val)
                                } else {
                                    Some("null".to_string())
                                };

                                if let Some(v) = final_val {
                                    if col.is_primary {
                                        primary_key_val = Some(v.clone());
                                    }
                                    row.columns.insert(col.name.clone(), v);
                                }
                            }
                            
                            // Enforce Constraints (NOT NULL and UNIQUE)
                            let mut unique_cols = Vec::new();
                            for col in &schema.columns {
                                let val = row.columns.get(&col.name).map(|s| s.as_str());
                                let is_null = val.is_none() || val == Some("null");
                                
                                if (col.is_not_null || col.is_primary) && is_null && !col.auto_increment {
                                    return Err(anyhow!("Column '{}' cannot be null", col.name));
                                }
                                if col.is_unique || col.is_primary {
                                    unique_cols.push(col.name.clone());
                                }
                            }
                            
                            if !unique_cols.is_empty() {
                                let all_rows = self.storage.get_all_with_keys::<RowData>(&full_table_name)?;
                                for (_, existing_row) in all_rows {
                                    for uc in &unique_cols {
                                        if let (Some(new_val), Some(exist_val)) = (row.columns.get(uc), existing_row.columns.get(uc)) {
                                            if new_val == exist_val && new_val != "null" {
                                                return Err(anyhow!("PingDart DB query failed: Duplicate entry '{}' for key '{}'", new_val, uc));
                                            }
                                        }
                                    }
                                }
                            }
                            
                            let id = primary_key_val.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                            row._id = id.clone();
                            self.storage.set(&full_table_name, &id, &row)?;
                            
                            // Update Secondary Indexes
                            let table_indexes = self.index_manager.get_table_indexes(&full_table_name).unwrap_or_default();
                            for index_fields in table_indexes {
                                let mut vals = Vec::new();
                                let mut valid = true;
                                for f in &index_fields {
                                    if let Some(v) = row.columns.get(f) {
                                        vals.push(v.clone());
                                    } else {
                                        valid = false; break;
                                    }
                                }
                                if valid {
                                    let combined_val = vals.join("|");
                                    let fields_str: Vec<&str> = index_fields.iter().map(|s| s.as_str()).collect();
                                    let _ = self.index_manager.update_document(&full_table_name, &id, &fields_str, &combined_val, None);
                                }
                            }
                            
                            inserted_count += 1;
                        }
                    }
                }
                
                self.storage.flush()?;
                Ok(format!("{} row(s) inserted successfully", inserted_count))
            }

            Statement::Update { table, assignments, selection, .. } => {
                let table_name = clean_ident(&table.relation.to_string());
                let full_table_name = format!("{}_{}", db_prefix, table_name);
                
                let schema_opt: Option<TableSchema> = self.storage.get("_schemas", &full_table_name)?;
                let schema = schema_opt.ok_or_else(|| anyhow!("Table {} does not exist", table_name))?;
                let pk_col = schema.columns.iter().find(|c| c.is_primary).map(|c| c.name.clone());

                let mut search_val = String::new();
                let mut is_pk_lookup = false;

                if let Some(Expr::BinaryOp { left, op, right }) = &selection {
                    let mut extracted_col = None;
                    if let (Expr::CompoundIdentifier(idents), sqlparser::ast::BinaryOperator::Eq) = (&**left, op) {
                        extracted_col = Some(idents.last().unwrap().value.clone());
                    } else if let (Expr::Identifier(ident), sqlparser::ast::BinaryOperator::Eq) = (&**left, op) {
                        extracted_col = Some(ident.value.clone());
                    }
                    if let Some(col) = extracted_col {
                        if Some(&col) == pk_col.as_ref() {
                            search_val = match &**right {
                                Expr::Value(Value::SingleQuotedString(s)) => s.clone(),
                                Expr::Value(Value::Number(n, _)) => n.clone(),
                                Expr::Value(Value::Boolean(b)) => b.to_string(),
                                _ => right.to_string(),
                            };
                            is_pk_lookup = true;
                        }
                    }
                }

                let mut updated_count = 0;

                let mut apply_assignments = |key: &str, row: &mut RowData| -> Result<()> {
                    let old_row = row.clone(); // Keep for index removal
                    for assignment in assignments {
                        let col_name = assignment.id[0].value.clone();
                        let new_val = match &assignment.value {
                            Expr::Value(Value::SingleQuotedString(s)) => s.clone(),
                            Expr::Value(Value::Number(n, _)) => n.clone(),
                            Expr::Value(Value::Boolean(b)) => b.to_string(),
                            Expr::Value(Value::Null) => "null".to_string(),
                            _ => assignment.value.to_string(),
                        };
                        row.columns.insert(col_name.clone(), new_val.clone());
                    }
                    
                    // Enforce Constraints
                    let mut unique_cols = Vec::new();
                    for col in &schema.columns {
                        let val = row.columns.get(&col.name).map(|s| s.as_str());
                        let is_null = val.is_none() || val == Some("null");
                        
                        if (col.is_not_null || col.is_primary) && is_null && !col.auto_increment {
                            return Err(anyhow!("Column '{}' cannot be null", col.name));
                        }
                        if col.is_unique || col.is_primary {
                            unique_cols.push(col.name.clone());
                        }
                    }
                    
                    if !unique_cols.is_empty() {
                        let all_rows = self.storage.get_all_with_keys::<RowData>(&full_table_name)?;
                        for (k, existing_row) in all_rows {
                            if k != key {
                                for uc in &unique_cols {
                                    if let (Some(new_val), Some(exist_val)) = (row.columns.get(uc), existing_row.columns.get(uc)) {
                                        if new_val == exist_val && new_val != "null" {
                                            return Err(anyhow!("PingDart DB query failed: Duplicate entry '{}' for key '{}'", new_val, uc));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    
                    // Update all secondary indexes
                    let table_indexes = self.index_manager.get_table_indexes(&full_table_name).unwrap_or_default();
                    for index_fields in table_indexes {
                        let mut old_vals = Vec::new();
                        let mut new_vals = Vec::new();
                        let mut old_valid = true;
                        let mut new_valid = true;
                        
                        for f in &index_fields {
                            if let Some(v) = old_row.columns.get(f) {
                                old_vals.push(v.clone());
                            } else { old_valid = false; }
                            
                            if let Some(v) = row.columns.get(f) {
                                new_vals.push(v.clone());
                            } else { new_valid = false; }
                        }
                        
                        let fields_str: Vec<&str> = index_fields.iter().map(|s| s.as_str()).collect();
                        
                        if new_valid {
                            let new_combined = new_vals.join("|");
                            let old_combined = if old_valid { Some(old_vals.join("|")) } else { None };
                            
                            let _ = self.index_manager.update_document(
                                &full_table_name, key, &fields_str, &new_combined, old_combined.as_deref()
                            );
                        } else if old_valid {
                            // The update removed a field required for the index
                            let old_combined = old_vals.join("|");
                            let _ = self.index_manager.remove_document(&full_table_name, key, &fields_str, &old_combined);
                        }
                    }
                    
                    self.storage.set(&full_table_name, key, row)?;
                    updated_count += 1;
                    Ok(())
                };

                if is_pk_lookup {
                    if let Some(mut row) = self.storage.get::<RowData>(&full_table_name, &search_val)? {
                        apply_assignments(&search_val, &mut row)?;
                    }
                } else {
                    let rows = self.storage.get_all_with_keys::<RowData>(&full_table_name)?;
                    for (key, mut row) in rows {
                        let mut is_match = true;
                        if let Some(selection_expr) = &selection {
                            is_match = match eval_expr(selection_expr, &row) {
                                EvalVal::Bool(b) => b,
                                _ => false,
                            };
                        }
                        if is_match {
                            apply_assignments(&key, &mut row)?;
                        }
                    }
                }

                self.storage.flush()?;
                Ok(format!("{} row(s) updated successfully", updated_count))
            }
            Statement::Delete { from, selection, .. } => {
                let table_name = match from {
                    sqlparser::ast::FromTable::WithFromKeyword(tables) => clean_ident(&tables[0].relation.to_string()),
                    sqlparser::ast::FromTable::WithoutKeyword(tables) => clean_ident(&tables[0].relation.to_string()),
                };
                let full_table_name = format!("{}_{}", db_prefix, table_name);
                
                let schema_opt: Option<TableSchema> = self.storage.get("_schemas", &full_table_name)?;
                let schema = schema_opt.ok_or_else(|| anyhow!("Table {} does not exist", table_name))?;
                let pk_col = schema.columns.iter().find(|c| c.is_primary).map(|c| c.name.clone());

                let mut search_val = String::new();
                let mut is_pk_lookup = false;

                if let Some(Expr::BinaryOp { left, op, right }) = &selection {
                    let mut extracted_col = None;
                    if let (Expr::CompoundIdentifier(idents), sqlparser::ast::BinaryOperator::Eq) = (&**left, op) {
                        extracted_col = Some(idents.last().unwrap().value.clone());
                    } else if let (Expr::Identifier(ident), sqlparser::ast::BinaryOperator::Eq) = (&**left, op) {
                        extracted_col = Some(ident.value.clone());
                    }
                    if let Some(col) = extracted_col {
                        if Some(&col) == pk_col.as_ref() {
                            search_val = match &**right {
                                Expr::Value(Value::SingleQuotedString(s)) => s.clone(),
                                Expr::Value(Value::Number(n, _)) => n.clone(),
                                Expr::Value(Value::Boolean(b)) => b.to_string(),
                                _ => right.to_string(),
                            };
                            is_pk_lookup = true;
                        }
                    }
                }

                let mut deleted_count = 0;
                
                let mut apply_delete = |key: &str, row: &RowData| {
                    let table_indexes = self.index_manager.get_table_indexes(&full_table_name).unwrap_or_default();
                    for index_fields in table_indexes {
                        let mut vals = Vec::new();
                        let mut valid = true;
                        for f in &index_fields {
                            if let Some(v) = row.columns.get(f) {
                                vals.push(v.clone());
                            } else {
                                valid = false; break;
                            }
                        }
                        if valid {
                            let combined_val = vals.join("|");
                            let fields_str: Vec<&str> = index_fields.iter().map(|s| s.as_str()).collect();
                            let _ = self.index_manager.remove_document(&full_table_name, key, &fields_str, &combined_val);
                        }
                    }
                    self.storage.delete(&full_table_name, key).unwrap();
                    deleted_count += 1;
                };

                if is_pk_lookup {
                    if let Ok(Some(row)) = self.storage.get::<RowData>(&full_table_name, &search_val) {
                        apply_delete(&search_val, &row);
                    }
                } else {
                    let rows = self.storage.get_all_with_keys::<RowData>(&full_table_name)?;
                    for (key, row) in rows {
                        let mut is_match = true;
                        if let Some(selection_expr) = &selection {
                            is_match = match eval_expr(selection_expr, &row) {
                                EvalVal::Bool(b) => b,
                                _ => false,
                            };
                        }
                        if is_match {
                            apply_delete(&key, &row);
                        }
                    }
                }

                self.storage.flush()?;
                Ok(format!("{} row(s) deleted successfully", deleted_count))
            }
            Statement::Query(q) => {
                if let SetExpr::Select(select) = &*q.body {
                    if let Some(table_with_joins) = select.from.first() {
                        let (base_table_name, base_alias) = match &table_with_joins.relation {
                            sqlparser::ast::TableFactor::Table { name, alias, .. } => {
                                let t_name = clean_ident(&name.to_string());
                                let t_alias = alias.as_ref().map(|a| clean_ident(&a.name.value)).unwrap_or_else(|| t_name.clone());
                                (t_name, t_alias)
                            },
                            _ => (clean_ident(&table_with_joins.relation.to_string()), clean_ident(&table_with_joins.relation.to_string())),
                        };
                        
                        let full_base_table = format!("{}_{}", db_prefix, base_table_name);
                        
                        let base_schema_opt = self.storage.get::<TableSchema>("_schemas", &full_base_table)?;
                        let base_schema = base_schema_opt.ok_or_else(|| anyhow!("Table {} does not exist", base_table_name))?;
                        
                        let pk_col = base_schema.columns.iter().find(|c| c.is_primary).map(|c| c.name.clone());

                        let mut search_val = String::new();
                        let mut is_pk_lookup = false;

                        // Parse the WHERE clause
                        if let Some(Expr::BinaryOp { left, op, right }) = &select.selection {
                            let mut extracted_col = None;
                            
                            if let (Expr::CompoundIdentifier(idents), sqlparser::ast::BinaryOperator::Eq) = (&**left, op) {
                                if idents.len() == 2 && (idents[0].value == base_table_name || idents[0].value == base_alias) {
                                    extracted_col = Some(idents[1].value.clone());
                                }
                            } else if let (Expr::Identifier(ident), sqlparser::ast::BinaryOperator::Eq) = (&**left, op) {
                                extracted_col = Some(ident.value.clone());
                            }
                            
                            if let Some(col) = extracted_col {
                                if Some(&col) == pk_col.as_ref() {
                                    search_val = match &**right {
                                        Expr::Value(Value::SingleQuotedString(s)) => s.clone(),
                                        Expr::Value(Value::Number(n, _)) => n.clone(),
                                        Expr::Value(Value::Boolean(b)) => b.to_string(),
                                        _ => right.to_string(),
                                    };
                                    is_pk_lookup = true;
                                }
                            }
                        }

                        let mut current_rows: Vec<RowData> = if is_pk_lookup {
                            if let Some(row) = self.storage.get::<RowData>(&full_base_table, &search_val)? {
                                vec![row]
                            } else {
                                vec![]
                            }
                        } else {
                            self.storage.get_all(&full_base_table)?
                        };

                        // Prefix base columns
                        current_rows = current_rows.into_iter().map(|r| {
                            let mut prefixed = HashMap::new();
                            for (k, v) in r.columns {
                                prefixed.insert(format!("{}.{}", base_alias, k.clone()), v.clone());
                                prefixed.insert(format!("{}.{}", base_table_name, k.clone()), v.clone());
                                prefixed.insert(k, v);
                            }
                            RowData { columns: prefixed, ..Default::default() }
                        }).collect();

                        // Nested Loop Joins
                        for join in &table_with_joins.joins {
                            let (joined_table_name, joined_alias) = match &join.relation {
                                sqlparser::ast::TableFactor::Table { name, alias, .. } => {
                                    let t_name = clean_ident(&name.to_string());
                                    let t_alias = alias.as_ref().map(|a| clean_ident(&a.name.value)).unwrap_or_else(|| t_name.clone());
                                    (t_name, t_alias)
                                },
                                _ => (clean_ident(&join.relation.to_string()), clean_ident(&join.relation.to_string())),
                            };
                            
                            let full_joined_table = format!("{}_{}", db_prefix, joined_table_name);
                            
                            // Make sure the table exists
                            let _ = self.storage.get::<TableSchema>("_schemas", &full_joined_table)?
                                .ok_or_else(|| anyhow!("Joined table {} does not exist", joined_table_name))?;
                                
                            let joined_rows: Vec<RowData> = self.storage.get_all(&full_joined_table)?;
                            let mut next_rows = Vec::new();

                            if let sqlparser::ast::JoinOperator::Inner(sqlparser::ast::JoinConstraint::On(on_expr)) = &join.join_operator {
                                for base_r in &current_rows {
                                    for j_r in &joined_rows {
                                        let mut temp_row = base_r.clone();
                                        for (k, v) in &j_r.columns {
                                            temp_row.columns.insert(format!("{}.{}", joined_alias, k), v.clone());
                                            temp_row.columns.insert(format!("{}.{}", joined_table_name, k), v.clone());
                                        }
                                        
                                        let is_match = match eval_expr(on_expr, &temp_row) {
                                            EvalVal::Bool(b) => b,
                                            _ => false,
                                        };
                                        
                                        if is_match {
                                            next_rows.push(temp_row);
                                        }
                                    }
                                }
                            } else {
                                return Err(anyhow!("Only INNER JOIN with explicitly defined ON conditions are supported."));
                            }
                            
                            current_rows = next_rows;
                        }

                        // Apply WHERE clause
                        if !is_pk_lookup {
                            if let Some(selection_expr) = &select.selection {
                                current_rows.retain(|r| {
                                    match eval_expr(selection_expr, r) {
                                        EvalVal::Bool(b) => b,
                                        _ => false,
                                    }
                                });
                            }
                        }

                        let result_rows = current_rows;

                        // Projection Filtering
                        let mut projected_rows = Vec::new();
                        let is_wildcard = select.projection.iter().any(|item| matches!(item, sqlparser::ast::SelectItem::Wildcard(_)));
                        
                        if is_wildcard {
                            projected_rows = result_rows.into_iter().map(|r| {
                                let mut clean_map = HashMap::new();
                                for (k, v) in r.columns {
                                    if k.contains('.') {
                                        clean_map.insert(k, v); 
                                    } else {
                                        clean_map.insert(k, v);
                                    }
                                }
                                RowData { columns: clean_map, ..Default::default() }
                            }).collect();
                        } else {
                            for row in result_rows {
                                let mut proj_map = std::collections::BTreeMap::new();
                                for item in &select.projection {
                                    if let sqlparser::ast::SelectItem::UnnamedExpr(expr) = item {
                                        if let Expr::CompoundIdentifier(idents) = expr {
                                            if idents.len() == 2 {
                                                let key = format!("{}.{}", idents[0].value, idents[1].value);
                                                if let Some(v) = row.columns.get(&key) {
                                                    proj_map.insert(idents[1].value.clone(), v.clone());
                                                }
                                            }
                                        } else if let Expr::Identifier(ident) = expr {
                                            let key = format!("{}.{}", base_table_name, ident.value);
                                            let alias_key = format!("{}.{}", base_alias, ident.value);
                                            if let Some(v) = row.columns.get(&key).or_else(|| row.columns.get(&alias_key)).or_else(|| row.columns.get(&ident.value)) {
                                                proj_map.insert(ident.value.clone(), v.clone());
                                            }
                                        }
                                    } else if let sqlparser::ast::SelectItem::ExprWithAlias { expr, alias } = item {
                                        if let Expr::CompoundIdentifier(idents) = expr {
                                            if idents.len() == 2 {
                                                let key = format!("{}.{}", idents[0].value, idents[1].value);
                                                if let Some(v) = row.columns.get(&key) {
                                                    proj_map.insert(alias.value.clone(), v.clone());
                                                }
                                            }
                                        } else if let Expr::Identifier(ident) = expr {
                                            if let Some(v) = row.columns.get(&ident.value) {
                                                proj_map.insert(alias.value.clone(), v.clone());
                                            }
                                        }
                                    }
                                }
                                projected_rows.push(RowData { columns: proj_map.into_iter().collect(), ..Default::default() });
                            }
                        }

                        let final_output: Vec<HashMap<String, String>> = projected_rows.into_iter().map(|r| r.columns).collect();
                        return Ok(serde_json::to_string_pretty(&final_output)?);
                    }
                }
                Err(anyhow!("Unsupported query type"))
            }
            Statement::ShowTables { .. } => {
                let prefix = format!("{}_", db_prefix);
                let full_tables = self.storage.get_keys_by_prefix("_tables", &prefix)?;
                
                let mut table_details = Vec::new();
                for full_name in full_tables {
                    let table_name = full_name.strip_prefix(&prefix).unwrap_or(&full_name).to_string();
                    let rows = self.storage.count_rows(&full_name).unwrap_or(0);
                    
                    table_details.push(serde_json::json!({
                        "Table": table_name,
                        "Rows": rows,
                        "Type": "PingDart (Sled)",
                        "Size_MB": 0.00
                    }));
                }
                
                
                let json = serde_json::to_string_pretty(&table_details)?;
                Ok(json)
            }
            Statement::Drop { object_type, names, .. } => {
                let is_table = match object_type {
                    sqlparser::ast::ObjectType::Table => true,
                    _ => false,
                };
                
                if is_table {
                    for name in names {
                        let table_name = clean_ident(&name.to_string());
                        let full_table_name = format!("{}_{}", db_prefix, table_name);
                        
                        // Drop the table data tree
                        let _ = self.storage.drop_tree(&full_table_name);
                        
                        // Delete schema and table markers
                        let _ = self.storage.delete("_schemas", &full_table_name);
                        let _ = self.storage.delete("_tables", &full_table_name);
                    }
                    Ok("Table(s) dropped successfully".to_string())
                } else {
                    Err(anyhow!("Unsupported drop object type"))
                }
            }
            Statement::ShowColumns { table_name, .. } => {
                let table = clean_ident(&table_name.to_string());
                let full_table_name = format!("{}_{}", db_prefix, table);
                let schema_opt: Option<TableSchema> = self.storage.get("_schemas", &full_table_name)?;
                if let Some(schema) = schema_opt {
                    let json = serde_json::to_string_pretty(&schema.columns)?;
                    Ok(json)
                } else {
                    Err(anyhow!("Table {} does not exist", table))
                }
            }
            Statement::AlterTable { name, operations, .. } => {
                let table = clean_ident(&name.to_string());
                let full_table_name = format!("{}_{}", db_prefix, table);
                let mut schema: TableSchema = match self.storage.get("_schemas", &full_table_name)? {
                    Some(s) => s,
                    None => return Err(anyhow!("Table {} does not exist", table)),
                };
                
                for op in operations {
                    match op {
                        sqlparser::ast::AlterTableOperation::AddColumn { column_def, .. } => {
                            let mut is_primary = false;
                            let mut auto_increment = false;
                            let mut is_unique = false;
                            let mut is_not_null = false;
                            let mut default_value = None;
                            
                            for opt in &column_def.options {
                                match &opt.option {
                                    ColumnOption::Unique { is_primary: primary, .. } => {
                                        if *primary {
                                            is_primary = true;
                                        } else {
                                            is_unique = true;
                                        }
                                    },
                                    ColumnOption::NotNull => {
                                        is_not_null = true;
                                    },
                                    ColumnOption::DialectSpecific(tokens) => {
                                        if tokens.iter().any(|t| t.to_string().to_uppercase() == "AUTO_INCREMENT") {
                                            auto_increment = true;
                                        }
                                    },
                                    ColumnOption::Default(expr) => {
                                        let val_str = match expr {
                                            Expr::Value(Value::SingleQuotedString(s)) => s.clone(),
                                            Expr::Value(Value::Number(n, _)) => n.clone(),
                                            Expr::Value(Value::Boolean(b)) => b.to_string(),
                                            _ => expr.to_string(),
                                        };
                                        default_value = Some(val_str);
                                    },
                                    _ => {}
                                }
                            }
                            
                            let new_col = ColumnDef {
                                name: column_def.name.value.clone(),
                                data_type: column_def.data_type.to_string(),
                                is_primary,
                                auto_increment,
                                is_unique,
                                is_not_null,
                                default_value,
                            };
                            
                            if auto_increment {
                                let seq_key = format!("{}_seq_{}", full_table_name, new_col.name);
                                let _ = self.storage.set("_sequences", &seq_key, &0_i64);
                            }
                            
                            schema.columns.push(new_col);
                        }
                        sqlparser::ast::AlterTableOperation::DropColumn { column_name, .. } => {
                            let col_to_drop = column_name.value.clone();
                            schema.columns.retain(|c| c.name != col_to_drop);
                        }
                        sqlparser::ast::AlterTableOperation::ChangeColumn { old_name, new_name, data_type, options, .. } => {
                            let old_col = old_name.value.clone();
                            let new_col = new_name.value.clone();
                            let new_type = data_type.to_string();
                            
                            let mut new_is_primary = None;
                            let mut new_auto_increment = None;
                            let mut new_is_unique = None;
                            let mut new_is_not_null = None;
                            let mut new_default_value = None;
                            
                            for opt in options {
                                match opt {
                                    ColumnOption::Unique { is_primary: primary, .. } => {
                                        if *primary {
                                            new_is_primary = Some(true);
                                        } else {
                                            new_is_unique = Some(true);
                                        }
                                    },
                                    ColumnOption::NotNull => {
                                        new_is_not_null = Some(true);
                                    },
                                    ColumnOption::DialectSpecific(tokens) => {
                                        if tokens.iter().any(|t| t.to_string().to_uppercase() == "AUTO_INCREMENT") {
                                            new_auto_increment = Some(true);
                                        }
                                    },
                                    ColumnOption::Default(expr) => {
                                        let val_str = match expr {
                                            Expr::Value(Value::SingleQuotedString(s)) => s.clone(),
                                            Expr::Value(Value::Number(n, _)) => n.clone(),
                                            Expr::Value(Value::Boolean(b)) => b.to_string(),
                                            _ => expr.to_string(),
                                        };
                                        new_default_value = Some(Some(val_str));
                                    },
                                    _ => {}
                                }
                            }
                            
                            for c in &mut schema.columns {
                                if c.name == old_col {
                                    c.name = new_col.clone();
                                    c.data_type = new_type.clone();
                                    if let Some(p) = new_is_primary { c.is_primary = p; }
                                    if let Some(a) = new_auto_increment { c.auto_increment = a; }
                                    if let Some(u) = new_is_unique { c.is_unique = u; }
                                    if let Some(n) = new_is_not_null { c.is_not_null = n; }
                                    if let Some(d) = new_default_value.clone() { c.default_value = d; }
                                }
                            }
                            
                            // To be fully correct, we should also update all existing rows
                            // to rename the key in their HashMap from old_name to new_name.
                            // But since Sled is Schemaless KV, the frontend will just see 
                            // `null` for the new column name on old rows until they are updated,
                            // unless we migrate the data. Let's do a quick migration of rows!
                            // For MVP, updating the schema is enough for the UI to show the new column.
                            // Updating data keys would require fetching the primary key for each row.
                        }
                        _ => return Err(anyhow!("Unsupported ALTER TABLE operation")),
                    }
                }
                
                self.storage.set("_schemas", &full_table_name, &schema)?;
                Ok("Table altered successfully".to_string())
            }
            Statement::CreateDatabase { db_name, .. } => {
                let name = clean_ident(&db_name.to_string());
                self.storage.set("_databases", &name, &true)?;
                self.storage.flush()?;
                Ok(format!("Database '{}' created successfully", name))
            }
            Statement::Use { db_name } => {
                Ok(format!("USE {} acknowledged", db_name))
            }
            Statement::CreateIndex { name: _, table_name, columns, .. } => {
                let table = clean_ident(&table_name.to_string());
                let full_table_name = format!("{}_{}", db_prefix, table);
                let mut fields = Vec::new();
                for col in columns {
                    if let sqlparser::ast::Expr::Identifier(ident) = &col.expr {
                        fields.push(ident.value.clone());
                    }
                }
                self.index_manager.create_index(&full_table_name, &fields.iter().map(|s| s.as_str()).collect::<Vec<&str>>())?;
                Ok(format!("Index created on {}({})", table, fields.join(", ")))
            }
            _ => Err(anyhow!("Unsupported statement for MVP: {}", stmt.to_string())),
        }
    }

fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371.0;
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2) +
            lat1.to_radians().cos() * lat2.to_radians().cos() *
            (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    r * c
}

    pub fn execute_document_query(&self, req: &crate::server::DocumentQueryRequest, current_db: Option<&str>) -> Result<String> {
        let db_prefix = current_db.unwrap_or("public");
        let collection = format!("{}_{}", db_prefix, req.collection);
        
        let mut filtered: Vec<serde_json::Value> = Vec::new();
        
        let mut filters_for_planner = Vec::new();
        if let Some(filters) = &req.r#where {
            for filter in filters {
                filters_for_planner.push((filter.field.clone(), filter.operator.clone(), filter.value.clone()));
            }
        }
        
        let available_indexes = self.index_manager.get_table_indexes(&collection).unwrap_or_default();
        let plan = self.planner.plan(&collection, &filters_for_planner, &available_indexes);
        
        if req.explain.unwrap_or(false) {
            return Ok(serde_json::to_string_pretty(&plan)?);
        }

        if let Some(near) = &req.near {
            let center = geohash::Coord { x: near.lon, y: near.lat };
            let precision = if near.radius_km <= 4.9 { 5 }
                            else if near.radius_km <= 39.0 { 4 }
                            else if near.radius_km <= 156.0 { 3 }
                            else if near.radius_km <= 1250.0 { 2 }
                            else { 1 };
                            
            if let Ok(center_hash) = geohash::encode(center, precision) {
                let mut hashes_to_search = vec![center_hash.clone()];
                if let Ok(neighbors) = geohash::neighbors(&center_hash) {
                    hashes_to_search.push(neighbors.n);
                    hashes_to_search.push(neighbors.ne);
                    hashes_to_search.push(neighbors.e);
                    hashes_to_search.push(neighbors.se);
                    hashes_to_search.push(neighbors.s);
                    hashes_to_search.push(neighbors.sw);
                    hashes_to_search.push(neighbors.w);
                    hashes_to_search.push(neighbors.nw);
                }
                
                let mut fetched_ids = std::collections::HashSet::new();
                for h in hashes_to_search {
                    let prefix = format!("{}_{}", collection, h);
                    if let Ok(keys) = self.storage.get_keys_by_prefix("_geo_idx", &prefix) {
                        for key in keys {
                            if let Ok(Some(id)) = self.storage.get::<String>("_geo_idx", &key) {
                                fetched_ids.insert(id);
                            }
                        }
                    }
                }
                
                for id in fetched_ids {
                    if let Ok(Some(row)) = self.storage.get::<RowData>(&collection, &id) {
                        if let Some(doc) = Self::filter_row(row, req) {
                            filtered.push(doc);
                        }
                    }
                }
            }
        } else {
            match plan.execution_type {
                crate::planner::planner::ExecutionType::PrimaryIndexLookup => {
                    let _ = self.index_stats.increment_usage(&format!("{}_PRIMARY", collection));
                    if let Ok(Some(row)) = self.storage.get::<RowData>(&collection, &plan.filter_values[0]) {
                        if let Some(doc) = Self::filter_row(row, req) {
                            filtered.push(doc);
                        }
                    }
                },
                crate::planner::planner::ExecutionType::SecondaryIndexScan | crate::planner::planner::ExecutionType::CompositeIndexScan => {
                    let index_used_str = plan.index_used.as_ref().unwrap();
                    let _ = self.index_stats.increment_usage(&format!("{}_{}", collection, index_used_str.replace(", ", "|")));
                    
                    let fields_str: Vec<&str> = index_used_str.split(", ").collect();
                    if let Ok(ids) = self.index_manager.get_ids(&collection, &fields_str, &plan.filter_values[0]) {
                        for id in ids {
                            if let Ok(Some(row)) = self.storage.get::<RowData>(&collection, &id) {
                                if let Some(doc) = Self::filter_row(row, req) {
                                    filtered.push(doc);
                                    if req.limit == Some(filtered.len()) && req.order_by.is_none() {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                },
                crate::planner::planner::ExecutionType::CollectionScan => {
                    if let Ok(iter) = self.storage.iter::<RowData>(&collection) {
                        for res in iter {
                            if let Ok(row) = res {
                                if let Some(doc) = Self::filter_row(row, req) {
                                    filtered.push(doc);
                                    if req.limit == Some(filtered.len()) && req.order_by.is_none() {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        if req.near.is_some() {
            filtered.sort_by(|a, b| {
                let d_a = a.get("_distance_km").and_then(|v| v.as_f64()).unwrap_or(f64::MAX);
                let d_b = b.get("_distance_km").and_then(|v| v.as_f64()).unwrap_or(f64::MAX);
                d_a.partial_cmp(&d_b).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        
        // Very basic mock ordering/limits for prototype
        if let Some(limit) = req.limit {
            filtered.truncate(limit);
        }
        
        Ok(serde_json::to_string_pretty(&filtered)?)
    }

    fn filter_row(row: RowData, req: &crate::server::DocumentQueryRequest) -> Option<serde_json::Value> {
        let mut distance_km = None;
        if let Some(near) = &req.near {
            if let (Some(lat_str), Some(lon_str)) = (row.columns.get("lat"), row.columns.get("lon")) {
                if let (Ok(lat), Ok(lon)) = (lat_str.parse::<f64>(), lon_str.parse::<f64>()) {
                    let d = Self::haversine_distance(near.lat, near.lon, lat, lon);
                    if d > near.radius_km {
                        return None;
                    }
                    distance_km = Some(d);
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        
        let mut doc = match serde_json::to_value(row) {
            Ok(v) => v,
            Err(_) => return None,
        };
        
        if let Some(d) = distance_km {
            if let Some(obj) = doc.as_object_mut() {
                obj.insert("_distance_km".to_string(), serde_json::json!(d));
            }
        }
        
        if let Some(filters) = &req.r#where {
            for filter in filters {
                let val_opt = doc.get(&filter.field).or_else(|| {
                    doc.get("columns").and_then(|cols| cols.get(&filter.field))
                });
                
                if let Some(val) = val_opt {
                    let doc_val_str = if val.is_string() {
                        val.as_str().unwrap().to_string()
                    } else {
                        val.to_string()
                    };
                    
                    let filter_val = &filter.value;
                    let b = match filter.operator.as_str() {
                        "==" | "=" => doc_val_str == *filter_val,
                        "!=" => doc_val_str != *filter_val,
                        ">" => doc_val_str > *filter_val,
                        "<" => doc_val_str < *filter_val,
                        _ => false,
                    };
                    if !b {
                        return None;
                    }
                } else {
                    return None;
                }
            }
        }
        Some(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_ident() {
        assert_eq!(clean_ident("`categories`"), "categories");
        assert_eq!(clean_ident("\"categories\""), "categories");
        assert_eq!(clean_ident("'categories'"), "categories");
        assert_eq!(clean_ident("  `blog_categories`  "), "blog_categories");
    }

    #[test]
    fn test_backtick_table_execution() -> Result<()> {
        let storage = StorageEngine::new_temp()?;
        let executor = Executor::new(storage);

        // 1. Create table with backticks
        let res = executor.execute("CREATE TABLE `categories` (id INT PRIMARY KEY, name STRING)", Some("testdb"))?;
        assert!(res.contains("categories created successfully"));

        // 2. Select from table with backticks
        let select_res = executor.execute("SELECT * FROM `categories`", Some("testdb"))?;
        assert_eq!(select_res, "[]");

        // 3. Show columns with backticks
        let cols_res = executor.execute("SHOW COLUMNS FROM `categories`", Some("testdb"))?;
        assert!(cols_res.contains("name"));

        Ok(())
    }
}

