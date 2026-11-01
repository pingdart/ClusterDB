use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

/// PRD Section 23 Standard Machine-Readable Error Detail
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// PRD Section 23 Standard Error Response Envelope
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ApiErrorResponse {
    pub status: String,
    pub error: ErrorDetail,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum ApiError {
    NotFound { code: &'static str, message: String },
    BadRequest { code: &'static str, message: String },
    Internal { code: &'static str, message: String },
    Unauthorized { code: &'static str, message: String },
    Forbidden { code: &'static str, message: String },
    Conflict { code: &'static str, message: String },
}

impl ApiError {
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound {
            code: "RECORD_NOT_FOUND",
            message: message.into(),
        }
    }

    pub fn table_not_found(table: &str) -> Self {
        Self::NotFound {
            code: "TABLE_NOT_FOUND",
            message: format!("Table '{}' not found in database", table),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest {
            code: "VALIDATION_ERROR",
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            code: "INTERNAL_ERROR",
            message: message.into(),
        }
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized {
            code: "UNAUTHORIZED",
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict {
            code: "CONFLICT",
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            ApiError::NotFound { code, message } => (StatusCode::NOT_FOUND, code, message),
            ApiError::BadRequest { code, message } => (StatusCode::BAD_REQUEST, code, message),
            ApiError::Internal { code, message } => (StatusCode::INTERNAL_SERVER_ERROR, code, message),
            ApiError::Unauthorized { code, message } => (StatusCode::UNAUTHORIZED, code, message),
            ApiError::Forbidden { code, message } => (StatusCode::FORBIDDEN, code, message),
            ApiError::Conflict { code, message } => (StatusCode::CONFLICT, code, message),
        };

        let req_id = format!("req_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let resp = ApiErrorResponse {
            status: "error".to_string(),
            error: ErrorDetail {
                code: code.to_string(),
                message: message.clone(),
                request_id: Some(req_id),
            },
            message,
        };
        (status, Json(resp)).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
