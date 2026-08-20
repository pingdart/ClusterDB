use crate::exec::Executor;
use crate::storage::StorageEngine;
use crate::auth::User;
use axum::{
    routing::{get, post},
    Router,
    Json,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{info, error, debug};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct QueryRequest {
    pub query: String,
    pub token: String,
    pub database: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct DocumentFilter {
    pub field: String,
    pub operator: String,
    pub value: String,
}

#[derive(Deserialize, Debug)]
pub struct GeoNear {
    pub lat: f64,
    pub lon: f64,
    pub radius_km: f64,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct DocumentQueryRequest {
    pub collection: String,
    pub r#where: Option<Vec<DocumentFilter>>,
    pub near: Option<GeoNear>,
    #[serde(rename = "orderBy")]
    pub order_by: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub explain: Option<bool>,
    pub token: String,
    pub database: Option<String>,
}

#[derive(Serialize)]
pub struct QueryResponse {
    pub status: String,
    pub result: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub status: String,
    pub token: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct ExportRequest {
    pub token: String,
    pub database: String,
}

#[derive(Deserialize)]
pub struct ImportRequest {
    pub token: String,
    pub database: String,
    pub data: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
pub struct IndexCreateRequest {
    pub token: String,
    pub database: String,
    pub table: String,
    pub fields: Vec<String>,
}

#[derive(Deserialize)]
pub struct IndexListRequest {
    pub token: String,
    pub database: String,
}

#[derive(Clone)]
pub struct AppState {
    executor: Arc<Executor>,
    storage: StorageEngine,
}

async fn handle_status() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({
        "status": "online",
        "engine": "PingDart Core Engine",
        "version": "0.1.0",
        "message": "PingDart Core Database Engine is running successfully"
    })))
}

pub async fn start_server(storage: StorageEngine, port: u16) -> anyhow::Result<()> {
    let executor = Arc::new(Executor::new(storage.clone()));
    let state = AppState { executor, storage };

    let app = Router::new()
        .route("/", get(handle_status))
        .route("/login", post(handle_login))
        .route("/query", post(handle_query))
        .route("/document/query", post(handle_document_query))
        .route("/database/export", post(handle_export))
        .route("/database/import", post(handle_import))
        .route("/index/create", post(handle_index_create))
        .route("/index/drop", post(handle_index_drop))
        .route("/index/list", post(handle_index_list))
        .route("/index/stats", post(handle_index_stats))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    info!("Starting PingDart HTTP API on {}", addr);
    
    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await?;
    
    Ok(())
}

fn authenticate_request(
    state: &AppState,
    token: &str,
    client_ip: &str,
    target_db: Option<&str>,
    requires_write: bool,
) -> Result<User, String> {
    let username = match state.storage.get::<String>("_tokens", token) {
        Ok(Some(name)) => name,
        _ => return Err("Invalid or expired token".to_string()),
    };

    let user = match state.storage.get::<User>("_users", &username) {
        Ok(Some(u)) => u,
        _ => return Err("User not found".to_string()),
    };

    if user.token.as_deref() != Some(token) {
        return Err("Token mismatch".to_string());
    }

    if !user.verify_ip(client_ip) {
        return Err("IP access denied".to_string());
    }

    if let Some(db) = target_db {
        if !user.has_access_to_db(db, requires_write) {
            return Err("Insufficient permissions for this database".to_string());
        }
    }

    Ok(user)
}

async fn handle_login(
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    info!("Login attempt for username: {} from IP: {}", payload.username, client_ip);
    
    match state.storage.get::<User>("_users", &payload.username) {
        Ok(Some(mut user)) => {
            if !user.verify_ip(&client_ip) {
                error!("Login failed for {}: IP access denied from {}", payload.username, client_ip);
                return (StatusCode::UNAUTHORIZED, Json(LoginResponse {
                    status: "error".to_string(),
                    token: None,
                    error: Some(format!("Access denied from IP: {}", client_ip)),
                }));
            }
            if user.verify_password(&payload.password) {
                let token = if let Some(existing_token) = &user.token {
                    existing_token.clone()
                } else {
                    let new_token = Uuid::new_v4().to_string();
                    user.token = Some(new_token.clone());
                    let _ = state.storage.set("_users", &user.username, &user);
                    let _ = state.storage.set("_tokens", &new_token, &user.username);
                    new_token
                };
                
                info!("Login successful for user: {}", payload.username);
                (StatusCode::OK, Json(LoginResponse {
                    status: "success".to_string(),
                    token: Some(token),
                    error: None,
                }))
            } else {
                error!("Login failed for {}: Invalid password", payload.username);
                (StatusCode::UNAUTHORIZED, Json(LoginResponse {
                    status: "error".to_string(),
                    token: None,
                    error: Some("Invalid credentials".to_string()),
                }))
            }
        },
        _ => {
            error!("Login failed for {}: User not found", payload.username);
            (StatusCode::UNAUTHORIZED, Json(LoginResponse {
                status: "error".to_string(),
                token: None,
                error: Some("Invalid credentials".to_string()),
            }))
        }
    }
}

async fn handle_query(
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    State(state): State<AppState>,
    Json(payload): Json<QueryRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    info!("SQL Query request from IP: {} on DB: {:?}", client_ip, payload.database);
    debug!("Query string: {}", payload.query);
    
    let requires_write = payload.query.to_lowercase().starts_with("create") 
                      || payload.query.to_lowercase().starts_with("insert")
                      || payload.query.to_lowercase().starts_with("update")
                      || payload.query.to_lowercase().starts_with("delete")
                      || payload.query.to_lowercase().starts_with("drop");

    if let Err(e) = authenticate_request(&state, &payload.token, &client_ip, payload.database.as_deref(), requires_write) {
        error!("SQL Query auth failed: {}", e);
        return (
            StatusCode::UNAUTHORIZED,
            Json(QueryResponse {
                status: "error".to_string(),
                result: None,
                error: Some(e),
            }),
        );
    }

    match state.executor.execute(&payload.query, payload.database.as_deref()) {
        Ok(result) => {
            info!("SQL Query successful");
            (
                StatusCode::OK,
                Json(QueryResponse {
                    status: "success".to_string(),
                    result: Some(result),
                    error: None,
                }),
            )
        },
        Err(e) => {
            error!("SQL Query failed: {}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(QueryResponse {
                    status: "error".to_string(),
                    result: None,
                    error: Some(e.to_string()),
                }),
            )
        }
    }
}

