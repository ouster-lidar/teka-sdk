//! API error envelope for app HTTP handlers.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::Value;

/// Closed catalog — shared app runtime (error-codes.md).
pub mod code {
    pub const UNAUTHORIZED: &str = "unauthorized";
    pub const IDENTITY_UNVERIFIED: &str = "identity_unverified";
    pub const FORBIDDEN: &str = "forbidden";
    pub const BUSY: &str = "busy";
    pub const INVALID_REQUEST: &str = "invalid_request";
    pub const NOT_FOUND: &str = "not_found";
}

#[derive(Debug, Clone)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    details: Value,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: ErrorEnvelope<'a>,
}

#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    code: &'a str,
    message: &'a str,
    details: &'a Value,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            details: Value::Object(serde_json::Map::new()),
        }
    }

    pub fn with_details(
        status: StatusCode,
        code: &'static str,
        message: impl Into<String>,
        details: Value,
    ) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            details,
        }
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, code::UNAUTHORIZED, message)
    }

    pub fn identity_unverified(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            code::IDENTITY_UNVERIFIED,
            message,
        )
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, code::FORBIDDEN, message)
    }

    pub fn busy(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code::BUSY, message)
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code::INVALID_REQUEST, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, code::NOT_FOUND, message)
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn error_code(&self) -> &'static str {
        self.code
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: ErrorEnvelope {
                code: self.code,
                message: &self.message,
                details: &self.details,
            },
        };
        (self.status, Json(body)).into_response()
    }
}
