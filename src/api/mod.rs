pub mod data;
pub mod dto;
pub mod error;
pub mod query;
pub mod schema;

pub use dto::*;
pub use error::{ApiError, ApiErrorResponse, ApiResult};

use axum::{
    routing::{get, post},
    Router,
};
use crate::server::AppState;

/// Assembles all PRD-compliant Database and REST CRUD API routes
pub fn router() -> Router<AppState> {
    Router::new()
        // PRD Section 6.4: REST Data CRUD Endpoints
        .route("/v1/data/:table", post(data::insert_record).get(data::list_records))
        .route(
            "/v1/data/:table/:id",
            get(data::get_record)
                .patch(data::update_record)
                .put(data::update_record)
                .delete(data::delete_record),
        )
        // PRD Section 6.2 & 22: Schema & Table Metadata Endpoints
        .route("/v1/schema/tables", get(schema::list_tables).post(schema::create_table))
        .route(
            "/v1/schema/tables/:table",
            get(schema::get_table).delete(schema::drop_table),
        )
        .route("/v1/schema/tables/:table/truncate", post(schema::truncate_table))
        // PRD Section 6.5: Structured Query Endpoints
        .route("/v1/query", post(query::execute_query))
        .route("/v1/data/query", post(query::execute_query))
}
