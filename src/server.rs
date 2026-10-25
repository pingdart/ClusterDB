use crate::exec::Executor;
use crate::storage::StorageEngine;
use crate::auth::User;
use axum::{
    routing::{get, post},
    Router,
    Json,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{info, error, debug};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct QueryRequest {
    pub query: String,
    pub token: String,
    pub database: Option<String>,
    pub tenant_id: Option<String>,
    pub traceparent: Option<String>,
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
pub struct DocumentQueryRequest {
    pub collection: String,
    pub r#where: Option<Vec<DocumentFilter>>,
    pub near: Option<GeoNear>,
    pub order_by: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub explain: Option<bool>,
    pub token: String,
    pub database: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
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

#[derive(Deserialize)]
pub struct VectorSearchRequest {
    pub token: String,
    pub database: String,
    pub table: String,
    pub column: String,
    pub vector: Vec<f32>,
    pub top_k: Option<usize>,
}

#[derive(Deserialize)]
pub struct TextSearchRequest {
    pub token: String,
    pub database: String,
    pub table: String,
    pub column: String,
    pub query: String,
    pub top_k: Option<usize>,
}

#[derive(Deserialize)]
pub struct HybridSearchRequest {
    pub token: String,
    pub database: String,
    pub table: String,
    pub vector_column: String,
    pub text_column: String,
    pub vector: Vec<f32>,
    pub query: String,
    pub alpha: Option<f32>,
    pub top_k: Option<usize>,
}

#[derive(Deserialize)]
pub struct MemoryStoreRequest {
    pub token: String,
    pub session_id: Option<String>,
    pub memory_type: String, // "Episodic", "Semantic", "Working", "LongTerm"
    pub content: String,
    pub importance: Option<f32>,
    pub embedding: Option<Vec<f32>>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct MemoryRecallRequest {
    pub token: String,
    pub session_id: Option<String>,
    pub memory_type: Option<String>,
    pub query_embedding: Option<Vec<f32>>,
    pub top_k: Option<usize>,
}

#[derive(Deserialize)]
pub struct MemoryConsolidateRequest {
    pub token: String,
    pub session_id: Option<String>,
}

#[derive(Deserialize)]
pub struct GraphNodeRequest {
    pub token: String,
    pub id: String,
    pub labels: Vec<String>,
    pub properties: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct GraphEdgeRequest {
    pub token: String,
    pub id: String,
    pub source: String,
    pub target: String,
    pub r#type: String,
    pub properties: Option<serde_json::Value>,
    pub weight: Option<f32>,
}

#[derive(Deserialize)]
pub struct GraphPathRequest {
    pub token: String,
    pub start: String,
    pub end: String,
    pub max_hops: Option<usize>,
}

#[derive(Deserialize)]
pub struct GraphNeighborsRequest {
    pub token: String,
    pub node_id: String,
    pub edge_type: Option<String>,
    pub target_label: Option<String>,
}

#[derive(Deserialize)]
pub struct CDCReplayRequest {
    pub token: String,
    pub since_sequence_id: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Clone)]
pub struct AppState {
    pub executor: Arc<Executor>,
    pub storage: StorageEngine,
    pub cluster: Arc<crate::cluster::ClusterEngine>,
    pub backup_manager: Arc<crate::backup::BackupManager>,
    pub metrics: Arc<crate::observability::MetricsCollector>,
    pub audit_logger: Arc<crate::observability::AuditLogger>,
    pub role_manager: Arc<crate::security::RoleManager>,
    pub api_key_manager: Arc<crate::security::ApiKeyManager>,
    pub jwt_validator: Arc<crate::security::JwtValidator>,
    pub crypto_engine: Arc<crate::security::CryptoEngine>,
    pub tracer: Arc<crate::observability::DistributedTracer>,
    pub slow_query_logger: Arc<crate::observability::SlowQueryLogger>,
    pub migration_manager: Arc<crate::migration::MigrationManager>,
}

const DASHBOARD_HTML: &str = include_str!("ui/dashboard.html");

async fn handle_database_ui() -> impl IntoResponse {
    Html(DASHBOARD_HTML)
}

async fn handle_status(headers: HeaderMap) -> impl IntoResponse {
    if let Some(accept) = headers.get(header::ACCEPT) {
        if let Ok(accept_str) = accept.to_str() {
            if accept_str.contains("text/html") {
                return Html(DASHBOARD_HTML).into_response();
            }
        }
    }
    (StatusCode::OK, Json(serde_json::json!({
        "status": "online",
        "engine": "PingDart AI-Native Core Engine",
        "version": "0.5.0",
        "ui_url": "/database",
        "capabilities": [
            "SQL Engine",
            "Document Store",
            "HNSW Vector Engine",
            "BM25 Full-Text Search",
            "Hybrid Search",
            "AI Cognitive Memory & Consolidation",
            "Neo4j Labeled Graph Engine",
            "Durable CDC Commit Log",
            "Raft Replication & Consensus",
            "Cluster Metadata Service",
            "Consistent Hash Sharding",
            "Distributed Scatter-Gather Vector Search",
            "Multi-Tenant RLS & Quota Governance",
            "Columnar Analytics Engine (OLAP)",
            "Continuous CDC Backup & PITR",
            "Prometheus Metrics & Observability",
            "Granular RBAC & Service Accounts",
            "Scoped API Key Management",
            "OIDC / JWT Bearer Authentication",
            "AES-256-GCM Envelope Encryption",
            "OpenTelemetry Tracing & Query Profiler (EXPLAIN ANALYZE)",
            "Cost-Based Query Optimizer (CBO) & Persistent Stats",
            "Enterprise Native Backup Engine (.pb Protobuf, ZSTD, Encryption)",
            "Universal Migration Framework (Postgres, MySQL, Mongo, CSV, Schema Drift, AI Vectors)"
        ],
        "message": "PingDart AI-Native Distributed Engine is operational"
    }))).into_response()
}

pub async fn start_server(
    storage: StorageEngine,
    port: u16,
    node_id: Option<String>,
    join_seed: Option<String>,
) -> anyhow::Result<()> {
    let nid = node_id.unwrap_or_else(|| "node-1".to_string());
    let my_addr = format!("127.0.0.1:{}", port);
    let executor = Arc::new(Executor::new(storage.clone()));
    let cluster = Arc::new(crate::cluster::ClusterEngine::new(
        nid.clone(),
        my_addr.clone(),
        "pingdart-cluster-alpha".to_string(),
        storage.clone(),
        executor.clone(),
    ));

    // Register this node in cluster metadata
    cluster.metadata.register_node(crate::cluster::node::NodeInfo::new(nid.clone(), my_addr.clone())).await;

    // Start background Raft & cluster tasks
    cluster.start_background_tasks();

    // Start PostgreSQL Wire Protocol (v3) Server
    let pg_port = std::env::var("PINGDART_PG_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(5432);
    let pg_addr = format!("0.0.0.0:{}", pg_port);
    let pg_server = crate::pgwire::PgWireServer::new(executor.clone(), pg_addr.clone());
    if let Err(e) = pg_server.run().await {
        tracing::warn!("Could not start PostgreSQL Wire Protocol on {}: {}. Trying fallback port {}.", pg_addr, e, port + 100);
        let fallback_addr = format!("0.0.0.0:{}", port + 100);
        let fallback_pg = crate::pgwire::PgWireServer::new(executor.clone(), fallback_addr);
        let _ = fallback_pg.run().await;
    }

    // If join seed provided, trigger async cluster join
    if let Some(seed) = join_seed {
        let cluster_clone = cluster.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
            if let Err(e) = cluster_clone.join_cluster(&seed).await {
                tracing::warn!("Could not join seed {}: {}", seed, e);
            }
        });
    }

    let cdc_log = executor.cdc_streamer.log().clone();
    let backup_dir = std::env::var("PINGDART_BACKUP_DIR")
        .unwrap_or_else(|_| format!("backups/pingdart_backups_{}", port));
    let backup_manager = Arc::new(crate::backup::BackupManager::new(storage.clone(), Arc::new(cdc_log), &backup_dir)?);
    let metrics = Arc::new(crate::observability::MetricsCollector::new());
    let audit_logger = Arc::new(crate::observability::AuditLogger::new(1000));
    let role_manager = Arc::new(crate::security::RoleManager::new(storage.clone()));
    let api_key_manager = Arc::new(crate::security::ApiKeyManager::new(storage.clone()));
    let jwt_secret = std::env::var("PINGDART_JWT_SECRET").unwrap_or_else(|_| "pingdart_enterprise_jwt_default_secret_2026".to_string());
    let jwt_issuer = std::env::var("PINGDART_JWT_ISSUER").ok();
    let jwt_validator = Arc::new(crate::security::JwtValidator::new(&jwt_secret, jwt_issuer));
    let master_key = std::env::var("PINGDART_MASTER_KEY").unwrap_or_else(|_| "pingdart_master_envelope_key_secret_2026".to_string());
    let crypto_engine = Arc::new(crate::security::CryptoEngine::new(&master_key));
    let tracer = Arc::new(crate::observability::DistributedTracer::new(5000));
    let slow_query_logger = Arc::new(crate::observability::SlowQueryLogger::new(50.0, 1000));
    let migration_manager = executor.migration_manager.clone();

    let state = AppState {
        executor,
        storage,
        cluster,
        backup_manager,
        metrics,
        audit_logger,
        role_manager,
        api_key_manager,
        jwt_validator,
        crypto_engine,
        tracer,
        slow_query_logger,
        migration_manager,
    };

    let app = Router::new()
        .route("/", get(handle_status))
        .route("/index.html", get(handle_database_ui))
        .route("/database", get(handle_database_ui))
        .route("/database/", get(handle_database_ui))
        .route("/console", get(handle_database_ui))
        .route("/admin", get(handle_database_ui))
        .route("/login", post(handle_login))
        .route("/query", post(handle_query))
        .route("/document/query", post(handle_document_query))
        .route("/database/export", post(handle_export))
        .route("/database/import", post(handle_import))
        .route("/export", post(handle_export))
        .route("/import", post(handle_import))
        .route("/index/create", post(handle_index_create))
        .route("/index/drop", post(handle_index_drop))
        .route("/index/list", post(handle_index_list))
        .route("/index/stats", post(handle_index_stats))
        .route("/vector/search", post(handle_vector_search))
        .route("/search/text", post(handle_text_search))
        .route("/search/hybrid", post(handle_hybrid_search))
        .route("/ai/memory/store", post(handle_memory_store))
        .route("/ai/memory/recall", post(handle_memory_recall))
        .route("/ai/memory/consolidate", post(handle_memory_consolidate))
        .route("/graph/node", post(handle_graph_node))
        .route("/graph/edge", post(handle_graph_edge))
        .route("/graph/path", post(handle_graph_path))
        .route("/graph/neighbors", post(handle_graph_neighbors))
        .route("/cdc/replay", post(handle_cdc_replay))
        // Distributed & Raft Cluster Endpoints
        .route("/raft/request_vote", post(handle_raft_request_vote))
        .route("/raft/append_entries", post(handle_raft_append_entries))
        .route("/cluster/status", get(handle_cluster_status))
        .route("/cluster/join", post(handle_cluster_join))
        .route("/cluster/shard/vector_search", post(handle_shard_vector_search))
        // Multi-Tenant & Security Endpoints
        .route("/tenant/create", post(handle_tenant_create))
        .route("/tenant/list", post(handle_tenant_list))
        .route("/tenant/policy", post(handle_tenant_policy))
        .route("/tenant/rls", post(handle_tenant_rls))
        // Observability & Health Endpoints
        .route("/metrics", get(handle_metrics))
        .route("/health/live", get(handle_health_live))
        .route("/health/ready", get(handle_health_ready))
        .route("/observability/traces", get(handle_traces_list))
        .route("/observability/traces/:trace_id", get(handle_trace_get))
        .route("/observability/slow_queries", get(handle_slow_queries_list))
        // Backup & Disaster Recovery Endpoints
        .route("/backup/snapshot", post(handle_backup_snapshot))
        .route("/backup/restore", post(handle_backup_restore))
        .route("/backup/pitr", post(handle_backup_pitr))
        .route("/backup/export", post(handle_backup_export_pb))
        .route("/backup/import", post(handle_backup_import_pb))
        // Universal Migration Framework Endpoints
        .route("/migration/start", post(handle_migration_start))
        .route("/migration/resume/:job_id", post(handle_migration_resume))
        .route("/migration/abort/:job_id", post(handle_migration_abort))
        .route("/migration/status/:job_id", get(handle_migration_status))
        .route("/migration/jobs", get(handle_migration_jobs))
        .route("/migration/validate/:job_id", get(handle_migration_validate))
        // Enterprise Security & Access Control Endpoints
        .route("/security/apikey/create", post(handle_apikey_create))
        .route("/security/apikey/revoke", post(handle_apikey_revoke))
        .route("/security/apikey/list", post(handle_apikey_list))
        .route("/security/role/create", post(handle_role_create))
        .route("/security/role/list", get(handle_role_list))
        .route("/security/encrypt", post(handle_security_encrypt))
        .route("/security/decrypt", post(handle_security_decrypt))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    info!("Starting PingDart HTTP API on {}", addr);
    
    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => {
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received SIGINT (Ctrl+C), initiating graceful database shutdown...");
        },
        _ = terminate => {
            info!("Received SIGTERM from systemd, initiating graceful database shutdown...");
        },
    }
}

fn authenticate_request(
    state: &AppState,
    token: &str,
    client_ip: &str,
    target_db: Option<&str>,
    requires_write: bool,
) -> Result<User, String> {
    if token == "cluster_internal" {
        return Ok(User::new_root("RavindraChinnu12#".to_string()));
    }

    let user = if token.starts_with("pd_live_") {
        match state.api_key_manager.validate_key(token) {
            Ok(key) => {
                let mut u = User::new_service_account(format!("svc_{}", key.name), key.roles, key.tenant_id);
                u.token = Some(token.to_string());
                u
            }
            Err(e) => return Err(format!("API Key authentication failed: {}", e)),
        }
    } else if token.contains('.') && token.len() > 20 {
        match state.jwt_validator.validate(token) {
            Ok(claims) => {
                let mut u = User::new_user(claims.sub, "".to_string(), "%".to_string(), vec![]);
                u.rbac_roles = claims.roles;
                if let Some(t) = claims.tenant_id {
                    u.assigned_tenants = vec![t];
                }
                u.token = Some(token.to_string());
                u
            }
            Err(e) => return Err(format!("JWT validation failed: {}", e)),
        }
    } else {
        let username = match state.storage.get::<String>("_tokens", token) {
            Ok(Some(name)) => name,
            _ => return Err("Invalid or expired token".to_string()),
        };

        let u = match state.storage.get::<User>("_users", &username) {
            Ok(Some(u)) => u,
            _ => return Err("User not found".to_string()),
        };

        if u.token.as_deref() != Some(token) {
            return Err("Token mismatch".to_string());
        }

        if !u.verify_ip(client_ip) {
            return Err("IP access denied".to_string());
        }

        u
    };

    if let Some(db) = target_db {
        if !user.has_access_to_db(db, requires_write) && !user.has_rbac_role("SuperAdmin") {
            let required_perm = if requires_write {
                crate::security::Permission::TableWrite
            } else {
                crate::security::Permission::TableRead
            };
            let tenant = user.assigned_tenants.first().map(|s| s.as_str());
            if !state.role_manager.has_permission(&user.rbac_roles, &required_perm, Some(db), tenant) {
                return Err(format!("Insufficient permissions for database '{}'", db));
            }
        }
    }

    if !user.has_rbac_role("SuperAdmin") && !user.roles.contains(&crate::auth::Role::MainAdmin) {
        if requires_write {
            let tenant = user.assigned_tenants.first().map(|s| s.as_str());
            if !state.role_manager.has_permission(&user.rbac_roles, &crate::security::Permission::TableWrite, target_db, tenant) {
                return Err("Access denied: missing TableWrite permission".to_string());
            }
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

    let trace_ctx = payload.traceparent.as_deref()
        .and_then(|tp| crate::observability::TraceContext::from_traceparent(tp).ok())
        .unwrap_or_else(crate::observability::TraceContext::new_root);
    let start_epoch_ms = crate::observability::tracer::now_epoch_ms();
    
    let requires_write = payload.query.to_lowercase().starts_with("create") 
                      || payload.query.to_lowercase().starts_with("insert")
                      || payload.query.to_lowercase().starts_with("update")
                      || payload.query.to_lowercase().starts_with("delete")
                      || payload.query.to_lowercase().starts_with("drop")
                      || payload.query.to_lowercase().starts_with("alter");

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

    let active_tenant = payload.tenant_id.as_deref().unwrap_or("default");
    if let Err(e) = state.executor.tenant_manager.check_rate_limit(active_tenant) {
        error!("Rate limit exceeded for tenant '{}': {}", active_tenant, e);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(QueryResponse {
                status: "error".to_string(),
                result: None,
                error: Some(format!("Rate limit exceeded for tenant '{}'", active_tenant)),
            }),
        );
    }

    // Follower Write-Forwarding: If this node is a Follower and has a known leader, forward write query
    if requires_write && state.cluster.raft.get_role().await == crate::cluster::node::NodeRole::Follower {
        if let Some(leader_id) = state.cluster.raft.get_leader_id().await {
            if let Some(leader_node) = state.cluster.metadata.get_node(&leader_id).await {
                if !leader_node.address.is_empty() && leader_node.id != state.cluster.raft.node_id {
                    info!("Follower forwarding write query to leader {} at {}", leader_id, leader_node.address);
                    let client = reqwest::Client::new();
                    let url = format!("http://{}/query", leader_node.address);
                    let forward_body = serde_json::json!({
                        "query": payload.query,
                        "token": "cluster_internal",
                        "database": payload.database,
                        "tenant_id": payload.tenant_id,
                        "traceparent": trace_ctx.to_traceparent()
                    });
                    if let Ok(resp) = client.post(&url).json(&forward_body).send().await {
                        if let Ok(forward_res) = resp.json::<QueryResponse>().await {
                            return (StatusCode::OK, Json(forward_res));
                        }
                    }
                }
            }
        }
    }

    let query_start = std::time::Instant::now();

    match state.executor.execute_with_tenant(&payload.query, payload.database.as_deref(), payload.tenant_id.as_deref()) {
        Ok(result) => {
            let elapsed_us = query_start.elapsed().as_micros() as u64;
            let duration_ms = elapsed_us as f64 / 1000.0;
            state.metrics.record_query(true, elapsed_us);
            state.audit_logger.log(
                &client_ip,
                "admin",
                active_tenant,
                "SQL_QUERY",
                Some(&payload.query),
                "SUCCESS",
                duration_ms,
            );
            info!("SQL Query successful");

            // Record OpenTelemetry trace span
            let mut attrs = std::collections::HashMap::new();
            attrs.insert("db.statement".to_string(), payload.query.clone());
            attrs.insert("db.tenant".to_string(), active_tenant.to_string());
            attrs.insert("db.system".to_string(), "pingdart".to_string());

            let span = crate::observability::Span {
                trace_id: trace_ctx.trace_id.clone(),
                span_id: trace_ctx.span_id.clone(),
                parent_span_id: None,
                name: "QueryExecution".to_string(),
                start_time_epoch_ms: start_epoch_ms,
                duration_ms,
                status: "OK".to_string(),
                attributes: attrs,
                events: vec![],
            };
            state.tracer.record_span(span);

            // Evaluate slow query logger
            let rows_count = result.lines().count().max(1);
            state.slow_query_logger.maybe_log_query(&payload.query, active_tenant, duration_ms, rows_count, rows_count, None);

            // If mutation succeeded, propose via Raft consensus replication
            if requires_write {
                let _ = state.cluster.raft.propose(crate::cluster::raft::ClusterCommand::ExecuteSql {
                    query: payload.query.clone(),
                    database: payload.database.clone(),
                }).await;
            }

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
            let elapsed_us = query_start.elapsed().as_micros() as u64;
            let duration_ms = elapsed_us as f64 / 1000.0;
            state.metrics.record_query(false, elapsed_us);
            state.audit_logger.log(
                &client_ip,
                "admin",
                active_tenant,
                "SQL_QUERY",
                Some(&payload.query),
                "ERROR",
                duration_ms,
            );
            error!("SQL Query failed: {}", e);

            // Record OpenTelemetry error span
            let mut attrs = std::collections::HashMap::new();
            attrs.insert("db.statement".to_string(), payload.query.clone());
            attrs.insert("error.message".to_string(), e.to_string());
            attrs.insert("db.tenant".to_string(), active_tenant.to_string());

            let span = crate::observability::Span {
                trace_id: trace_ctx.trace_id.clone(),
                span_id: trace_ctx.span_id.clone(),
                parent_span_id: None,
                name: "QueryExecution".to_string(),
                start_time_epoch_ms: start_epoch_ms,
                duration_ms,
                status: "ERROR".to_string(),
                attributes: attrs,
                events: vec![],
            };
            state.tracer.record_span(span);

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
    
    let mut table_names = std::collections::BTreeSet::new();
    let trees = state.storage.tree_names();
    for tree in trees {
        if tree.starts_with(&db_prefix) {
            let collection_name = tree.strip_prefix(&db_prefix).unwrap().to_string();
            table_names.insert(collection_name);
        }
    }
    if let Ok(keys) = state.storage.get_keys_by_prefix("_tables", &db_prefix) {
        for full_name in keys {
            if let Some(col) = full_name.strip_prefix(&db_prefix) {
                table_names.insert(col.to_string());
            }
        }
    }

    for collection_name in table_names {
        let full_table_name = format!("{}{}", db_prefix, collection_name);
        let mut row_values = Vec::new();
        if let Ok(rows) = state.storage.get_all::<crate::exec::RowData>(&full_table_name) {
            for row in rows {
                if let Ok(val) = serde_json::to_value(row) {
                    row_values.push(val);
                }
            }
        }
        export_data.insert(collection_name, serde_json::Value::Array(row_values));
    }
    
    info!("Database Export successful for DB: {}", payload.database);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "success",
            "database": payload.database,
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
    let _ = state.storage.set("_databases", &payload.database, &payload.database);
    let mut imported_collections = 0;
    let mut imported_rows = 0;
    
    for (collection, rows_val) in payload.data {
        if let Some(rows) = rows_val.as_array() {
            let full_table_name = format!("{}{}", db_prefix, collection);
            let _ = state.storage.set("_tables", &full_table_name, &collection);
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
            "database": payload.database,
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

// Vector Search Handler
pub async fn handle_vector_search(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<VectorSearchRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, Some(&req.database), false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    match state.cluster.search_coordinator.scatter_gather_vector_search(
        &req.database,
        &req.table,
        &req.column,
        &req.vector,
        req.top_k.unwrap_or(10),
    ).await {
        Ok(results) => {
            let json_res = serde_json::to_string(&results).unwrap_or("[]".to_string());
            (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(json_res), error: None }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

// Full-Text BM25 Search Handler
pub async fn handle_text_search(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<TextSearchRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, Some(&req.database), false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    let full_table_name = format!("{}_{}", req.database, req.table);
    match state.executor.search_manager.search(&full_table_name, &req.column, &req.query, req.top_k.unwrap_or(10)) {
        Ok(results) => {
            let json_res = serde_json::to_string(&results).unwrap_or("[]".to_string());
            (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(json_res), error: None }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

// Hybrid Search Handler
pub async fn handle_hybrid_search(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<HybridSearchRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, Some(&req.database), false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    let full_table_name = format!("{}_{}", req.database, req.table);
    let top_k = req.top_k.unwrap_or(10);
    let vec_hits = state.executor.vector_manager.search(&full_table_name, &req.vector_column, &req.vector, top_k * 2)
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.id, r.similarity))
        .collect::<Vec<_>>();

    let text_hits = state.executor.search_manager.search(&full_table_name, &req.text_column, &req.query, top_k * 2)
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.doc_id, r.score))
        .collect::<Vec<_>>();

    let combined = crate::search::combine_hybrid_scores(&vec_hits, &text_hits, req.alpha.unwrap_or(0.5), top_k);
    let json_res = serde_json::to_string(&combined).unwrap_or("[]".to_string());
    (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(json_res), error: None }))
}

// AI Memory Handlers
pub async fn handle_memory_store(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<MemoryStoreRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    let memory_type = match req.memory_type.to_lowercase().as_str() {
        "episodic" => crate::memory::MemoryType::Episodic,
        "semantic" => crate::memory::MemoryType::Semantic,
        "working" => crate::memory::MemoryType::Working,
        "longterm" | "long_term" => crate::memory::MemoryType::LongTerm,
        _ => crate::memory::MemoryType::Working,
    };

    let id = uuid::Uuid::new_v4().to_string();
    let item = crate::memory::MemoryItem::new(
        id.clone(),
        req.session_id,
        memory_type,
        req.content,
        req.importance.unwrap_or(0.5),
        req.embedding,
        req.metadata.unwrap_or(serde_json::json!({})),
    );

    match state.executor.memory_engine.store(item) {
        Ok(_) => {
            let res = serde_json::json!({ "status": "stored", "memory_id": id }).to_string();
            (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(res), error: None }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

pub async fn handle_memory_recall(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<MemoryRecallRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    let memory_type = req.memory_type.as_deref().map(|t| match t.to_lowercase().as_str() {
        "episodic" => crate::memory::MemoryType::Episodic,
        "semantic" => crate::memory::MemoryType::Semantic,
        "working" => crate::memory::MemoryType::Working,
        "longterm" | "long_term" => crate::memory::MemoryType::LongTerm,
        _ => crate::memory::MemoryType::Working,
    });

    match state.executor.memory_engine.recall(
        req.query_embedding.as_deref(),
        memory_type,
        req.session_id.as_deref(),
        req.top_k.unwrap_or(5),
    ) {
        Ok(memories) => {
            let json_res = serde_json::to_string(&memories).unwrap_or("[]".to_string());
            (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(json_res), error: None }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

pub async fn handle_memory_consolidate(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<MemoryConsolidateRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    match state.executor.memory_engine.consolidate_session(req.session_id) {
        Ok(report) => {
            let json_res = serde_json::to_string(&report).unwrap_or("null".to_string());
            (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(json_res), error: None }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

// Graph Handlers
pub async fn handle_graph_node(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<GraphNodeRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    let node = crate::graph::GraphNode::new(req.id, req.labels, req.properties.unwrap_or(serde_json::json!({})));
    match state.executor.graph_engine.create_node(node) {
        Ok(n) => {
            let json_res = serde_json::to_string(&n).unwrap_or("{}".to_string());
            (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(json_res), error: None }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

pub async fn handle_graph_edge(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<GraphEdgeRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    let edge = crate::graph::GraphEdge::new(req.id, req.source, req.target, req.r#type, req.properties.unwrap_or(serde_json::json!({})), req.weight);
    match state.executor.graph_engine.create_edge(edge) {
        Ok(e) => {
            let json_res = serde_json::to_string(&e).unwrap_or("{}".to_string());
            (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(json_res), error: None }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

pub async fn handle_graph_path(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<GraphPathRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    match state.executor.graph_engine.find_path(&req.start, &req.end, req.max_hops.unwrap_or(4)) {
        Ok(path) => {
            let json_res = serde_json::to_string(&path).unwrap_or("null".to_string());
            (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(json_res), error: None }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

pub async fn handle_graph_neighbors(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<GraphNeighborsRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    match state.executor.graph_engine.get_neighbors(&req.node_id, req.edge_type.as_deref(), req.target_label.as_deref()) {
        Ok(neighbors) => {
            let json_res = serde_json::to_string(&neighbors).unwrap_or("[]".to_string());
            (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(json_res), error: None }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

// CDC Replay Handler
pub async fn handle_cdc_replay(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<CDCReplayRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    match state.executor.cdc_streamer.log().replay(req.since_sequence_id.unwrap_or(0), req.limit.unwrap_or(100)) {
        Ok(records) => {
            let json_res = serde_json::to_string(&records).unwrap_or("[]".to_string());
            (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(json_res), error: None }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

// Raft Request Vote Handler
pub async fn handle_raft_request_vote(
    State(state): State<AppState>,
    Json(args): Json<crate::cluster::raft::RequestVoteArgs>,
) -> impl IntoResponse {
    let reply = state.cluster.raft.handle_request_vote(args).await;
    (StatusCode::OK, Json(reply))
}

// Raft Append Entries (Heartbeats & Replication) Handler
pub async fn handle_raft_append_entries(
    State(state): State<AppState>,
    Json(args): Json<crate::cluster::raft::AppendEntriesArgs>,
) -> impl IntoResponse {
    let reply = state.cluster.raft.handle_append_entries(args).await;
    (StatusCode::OK, Json(reply))
}

// Cluster Status & Health Handler
pub async fn handle_cluster_status(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let summary = state.cluster.get_summary().await;
    (StatusCode::OK, Json(summary))
}

// Node Join Handler
pub async fn handle_cluster_join(
    State(state): State<AppState>,
    Json(node): Json<crate::cluster::node::NodeInfo>,
) -> impl IntoResponse {
    info!("Received join request from node {} at {}", node.id, node.address);
    state.cluster.metadata.register_node(node.clone()).await;
    state.cluster.raft.add_peer(node.id.clone(), node.address.clone()).await;
    let _ = state.cluster.raft.propose(crate::cluster::raft::ClusterCommand::RegisterNode { node: node.clone() }).await;
    let all_nodes = state.cluster.metadata.get_all_nodes().await;
    let leader_id = state.cluster.raft.get_leader_id().await;
    (StatusCode::OK, Json(serde_json::json!({
        "status": "success",
        "message": format!("Node {} joined the cluster successfully", node.id),
        "leader_id": leader_id,
        "nodes": all_nodes
    })))
}

// Internal Shard Vector Search Handler
pub async fn handle_shard_vector_search(
    State(state): State<AppState>,
    Json(req): Json<crate::cluster::distributed_search::ShardVectorSearchRequest>,
) -> impl IntoResponse {
    let full_table_name = format!("{}_{}", req.database, req.table);
    let results = state
        .executor
        .vector_manager
        .search(&full_table_name, &req.column, &req.vector, req.top_k)
        .unwrap_or_default();
    (StatusCode::OK, Json(results))
}

// Tenant Management Payloads & Handlers
#[derive(Deserialize)]
pub struct TenantCreateRequest {
    pub token: String,
    pub tenant_id: String,
    pub name: Option<String>,
    pub max_storage_mb: Option<u64>,
    pub max_vectors: Option<usize>,
    pub max_qps: Option<u32>,
}

#[derive(Deserialize)]
pub struct TenantListRequest {
    pub token: String,
}

#[derive(Deserialize)]
pub struct TenantPolicyRequest {
    pub token: String,
    pub name: String,
    pub table: String,
    pub command: Option<String>,
    pub using_expr: String,
}

#[derive(Deserialize)]
pub struct TenantRlsRequest {
    pub token: String,
    pub table: String,
    pub enable: bool,
}

pub async fn handle_tenant_create(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<TenantCreateRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, true) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    let max_storage_bytes = req.max_storage_mb.unwrap_or(10_240) * 1024 * 1024;
    let max_vectors = req.max_vectors.unwrap_or(1_000_000);
    let max_qps = req.max_qps.unwrap_or(1_000);
    let name = req.name.unwrap_or_else(|| req.tenant_id.clone());

    match state.executor.tenant_manager.create_tenant(
        req.tenant_id.clone(),
        name,
        max_storage_bytes,
        max_vectors,
        max_qps,
    ) {
        Ok(_) => (StatusCode::OK, Json(QueryResponse {
            status: "success".to_string(),
            result: Some(format!("Tenant '{}' created successfully", req.tenant_id)),
            error: None,
        })),
        Err(e) => (StatusCode::BAD_REQUEST, Json(QueryResponse {
            status: "error".to_string(),
            result: None,
            error: Some(e.to_string()),
        })),
    }
}

pub async fn handle_tenant_list(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<TenantListRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    let tenants = state.executor.tenant_manager.list_tenants();
    let json_str = serde_json::to_string_pretty(&tenants).unwrap_or("[]".to_string());
    (StatusCode::OK, Json(QueryResponse {
        status: "success".to_string(),
        result: Some(json_str),
        error: None,
    }))
}

pub async fn handle_tenant_policy(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<TenantPolicyRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, true) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    let policy = crate::tenant::RlsPolicy {
        name: req.name.clone(),
        table: req.table.clone(),
        command: req.command.unwrap_or_else(|| "ALL".to_string()),
        using_expr: req.using_expr.clone(),
    };
    state.executor.tenant_manager.add_policy(policy);

    (StatusCode::OK, Json(QueryResponse {
        status: "success".to_string(),
        result: Some(format!("Policy '{}' on table '{}' created successfully", req.name, req.table)),
        error: None,
    }))
}

pub async fn handle_tenant_rls(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<TenantRlsRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, true) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    state.executor.tenant_manager.set_rls_enabled(&req.table, req.enable);
    let state_str = if req.enable { "enabled" } else { "disabled" };
    (StatusCode::OK, Json(QueryResponse {
        status: "success".to_string(),
        result: Some(format!("Row-level security {} for table '{}'", state_str, req.table)),
        error: None,
    }))
}

// Observability Handlers
pub async fn handle_metrics(State(state): State<AppState>) -> impl IntoResponse {
    let body = state.metrics.render_prometheus();
    (
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
}

pub async fn handle_health_live() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({
        "status": "UP",
        "component": "pingdart-core",
        "version": "0.4.0"
    })))
}

pub async fn handle_health_ready(State(state): State<AppState>) -> impl IntoResponse {
    let leader_id = state.cluster.raft.get_leader_id().await;
    let node_count = state.cluster.metadata.get_all_nodes().await.len();
    (StatusCode::OK, Json(serde_json::json!({
        "status": "READY",
        "leader_id": leader_id,
        "nodes": node_count,
        "storage": "online"
    })))
}

#[derive(Deserialize)]
pub struct TracesQueryParams {
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct SlowQueriesQueryParams {
    pub limit: Option<usize>,
    pub tenant: Option<String>,
}

pub async fn handle_traces_list(
    State(state): State<AppState>,
    Query(params): Query<TracesQueryParams>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50);
    let traces = state.tracer.list_recent_traces(limit);
    (StatusCode::OK, Json(traces))
}

pub async fn handle_trace_get(
    State(state): State<AppState>,
    Path(trace_id): Path<String>,
) -> impl IntoResponse {
    let spans = state.tracer.get_trace(&trace_id);
    (StatusCode::OK, Json(spans))
}

pub async fn handle_slow_queries_list(
    State(state): State<AppState>,
    Query(params): Query<SlowQueriesQueryParams>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50);
    let slow_queries = state.slow_query_logger.get_slow_queries(limit, params.tenant.as_deref());
    (StatusCode::OK, Json(slow_queries))
}

// Backup & Disaster Recovery Requests & Handlers
#[derive(Deserialize)]
pub struct BackupSnapshotRequest {
    pub token: String,
}

#[derive(Deserialize)]
pub struct BackupRestoreRequest {
    pub token: String,
    pub snapshot_id: String,
}

#[derive(Deserialize)]
pub struct BackupPitrRequest {
    pub token: String,
    pub target_timestamp: u64,
}

pub async fn handle_backup_snapshot(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<BackupSnapshotRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, true) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    match state.backup_manager.create_snapshot() {
        Ok(info) => {
            let res_json = serde_json::to_string_pretty(&info).unwrap_or("{}".to_string());
            (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(res_json), error: None }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

pub async fn handle_backup_restore(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<BackupRestoreRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, true) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    match state.backup_manager.restore_snapshot(&req.snapshot_id) {
        Ok(info) => {
            let res_json = serde_json::to_string_pretty(&info).unwrap_or("{}".to_string());
            (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(res_json), error: None }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

pub async fn handle_backup_pitr(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<BackupPitrRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, true) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    match state.backup_manager.point_in_time_recovery(req.target_timestamp) {
        Ok(replayed) => {
            (StatusCode::OK, Json(QueryResponse {
                status: "success".to_string(),
                result: Some(format!("Point-in-time recovery succeeded. Replayed {} transaction(s).", replayed)),
                error: None
            }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

#[derive(Deserialize)]
pub struct BackupExportPbRequest {
    pub token: String,
    pub export_type: Option<String>, // "database" or "cluster"
    pub database: Option<String>,
    pub file_path: String,
    pub encrypt: Option<bool>,
    pub key: Option<String>,
}

#[derive(Deserialize)]
pub struct BackupImportPbRequest {
    pub token: String,
    pub file_path: String,
    pub key: Option<String>,
    pub target_db: Option<String>,
}

pub async fn handle_backup_export_pb(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<BackupExportPbRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, true) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    let export_type = req.export_type.unwrap_or_else(|| "database".to_string());
    let database = req.database.as_deref().unwrap_or("default");
    let encrypt = req.encrypt.unwrap_or(req.key.is_some());

    match crate::backup::PbPackageManager::export_pb(
        &state.storage,
        &export_type,
        database,
        &req.file_path,
        encrypt,
        req.key.as_deref(),
    ) {
        Ok(report) => {
            let res_json = serde_json::to_string_pretty(&report).unwrap_or("{}".to_string());
            (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(res_json), error: None }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

pub async fn handle_backup_import_pb(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<BackupImportPbRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, true) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    match crate::backup::PbPackageManager::import_pb(
        &state.storage,
        &req.file_path,
        req.key.as_deref(),
        req.target_db.as_deref(),
    ) {
        Ok(report) => {
            let res_json = serde_json::to_string_pretty(&report).unwrap_or("{}".to_string());
            (StatusCode::OK, Json(QueryResponse { status: "success".to_string(), result: Some(res_json), error: None }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e.to_string()) })),
    }
}

// Universal Migration Framework Payloads & Handlers
#[derive(Deserialize)]
pub struct MigrationStartApiRequest {
    pub token: String,
    pub source_type: String,
    pub source_path: String,
    pub target_table: String,
    pub schema_policy: Option<String>,
    pub primary_key: Option<String>,
    pub column_renames: Option<HashMap<String, String>>,
    pub embedding_configs: Option<Vec<crate::migration::EmbeddingConfig>>,
    pub batch_size: Option<usize>,
    pub workers: Option<usize>,
}

#[derive(Deserialize)]
pub struct MigrationTokenOnlyRequest {
    pub token: String,
}

pub async fn handle_migration_start(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<MigrationStartApiRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, true) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    let mut mapping = crate::migration::MigrationMapping::default();
    if let Some(sp) = req.schema_policy {
        mapping.schema_policy = crate::migration::SchemaPolicy::from_str(&sp);
    }
    mapping.primary_key = req.primary_key;
    if let Some(renames) = req.column_renames {
        mapping.column_renames = renames;
    }
    if let Some(embs) = req.embedding_configs {
        mapping.embedding_configs = embs;
    }

    let mut config = crate::migration::MigrationConfig::default();
    if let Some(bs) = req.batch_size {
        config.batch_size = bs;
    }
    if let Some(w) = req.workers {
        config.workers = w;
    }

    let embedding_mgr = Arc::new(state.executor.embedding_manager.clone());
    let target_table = if req.target_table.starts_with("default_") {
        req.target_table
    } else {
        format!("default_{}", req.target_table)
    };
    match state.migration_manager.start_job(
        req.source_type,
        req.source_path,
        target_table,
        mapping,
        config,
        embedding_mgr,
        Some(state.tracer.clone()),
    ) {
        Ok(job_id) => (StatusCode::OK, Json(QueryResponse {
            status: "success".to_string(),
            result: Some(job_id),
            error: None,
        })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse {
            status: "error".to_string(),
            result: None,
            error: Some(e.to_string()),
        })),
    }
}

pub async fn handle_migration_resume(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<MigrationTokenOnlyRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, true) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    let embedding_mgr = Arc::new(state.executor.embedding_manager.clone());
    match state.migration_manager.resume_job(&job_id, embedding_mgr, Some(state.tracer.clone())) {
        Ok(res_id) => (StatusCode::OK, Json(QueryResponse {
            status: "success".to_string(),
            result: Some(format!("Migration job '{}' resumed successfully", res_id)),
            error: None,
        })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse {
            status: "error".to_string(),
            result: None,
            error: Some(e.to_string()),
        })),
    }
}

pub async fn handle_migration_abort(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<MigrationTokenOnlyRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, true) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    match state.migration_manager.abort_job(&job_id) {
        Ok(_) => (StatusCode::OK, Json(QueryResponse {
            status: "success".to_string(),
            result: Some(format!("Migration job '{}' aborted successfully", job_id)),
            error: None,
        })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse {
            status: "error".to_string(),
            result: None,
            error: Some(e.to_string()),
        })),
    }
}

pub async fn handle_migration_status(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    match state.migration_manager.get_status(&job_id) {
        Ok(Some(job)) => {
            let res_json = serde_json::to_string_pretty(&job).unwrap_or("{}".to_string());
            (StatusCode::OK, Json(QueryResponse {
                status: "success".to_string(),
                result: Some(res_json),
                error: None,
            }))
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(QueryResponse {
            status: "error".to_string(),
            result: None,
            error: Some(format!("Migration job '{}' not found", job_id)),
        })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse {
            status: "error".to_string(),
            result: None,
            error: Some(e.to_string()),
        })),
    }
}

pub async fn handle_migration_jobs(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.migration_manager.list_jobs() {
        Ok(jobs) => {
            let res_json = serde_json::to_string_pretty(&jobs).unwrap_or("[]".to_string());
            (StatusCode::OK, Json(QueryResponse {
                status: "success".to_string(),
                result: Some(res_json),
                error: None,
            }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse {
            status: "error".to_string(),
            result: None,
            error: Some(e.to_string()),
        })),
    }
}

pub async fn handle_migration_validate(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    match state.migration_manager.validate_job(&job_id) {
        Ok(report) => {
            let res_json = serde_json::to_string_pretty(&report).unwrap_or("{}".to_string());
            (StatusCode::OK, Json(QueryResponse {
                status: "success".to_string(),
                result: Some(res_json),
                error: None,
            }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse {
            status: "error".to_string(),
            result: None,
            error: Some(e.to_string()),
        })),
    }
}

// Enterprise Security Payloads & Handlers
#[derive(Deserialize)]
pub struct CreateApiKeyRequest {
    pub token: String,
    pub name: String,
    pub roles: Vec<String>,
    pub tenant_id: Option<String>,
    pub ttl_secs: Option<u64>,
}

#[derive(Deserialize)]
pub struct RevokeApiKeyRequest {
    pub token: String,
    pub key_id: String,
}

#[derive(Deserialize)]
pub struct ListApiKeysRequest {
    pub token: String,
}

#[derive(Deserialize)]
pub struct CreateRoleRequest {
    pub token: String,
    pub name: String,
    pub description: String,
    pub permissions: Vec<String>,
    pub scope: Option<String>, // "global", "database:xyz", "tenant:xyz"
}

#[derive(Deserialize)]
pub struct EncryptPayloadRequest {
    pub token: String,
    pub plaintext: String,
}

#[derive(Deserialize)]
pub struct DecryptPayloadRequest {
    pub token: String,
    pub ciphertext_hex: String,
}

pub async fn handle_apikey_create(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<CreateApiKeyRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, true) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    match state.api_key_manager.create_key(&req.name, req.roles, req.tenant_id, req.ttl_secs) {
        Ok((meta, raw_secret)) => {
            let res = serde_json::json!({
                "key_id": meta.key_id,
                "name": meta.name,
                "prefix": meta.prefix,
                "api_key": raw_secret,
                "roles": meta.roles,
                "tenant_id": meta.tenant_id,
                "expires_at": meta.expires_at,
                "created_at": meta.created_at
            });
            (StatusCode::OK, Json(QueryResponse {
                status: "success".to_string(),
                result: Some(serde_json::to_string_pretty(&res).unwrap_or_default()),
                error: None,
            }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse {
            status: "error".to_string(),
            result: None,
            error: Some(e.to_string()),
        })),
    }
}

pub async fn handle_apikey_revoke(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<RevokeApiKeyRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, true) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    match state.api_key_manager.revoke_key(&req.key_id) {
        Ok(_) => (StatusCode::OK, Json(QueryResponse {
            status: "success".to_string(),
            result: Some(format!("API key '{}' revoked successfully", req.key_id)),
            error: None,
        })),
        Err(e) => (StatusCode::BAD_REQUEST, Json(QueryResponse {
            status: "error".to_string(),
            result: None,
            error: Some(e.to_string()),
        })),
    }
}

pub async fn handle_apikey_list(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<ListApiKeysRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    match state.api_key_manager.list_keys() {
        Ok(keys) => {
            let json_str = serde_json::to_string_pretty(&keys).unwrap_or("[]".to_string());
            (StatusCode::OK, Json(QueryResponse {
                status: "success".to_string(),
                result: Some(json_str),
                error: None,
            }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse {
            status: "error".to_string(),
            result: None,
            error: Some(e.to_string()),
        })),
    }
}

pub async fn handle_role_create(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<CreateRoleRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, true) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    let mut permissions = std::collections::HashSet::new();
    for p in req.permissions {
        match p.as_str() {
            "DatabaseCreate" => { permissions.insert(crate::security::Permission::DatabaseCreate); }
            "DatabaseDrop" => { permissions.insert(crate::security::Permission::DatabaseDrop); }
            "DatabaseRead" => { permissions.insert(crate::security::Permission::DatabaseRead); }
            "DatabaseWrite" => { permissions.insert(crate::security::Permission::DatabaseWrite); }
            "TableCreate" => { permissions.insert(crate::security::Permission::TableCreate); }
            "TableDrop" => { permissions.insert(crate::security::Permission::TableDrop); }
            "TableRead" => { permissions.insert(crate::security::Permission::TableRead); }
            "TableWrite" => { permissions.insert(crate::security::Permission::TableWrite); }
            "VectorSearch" => { permissions.insert(crate::security::Permission::VectorSearch); }
            "VectorInsert" => { permissions.insert(crate::security::Permission::VectorInsert); }
            "GraphRead" => { permissions.insert(crate::security::Permission::GraphRead); }
            "GraphWrite" => { permissions.insert(crate::security::Permission::GraphWrite); }
            "MemoryRecall" => { permissions.insert(crate::security::Permission::MemoryRecall); }
            "MemoryWrite" => { permissions.insert(crate::security::Permission::MemoryWrite); }
            "BackupCreate" => { permissions.insert(crate::security::Permission::BackupCreate); }
            "BackupRestore" => { permissions.insert(crate::security::Permission::BackupRestore); }
            "PITR" => { permissions.insert(crate::security::Permission::PITR); }
            "TenantAdmin" => { permissions.insert(crate::security::Permission::TenantAdmin); }
            "UserAdmin" => { permissions.insert(crate::security::Permission::UserAdmin); }
            "AuditRead" => { permissions.insert(crate::security::Permission::AuditRead); }
            _ => continue,
        };
    }

    let scope = match req.scope.as_deref() {
        Some(s) if s.starts_with("database:") => {
            crate::security::RoleScope::Database(s.trim_start_matches("database:").to_string())
        }
        Some(s) if s.starts_with("tenant:") => {
            crate::security::RoleScope::Tenant(s.trim_start_matches("tenant:").to_string())
        }
        _ => crate::security::RoleScope::Global,
    };

    let def = crate::security::RoleDefinition {
        name: req.name.clone(),
        description: req.description,
        permissions,
        scope,
    };

    match state.role_manager.create_role(def) {
        Ok(_) => (StatusCode::OK, Json(QueryResponse {
            status: "success".to_string(),
            result: Some(format!("Role '{}' created successfully", req.name)),
            error: None,
        })),
        Err(e) => (StatusCode::BAD_REQUEST, Json(QueryResponse {
            status: "error".to_string(),
            result: None,
            error: Some(e.to_string()),
        })),
    }
}

pub async fn handle_role_list(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let roles = state.role_manager.list_roles();
    let json_str = serde_json::to_string_pretty(&roles).unwrap_or("[]".to_string());
    (StatusCode::OK, Json(QueryResponse {
        status: "success".to_string(),
        result: Some(json_str),
        error: None,
    }))
}

pub async fn handle_security_encrypt(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<EncryptPayloadRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    match state.crypto_engine.encrypt(req.plaintext.as_bytes()) {
        Ok(ciphertext) => {
            let hex_encoded = hex::encode(&ciphertext);
            (StatusCode::OK, Json(QueryResponse {
                status: "success".to_string(),
                result: Some(hex_encoded),
                error: None,
            }))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(QueryResponse {
            status: "error".to_string(),
            result: None,
            error: Some(e.to_string()),
        })),
    }
}

pub async fn handle_security_decrypt(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<DecryptPayloadRequest>,
) -> impl IntoResponse {
    let client_ip = addr.ip().to_string();
    if let Err(e) = authenticate_request(&state, &req.token, &client_ip, None, false) {
        return (StatusCode::UNAUTHORIZED, Json(QueryResponse { status: "error".to_string(), result: None, error: Some(e) }));
    }

    let raw_bytes = match hex::decode(&req.ciphertext_hex) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(QueryResponse {
            status: "error".to_string(),
            result: None,
            error: Some(format!("Invalid hex string: {}", e)),
        })),
    };

    match state.crypto_engine.decrypt(&raw_bytes) {
        Ok(plaintext_bytes) => {
            let text = String::from_utf8_lossy(&plaintext_bytes).to_string();
            (StatusCode::OK, Json(QueryResponse {
                status: "success".to_string(),
                result: Some(text),
                error: None,
            }))
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(QueryResponse {
            status: "error".to_string(),
            result: None,
            error: Some(e.to_string()),
        })),
    }
}


