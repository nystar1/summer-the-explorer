use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Input validation failed: {field}")]
    Validation { field: String, message: String },

    #[error("External API request failed: {0}")]
    ExternalApi(String),

    #[error("Embedding generation failed: {0}")]
    Embedding(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Resource not found: {resource}")]
    NotFound { resource: String, id: String },

    #[error("Rate limit exceeded: {message}")]
    RateLimit { retry_after: u64, message: String },
}

impl ApiError {
    const DB_ERROR: &'static str = "DB_ERROR";
    const VALIDATION_ERROR: &'static str = "VALIDATION_ERROR";
    const EXTERNAL_API_ERROR: &'static str = "EXTERNAL_API_ERROR";
    const EMBEDDING_ERROR: &'static str = "EMBEDDING_ERROR";
    const CONFIG_ERROR: &'static str = "CONFIG_ERROR";
    const NOT_FOUND: &'static str = "NOT_FOUND";
    const RATE_LIMITED: &'static str = "RATE_LIMITED";
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message, code) = match &self {
            Self::Database(e) => {
                tracing::error!("Database error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".into(),
                    Self::DB_ERROR,
                )
            }
            Self::Validation { message, .. } => (
                StatusCode::BAD_REQUEST,
                message.clone(),
                Self::VALIDATION_ERROR,
            ),
            Self::ExternalApi(msg) => (
                StatusCode::BAD_GATEWAY,
                msg.clone(),
                Self::EXTERNAL_API_ERROR,
            ),
            Self::Embedding(msg) => {
                tracing::error!("Embedding error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Embedding generation failed".into(),
                    Self::EMBEDDING_ERROR,
                )
            }
            Self::Config(msg) => {
                tracing::error!("Configuration error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Configuration error".into(),
                    Self::CONFIG_ERROR,
                )
            }
            Self::NotFound { resource, id } => (
                StatusCode::NOT_FOUND,
                format!("{resource} with id {id} not found"),
                Self::NOT_FOUND,
            ),
            Self::RateLimit {
                retry_after,
                message,
            } => {
                let body = json!({
                    "error": message,
                    "error_code": Self::RATE_LIMITED,
                    "status": StatusCode::TOO_MANY_REQUESTS.as_u16(),
                    "retry_after": retry_after
                });

                let mut response = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();
                response
                    .headers_mut()
                    .insert("Retry-After", retry_after.to_string().parse().unwrap());
                return response;
            }
        };

        let body = json!({
            "error": message,
            "error_code": code,
            "status": status.as_u16()
        });

        (status, Json(body)).into_response()
    }
}

impl From<tokio_postgres::Error> for ApiError {
    fn from(err: tokio_postgres::Error) -> Self {
        Self::Database(err.to_string())
    }
}

impl From<deadpool_postgres::PoolError> for ApiError {
    fn from(err: deadpool_postgres::PoolError) -> Self {
        Self::Database(err.to_string())
    }
}

impl From<chrono::ParseError> for ApiError {
    fn from(err: chrono::ParseError) -> Self {
        Self::Validation {
            field: "date".to_string(),
            message: format!("Invalid date format: {err}"),
        }
    }
}

impl From<std::io::Error> for ApiError {
    fn from(err: std::io::Error) -> Self {
        Self::ExternalApi(format!("IO error: {err}"))
    }
}


impl From<ort::Error> for ApiError {
    fn from(err: ort::Error) -> Self {
        Self::Embedding(err.to_string())
    }
}

impl From<ndarray::ShapeError> for ApiError {
    fn from(err: ndarray::ShapeError) -> Self {
        Self::Embedding(format!("Tensor shape error: {err}"))
    }
}

pub type Result<T> = std::result::Result<T, ApiError>;
