use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::api::dto::{FilterOp, PaginationMeta, RestDataListQuery, RestListResponse};
use crate::api::error::{ApiError, ApiResult};
use crate::server::AppState;

/// Helper to resolve the active tenant/database from headers or query parameters
pub fn resolve_tenant_db(headers: &HeaderMap, query_db: Option<&str>) -> String {
    if let Some(proj) = headers.get("x-pingdart-project").and_then(|h| h.to_str().ok()) {
        let clean = proj.trim();
        if !clean.is_empty() {
            return clean.to_string();
        }
    }
    query_db
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("default")
        .to_string()
}

/// GET /v1/data/:table - PRD Section 6.4 List records with rich queries, filtering, sorting, projection
pub async fn list_records(
    Path(table): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RestDataListQuery>,
) -> ApiResult<impl IntoResponse> {
    let db = resolve_tenant_db(&headers, query.database.as_deref());
    let full_table_name = format!("{}_{}", db, table);

    let tree = match state.storage.db().open_tree(&full_table_name) {
        Ok(t) => t,
        Err(e) => return Err(ApiError::internal(format!("Storage error: {}", e))),
    };

    // Parse select fields if provided
    let select_set: Option<HashSet<String>> = query.select.as_ref().map(|s| {
        s.split(',')
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty())
            .collect()
    });

    // Parse search text if provided
    let search_term = query
        .q
        .as_ref()
        .or(query.search.as_ref())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());

    let mut all_records = Vec::new();

    for item in tree.iter() {
        if let Ok((key_bytes, val_bytes)) = item {
            if let Ok(row) = crate::exec::RowData::try_from_bytes(&val_bytes) {
                if row._deleted {
                    continue;
                }

                let key_str = String::from_utf8_lossy(&key_bytes).to_string();
                let record_id = if row._id.is_empty() { key_str } else { row._id };

                let mut map = serde_json::Map::new();
                map.insert("id".to_string(), Value::String(record_id));

                for (k, v) in row.columns {
                    if let Ok(parsed) = serde_json::from_str::<Value>(&v) {
                        map.insert(k, parsed);
                    } else {
                        map.insert(k, Value::String(v));
                    }
                }

                // Apply free-text search if requested
                if let Some(ref term) = search_term {
                    let mut found = false;
                    for (_, val) in &map {
                        let text = match val {
                            Value::String(s) => s.to_lowercase(),
                            Value::Number(n) => n.to_string(),
                            Value::Bool(b) => b.to_string(),
                            other => other.to_string().to_lowercase(),
                        };
                        if text.contains(term) {
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        continue;
                    }
                }

                // Apply rich query filters (e.g. status=eq.active, age=gt.21)
                let mut matches = true;
                for (fk, raw_val) in &query.filters {
                    if fk == "limit"
                        || fk == "offset"
                        || fk == "page"
                        || fk == "select"
                        || fk == "order"
                        || fk == "sort"
                        || fk == "q"
                        || fk == "search"
                        || fk == "database"
                    {
                        continue;
                    }

                    let op = FilterOp::parse(raw_val);
                    if !op.matches(map.get(fk)) {
                        matches = false;
                        break;
                    }
                }

                if !matches {
                    continue;
                }

                all_records.push(Value::Object(map));
            }
        }
    }

    // Apply sorting/ordering
    let order_clause = query.order.as_ref().or(query.sort.as_ref());
    if let Some(clause) = order_clause {
        let (field, is_desc) = if let Some(stripped) = clause.strip_suffix(".desc") {
            (stripped, true)
        } else if let Some(stripped) = clause.strip_suffix(":desc") {
            (stripped, true)
        } else if let Some(stripped) = clause.strip_suffix(".asc") {
            (stripped, false)
        } else if let Some(stripped) = clause.strip_suffix(":asc") {
            (stripped, false)
        } else {
            (clause.as_str(), false)
        };

        all_records.sort_by(|a, b| {
            let v_a = a.get(field);
            let v_b = b.get(field);
            let ord = match (v_a, v_b) {
                (Some(Value::Number(n1)), Some(Value::Number(n2))) => {
                    let f1 = n1.as_f64().unwrap_or(0.0);
                    let f2 = n2.as_f64().unwrap_or(0.0);
                    f1.partial_cmp(&f2).unwrap_or(std::cmp::Ordering::Equal)
                }
                (Some(Value::String(s1)), Some(Value::String(s2))) => s1.cmp(s2),
                (Some(Value::Bool(b1)), Some(Value::Bool(b2))) => b1.cmp(b2),
                (Some(_), None) => std::cmp::Ordering::Greater,
                (None, Some(_)) => std::cmp::Ordering::Less,
                _ => std::cmp::Ordering::Equal,
            };

            if is_desc {
                ord.reverse()
            } else {
                ord
            }
        });
    }

    // Apply field projection (select)
    if let Some(ref selected) = select_set {
        all_records = all_records
            .into_iter()
            .map(|mut rec| {
                if let Value::Object(ref mut map) = rec {
                    map.retain(|k, _| k == "id" || selected.contains(k));
                }
                rec
            })
            .collect();
    }

    // Compute bounded pagination
    let total = all_records.len();
    let limit = query.limit.unwrap_or(50).clamp(1, 1000);
    let offset = if let Some(page) = query.page {
        if page > 0 {
            (page - 1) * limit
        } else {
            0
        }
    } else {
        query.offset.unwrap_or(0)
    };

    let paged_records = if offset < total {
        all_records
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let count = paged_records.len();
    let page = if limit > 0 { (offset / limit) + 1 } else { 1 };
    let has_more = offset + count < total;

    let response = RestListResponse {
        status: "success".to_string(),
        table,
        count,
        data: paged_records,
        pagination: PaginationMeta {
            total,
            page,
            limit,
            offset,
            has_more,
        },
    };

    Ok((StatusCode::OK, Json(response)))
}

/// POST /v1/data/:table - PRD Section 6.4 Insert single record or bulk batch records
pub async fn insert_record(
    Path(table): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Json(payload): Json<Value>,
) -> ApiResult<impl IntoResponse> {
    let db = resolve_tenant_db(&headers, params.get("database").map(|s| s.as_str()));
    let full_table_name = format!("{}_{}", db, table);

    // Idempotency check if header provided
    let idempotency_key = headers
        .get("x-idempotency-key")
        .or_else(|| headers.get("idempotency-key"))
        .and_then(|h| h.to_str().ok());

    if let Some(key) = idempotency_key {
        if let Ok(Some(cached_bytes)) = state.storage.get::<String>("_idempotency", key) {
            if let Ok(cached_json) = serde_json::from_str::<Value>(&cached_bytes) {
                return Ok((StatusCode::OK, Json(cached_json)));
            }
        }
    }

    let now_ts = chrono::Utc::now().timestamp_millis() as u64;

    // Handle batch insert if payload is an array
    if let Some(arr) = payload.as_array() {
        let mut inserted_ids = Vec::new();
        let mut inserted_records = Vec::new();

        for item in arr {
            let mut record_obj = match item.as_object() {
                Some(obj) => obj.clone(),
                None => return Err(ApiError::bad_request("Batch items must be JSON objects")),
            };

            let id = record_obj
                .get("id")
                .and_then(|v| v.as_str())
                .or_else(|| record_obj.get("_id").and_then(|v| v.as_str()))
                .map(|s| s.to_string())
                .unwrap_or_else(|| Uuid::new_v4().to_string());

            record_obj.insert("id".to_string(), Value::String(id.clone()));

            let mut columns = HashMap::new();
            for (k, v) in &record_obj {
                columns.insert(
                    k.clone(),
                    if let Some(s) = v.as_str() {
                        s.to_string()
                    } else {
                        v.to_string()
                    },
                );
            }

            let row = crate::exec::RowData {
                _id: id.clone(),
                _version: 1,
                _created_at: now_ts,
                _updated_at: now_ts,
                _deleted: false,
                _owner: None,
                _tenant: Some(db.clone()),
                _checksum: None,
                _page_id: None,
                _transaction_id: None,
                columns,
            };

            if let Err(e) = state.storage.set(&full_table_name, &id, &row) {
                return Err(ApiError::internal(format!("Failed to store row: {}", e)));
            }

            // Emit CDC event for each inserted row
            let final_val = Value::Object(record_obj);
            let _ = state.executor.cdc_streamer.emit(
                &db,
                &table,
                "INSERT",
                &id,
                None,
                Some(final_val.clone()),
            );

            inserted_ids.push(id);
            inserted_records.push(final_val);
        }

        // Register table in _tables
        let _ = state.storage.set("_tables", &full_table_name, &true);

        let resp_json = serde_json::json!({
            "status": "success",
            "table": table,
            "count": inserted_records.len(),
            "ids": inserted_ids,
            "records": inserted_records
        });

        if let Some(key) = idempotency_key {
            let _ = state.storage.set("_idempotency", key, &resp_json.to_string());
        }

        return Ok((StatusCode::CREATED, Json(resp_json)));
    }

    // Handle single object insert
    let mut payload_obj = match payload.as_object() {
        Some(obj) => obj.clone(),
        None => {
            return Err(ApiError::bad_request(
                "Request body must be a JSON object or array of objects",
            ))
        }
    };

    let id = payload_obj
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| payload_obj.get("_id").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    payload_obj.insert("id".to_string(), Value::String(id.clone()));

    let mut columns = HashMap::new();
    for (k, v) in &payload_obj {
        columns.insert(
            k.clone(),
            if let Some(s) = v.as_str() {
                s.to_string()
            } else {
                v.to_string()
            },
        );
    }

    let row = crate::exec::RowData {
        _id: id.clone(),
        _version: 1,
        _created_at: now_ts,
        _updated_at: now_ts,
        _deleted: false,
        _owner: None,
        _tenant: Some(db.clone()),
        _checksum: None,
        _page_id: None,
        _transaction_id: None,
        columns,
    };

    if let Err(e) = state.storage.set(&full_table_name, &id, &row) {
        return Err(ApiError::internal(format!("Failed to store row: {}", e)));
    }

    // Register table in _tables
    let _ = state.storage.set("_tables", &full_table_name, &true);

    let final_val = Value::Object(payload_obj);

    // Emit CDC event -> automatically caught by RealtimeHub and pushed to subscribers
    let _ = state.executor.cdc_streamer.emit(
        &db,
        &table,
        "INSERT",
        &id,
        None,
        Some(final_val.clone()),
    );

    let resp_json = serde_json::json!({
        "status": "success",
        "id": id,
        "table": table,
        "record": final_val
    });

    if let Some(key) = idempotency_key {
        let _ = state.storage.set("_idempotency", key, &resp_json.to_string());
    }

    Ok((StatusCode::CREATED, Json(resp_json)))
}

/// GET /v1/data/:table/:id - PRD Section 6.4 Retrieve a single record by primary key
pub async fn get_record(
    Path((table, id)): Path<(String, String)>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<impl IntoResponse> {
    let db = resolve_tenant_db(&headers, params.get("database").map(|s| s.as_str()));
    let full_table_name = format!("{}_{}", db, table);

    let row = match state.storage.get::<crate::exec::RowData>(&full_table_name, &id) {
        Ok(Some(r)) if !r._deleted => r,
        Ok(_) => {
            return Err(ApiError::not_found(format!(
                "Record '{}' not found in table '{}'",
                id, table
            )))
        }
        Err(e) => return Err(ApiError::internal(format!("Storage error: {}", e))),
    };

    let mut map = serde_json::Map::new();
    map.insert("id".to_string(), Value::String(if row._id.is_empty() { id } else { row._id }));

    for (k, v) in row.columns {
        if let Ok(parsed) = serde_json::from_str::<Value>(&v) {
            map.insert(k, parsed);
        } else {
            map.insert(k, Value::String(v));
        }
    }

    // Apply optional select projection
    if let Some(select_str) = params.get("select") {
        let selected: HashSet<&str> = select_str.split(',').map(|s| s.trim()).collect();
        map.retain(|k, _| k == "id" || selected.contains(k.as_str()));
    }

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "success",
            "table": table,
            "data": map
        })),
    ))
}

