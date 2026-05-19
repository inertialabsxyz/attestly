//! Uniform API error envelope.
//!
//! Every non-2xx response uses the same `{error, detail}` JSON shape, per
//! `API.md`. The slugs are also enumerated there — when you add a variant,
//! mirror it in the doc.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("not found")]
    NotFound,

    #[error("signature invalid: {0}")]
    SignatureInvalid(String),

    #[error("share link not found or expired")]
    LinkNotFound,

    #[error("internal error: {0}")]
    Internal(String),
}

impl ApiError {
    fn slug(&self) -> &'static str {
        match self {
            ApiError::InvalidRequest(_) => "invalid_request",
            ApiError::Unauthorized => "unauthorized",
            ApiError::NotFound => "not_found",
            ApiError::SignatureInvalid(_) => "signature_invalid",
            ApiError::LinkNotFound => "link_not_found",
            ApiError::Internal(_) => "internal",
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            ApiError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized => StatusCode::UNAUTHORIZED,
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::LinkNotFound => StatusCode::NOT_FOUND,
            ApiError::SignatureInvalid(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
    detail: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: self.slug(),
            detail: match &self {
                ApiError::InvalidRequest(d) => d.clone(),
                ApiError::Unauthorized => "missing or invalid x-api-key".into(),
                ApiError::NotFound => "no such resource".into(),
                ApiError::SignatureInvalid(d) => d.clone(),
                ApiError::LinkNotFound => "share link not found, expired, or revoked".into(),
                ApiError::Internal(d) => {
                    // Don't leak internals to clients.
                    tracing::error!(detail = %d, "internal error surfaced to client");
                    "an internal error occurred".into()
                }
            },
        };
        (self.status(), Json(body)).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
