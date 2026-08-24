use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(serde_json::json!({ "error": self.message }))).into_response()
    }
}

impl From<academic_core::CoreError> for ApiError {
    fn from(e: academic_core::CoreError) -> Self {
        let status = match &e {
            academic_core::CoreError::NotFound(_) => StatusCode::NOT_FOUND,
            academic_core::CoreError::Validation(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError::new(status, e.to_string())
    }
}

impl From<ai_engine::AiError> for ApiError {
    fn from(e: ai_engine::AiError) -> Self {
        ApiError::new(StatusCode::BAD_GATEWAY, e.to_string())
    }
}

impl From<document_engine::DocumentError> for ApiError {
    fn from(e: document_engine::DocumentError) -> Self {
        ApiError::new(StatusCode::BAD_GATEWAY, e.to_string())
    }
}