/// PATCH / PUT /v1/data/:table/:id - PRD Section 6.4 Update record fields
pub async fn update_record(
    Path((table, id)): Path<(String, String)>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    Json(payload): Json<Value>,
) -> ApiResult<impl IntoResponse> {
    let db = resolve_tenant_db(&headers, params.get("database").map(|s| s.as_str()));
    let full_table_name = format!("{}_{}", db, table);

    let mut row = match state.storage.get::<crate::exec::RowData>(&full_table_name, &id) {
        Ok(Some(r)) if !r._deleted => r,
        Ok(_) => {
            return Err(ApiError::not_found(format!(
                "Record '{}' not found in table '{}'",
                id, table
            )))
        }
        Err(e) => return Err(ApiError::internal(format!("Storage error: {}", e))),
    };

    let before_json = serde_json::to_value(&row.columns).unwrap_or_default();

    if let Some(obj) = payload.as_object() {
        for (k, v) in obj {
            if k == "id" || k == "_id" {
                continue;
            }
            row.columns.insert(
                k.clone(),
                if let Some(s) = v.as_str() {
                    s.to_string()
                } else {
                    v.to_string()
                },
            );
        }
    }

    let now_ts = chrono::Utc::now().timestamp_millis() as u64;
    row._updated_at = now_ts;
    row._version += 1;

    let after_json = serde_json::to_value(&row.columns).unwrap_or_default();

    if let Err(e) = state.storage.set(&full_table_name, &id, &row) {
        return Err(ApiError::internal(format!("Failed to update row: {}", e)));
    }

    // Emit CDC update event
    let _ = state.executor.cdc_streamer.emit(
        &db,
        &table,
        "UPDATE",
        &id,
        Some(before_json),
        Some(after_json.clone()),
    );

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "success",
            "id": id,
            "table": table,
            "record": after_json
        })),
    ))
}

/// DELETE /v1/data/:table/:id - PRD Section 6.4 Delete a record by ID
pub async fn delete_record(
    Path((table, id)): Path<(String, String)>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<impl IntoResponse> {
    let db = resolve_tenant_db(&headers, params.get("database").map(|s| s.as_str()));
    let full_table_name = format!("{}_{}", db, table);

    let row = match state.storage.get::<crate::exec::RowData>(&full_table_name, &id) {
        Ok(Some(r)) if !r._deleted => r,
        Ok(_) => {
            return Err(ApiError::not_found(format!(
                "Record '{}' not found in table '{}'",
                id, table
            )))
        }
        Err(e) => return Err(ApiError::internal(format!("Storage error: {}", e))),
    };

    let old_json = serde_json::to_value(&row.columns).unwrap_or_default();
    let _ = state.storage.delete(&full_table_name, &id);

    // Emit CDC delete event
    let _ = state.executor.cdc_streamer.emit(
        &db,
        &table,
        "DELETE",
        &id,
        Some(old_json),
        None,
    );

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "success",
            "table": table,
            "deleted_id": id
        })),
    ))
}
