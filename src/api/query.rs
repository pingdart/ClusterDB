use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};

use crate::api::data::resolve_tenant_db;
use crate::api::dto::QueryApiRequest;
use crate::api::error::{ApiError, ApiResult};
use crate::server::AppState;

/// POST /v1/query and POST /v1/data/query - PRD Section 6.5 Execute SQL or structured query
pub async fn execute_query(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<QueryApiRequest>,
) -> ApiResult<impl IntoResponse> {
    let db = resolve_tenant_db(&headers, payload.database.as_deref());
    let tenant = payload.tenant_id.as_deref().unwrap_or(&db);

    if payload.query.trim().is_empty() {
        return Err(ApiError::bad_request("Query cannot be empty"));
    }

    match state
        .executor
        .execute_with_tenant(&payload.query, Some(&db), Some(tenant))
    {
        Ok(result_str) => {
            // Attempt to parse string as JSON for clean nested response
            let result_val = serde_json::from_str::<serde_json::Value>(&result_str)
                .unwrap_or_else(|_| serde_json::Value::String(result_str));

            Ok((
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "success",
                    "database": db,
                    "result": result_val
                })),
            ))
        }
        Err(e) => Err(ApiError::bad_request(format!("Query error: {}", e))),
    }
}