async fn handle_document_query(
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    State(state): State<AppState>,
    Json(payload): Json<DocumentQueryRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    info!("Document Query request from IP: {} on DB: {:?}", client_ip, payload.database);
    
    if let Err(e) = authenticate_request(&state, &payload.token, &client_ip, payload.database.as_deref(), false) {
        error!("Document Query auth failed: {}", e);
        return (
            StatusCode::UNAUTHORIZED,
            Json(QueryResponse {
                status: "error".to_string(),
                result: None,
                error: Some(e),
            }),
        );
    }

    match state.executor.execute_document_query(&payload, payload.database.as_deref()) {
        Ok(result) => {
            info!("Document Query successful");
            (
                StatusCode::OK,
                Json(QueryResponse {
                    status: "success".to_string(),
                    result: Some(result),
                    error: None,
                }),
            )
        },
        Err(e) => {
            error!("Document Query failed: {}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(QueryResponse {
                    status: "error".to_string(),
                    result: None,
                    error: Some(e.to_string()),
                }),
            )
        }
    }
}

async fn handle_export(
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    State(state): State<AppState>,
    Json(payload): Json<ExportRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    info!("Database Export request from IP: {} for DB: {}", client_ip, payload.database);
    
    if let Err(e) = authenticate_request(&state, &payload.token, &client_ip, Some(&payload.database), false) {
        error!("Export auth failed: {}", e);
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "status": "error",
                "error": e
            })),
        );
    }
    
    let db_prefix = format!("{}_", payload.database);
    let mut export_data = serde_json::Map::new();
    
    let trees = state.storage.tree_names();
    for tree in trees {
        if tree.starts_with(&db_prefix) {
            let collection_name = tree.strip_prefix(&db_prefix).unwrap().to_string();
            if let Ok(rows) = state.storage.get_all::<crate::exec::RowData>(&tree) {
                let mut row_values = Vec::new();
                for row in rows {
                    if let Ok(val) = serde_json::to_value(row) {
                        row_values.push(val);
                    }
                }
                export_data.insert(collection_name, serde_json::Value::Array(row_values));
            }
        }
    }
    
    info!("Database Export successful for DB: {}", payload.database);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "success",
            "data": export_data
        }))
    )
}

async fn handle_import(
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    State(state): State<AppState>,
    Json(payload): Json<ImportRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    info!("Database Import request from IP: {} for DB: {}", client_ip, payload.database);
    
    if let Err(e) = authenticate_request(&state, &payload.token, &client_ip, Some(&payload.database), true) {
        error!("Import auth failed: {}", e);
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "status": "error",
                "error": e
            })),
        );
    }
    
    let db_prefix = format!("{}_", payload.database);
    let mut imported_collections = 0;
    let mut imported_rows = 0;
    
    for (collection, rows_val) in payload.data {
        if let Some(rows) = rows_val.as_array() {
            let full_table_name = format!("{}{}", db_prefix, collection);
            for row_val in rows {
                if let Ok(row_data) = serde_json::from_value::<crate::exec::RowData>(row_val.clone()) {
                    let _ = state.storage.set(&full_table_name, &row_data._id, &row_data);
                    imported_rows += 1;
                }
            }
            imported_collections += 1;
        }
    }
    let _ = state.storage.flush();
    info!("Database Import successful for DB: {}. Collections: {}, Rows: {}", payload.database, imported_collections, imported_rows);
    
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "success",
            "imported_collections": imported_collections,
            "imported_rows": imported_rows
        }))
    )
}

pub async fn handle_index_create(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<IndexCreateRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, Some(&req.database), true) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }
    let sql = format!("CREATE INDEX idx_name ON {}({})", req.table, req.fields.join(", "));
    match state.executor.execute(&sql, Some(&req.database)) {
        Ok(res) => (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(res), error: None })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

pub async fn handle_index_drop(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<IndexCreateRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, Some(&req.database), true) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }
    let sql = format!("DROP INDEX ON {} {}", req.table, req.fields.join(", "));
    match state.executor.execute(&sql, Some(&req.database)) {
        Ok(res) => (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(res), error: None })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

pub async fn handle_index_list(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<IndexListRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, Some(&req.database), false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }
    match state.executor.execute("SHOW INDEXES", Some(&req.database)) {
        Ok(res) => (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(res), error: None })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

pub async fn handle_index_stats(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<IndexListRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, Some(&req.database), false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }
    match state.executor.execute("SHOW INDEX STATS", Some(&req.database)) {
        Ok(res) => (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(res), error: None })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}
