use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use std::collections::HashMap;

use crate::api::data::resolve_tenant_db;
use crate::api::dto::{ColumnDefDto, CreateTableDto, TableSummaryDto};
use crate::api::error::{ApiError, ApiResult};
use crate::exec::{ColumnDef, TableSchema};
use crate::server::AppState;

/// GET /v1/schema/tables - PRD Section 6.2 List all tables in the current project/database
pub async fn list_tables(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<impl IntoResponse> {
    let db = resolve_tenant_db(&headers, params.get("database").map(|s| s.as_str()));
    let prefix = format!("{}_", db);

    let full_tables = match state.storage.get_keys_by_prefix("_tables", &prefix) {
        Ok(t) => t,
        Err(e) => return Err(ApiError::internal(format!("Storage error: {}", e))),
    };

    let mut summaries = Vec::new();

    for full_table in full_tables {
        let table_name = full_table.strip_prefix(&prefix).unwrap_or(&full_table).to_string();
        if table_name.starts_with('_') && table_name != "_storage_files" {
            continue;
        }

        // Get schema if registered
        let schema_opt = state.executor.get_table_schema(&full_table).ok().flatten();
        let columns = if let Some(schema) = schema_opt {
            schema
                .columns
                .into_iter()
                .map(|c| ColumnDefDto {
                    name: c.name,
                    data_type: c.data_type,
                    is_primary: c.is_primary,
                    auto_increment: c.auto_increment,
                    is_unique: c.is_unique,
                    is_not_null: c.is_not_null,
                    default_value: c.default_value,
                })
                .collect()
        } else {
            Vec::new()
        };

        // Count active rows
        let mut row_count = 0;
        if let Ok(tree) = state.storage.db().open_tree(&full_table) {
            for item in tree.iter() {
                if let Ok((_, bytes)) = item {
                    if let Ok(row) = crate::exec::RowData::try_from_bytes(&bytes) {
                        if !row._deleted {
                            row_count += 1;
                        }
                    }
                }
            }
        }

        summaries.push(TableSummaryDto {
            name: table_name,
            full_name: full_table,
            database: db.clone(),
            row_count,
            columns,
        });
    }

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "success",
            "database": db,
            "count": summaries.len(),
            "tables": summaries
        })),
    ))
}

/// POST /v1/schema/tables - PRD Section 6.2 Create a new table with schema definition
pub async fn create_table(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Json(payload): Json<CreateTableDto>,
) -> ApiResult<impl IntoResponse> {
    let db = resolve_tenant_db(&headers, params.get("database").map(|s| s.as_str()));
    let table_name = payload.name.trim().to_string();

    if table_name.is_empty() {
        return Err(ApiError::bad_request("Table name cannot be empty"));
    }

    let full_table_name = format!("{}_{}", db, table_name);

    // Convert columns
    let mut columns: Vec<ColumnDef> = payload
        .columns
        .into_iter()
        .map(|c| ColumnDef {
            name: c.name,
            data_type: c.data_type,
            is_primary: c.is_primary,
            auto_increment: c.auto_increment,
            is_unique: c.is_unique,
            is_not_null: c.is_not_null,
            default_value: c.default_value,
            generated_from: None,
            using_model: None,
        })
        .collect();

    // Ensure default primary key 'id' exists if no primary key specified
    if !columns.iter().any(|c| c.is_primary) {
        columns.insert(
            0,
            ColumnDef {
                name: "id".to_string(),
                data_type: "TEXT".to_string(),
                is_primary: true,
                auto_increment: false,
                is_unique: true,
                is_not_null: true,
                default_value: None,
                generated_from: None,
                using_model: None,
            },
        );
    }

    let schema = TableSchema {
        name: table_name.clone(),
        columns: columns.clone(),
    };

    // Store in _schemas and register in _tables
    if let Err(e) = state.storage.set("_schemas", &full_table_name, &schema) {
        return Err(ApiError::internal(format!("Failed to save schema: {}", e)));
    }
    let _ = state.storage.set("_tables", &full_table_name, &true);
    let _ = state.storage.flush();

    // Emit CDC CREATE_TABLE event
    let schema_json = serde_json::to_value(&schema).unwrap_or_default();
    let _ = state.executor.cdc_streamer.emit(
        &db,
        &table_name,
        "CREATE_TABLE",
        &table_name,
        None,
        Some(schema_json.clone()),
    );

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "status": "success",
            "table": table_name,
            "database": db,
            "schema": schema_json
        })),
    ))
}

/// GET /v1/schema/tables/:table - PRD Section 6.2 Inspect table schema and metadata
pub async fn get_table(
    Path(table): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<impl IntoResponse> {
    let db = resolve_tenant_db(&headers, params.get("database").map(|s| s.as_str()));
    let full_table_name = format!("{}_{}", db, table);

    let schema_opt = state.executor.get_table_schema(&full_table_name).ok().flatten();

    // Count rows
    let mut row_count = 0;
    if let Ok(tree) = state.storage.db().open_tree(&full_table_name) {
        for item in tree.iter() {
            if let Ok((_, bytes)) = item {
                if let Ok(row) = crate::exec::RowData::try_from_bytes(&bytes) {
                    if !row._deleted {
                        row_count += 1;
                    }
                }
            }
        }
    }

    if schema_opt.is_none() && row_count == 0 {
        return Err(ApiError::table_not_found(&table));
    }

    let schema_val = schema_opt
        .map(|s| serde_json::to_value(s).unwrap_or_default())
        .unwrap_or_else(|| serde_json::json!({ "name": table, "columns": [] }));

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "success",
            "table": table,
            "database": db,
            "row_count": row_count,
            "schema": schema_val
        })),
    ))
}

/// DELETE /v1/schema/tables/:table - PRD Section 6.2 Drop table and its records
pub async fn drop_table(
    Path(table): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<impl IntoResponse> {
    let db = resolve_tenant_db(&headers, params.get("database").map(|s| s.as_str()));
    let full_table_name = format!("{}_{}", db, table);

    // Remove from _tables and _schemas
    let _ = state.storage.delete("_tables", &full_table_name);
    let _ = state.storage.delete("_schemas", &full_table_name);

    // Drop the sled tree and remove from metadata
    let _ = state.storage.drop_tree(&full_table_name);
    let _ = state.storage.flush();

    // Emit CDC DROP_TABLE event
    let _ = state.executor.cdc_streamer.emit(
        &db,
        &table,
        "DROP_TABLE",
        &table,
        None,
        None,
    );

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "success",
            "database": db,
            "dropped_table": table
        })),
    ))
}

/// POST /v1/schema/tables/:table/truncate - Empty all rows in a table
pub async fn truncate_table(
    Path(table): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<impl IntoResponse> {
    let db = resolve_tenant_db(&headers, params.get("database").map(|s| s.as_str()));
    let full_table_name = format!("{}_{}", db, table);

    let mut cleared_rows = 0;
    if let Ok(tree) = state.storage.db().open_tree(&full_table_name) {
        cleared_rows = tree.len();
        let _ = tree.clear();
    }
    let _ = state.storage.flush();

    // Emit CDC TRUNCATE event
    let _ = state.executor.cdc_streamer.emit(
        &db,
        &table,
        "TRUNCATE",
        &table,
        None,
        None,
    );

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "success",
            "database": db,
            "truncated_table": table,
            "cleared_rows": cleared_rows
        })),
    ))
}
