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

impl ApiError {
    /// The status a `CoreError` deserves, usable without owning it — so a
    /// wrapper error can delegate instead of duplicating the mapping and
    /// drifting from it.
    fn from_core_ref(e: &academic_core::CoreError, message: String) -> Self {
        let status = match e {
            academic_core::CoreError::NotFound(_) => StatusCode::NOT_FOUND,
            academic_core::CoreError::Validation(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError::new(status, message)
    }
}

impl From<academic_core::CoreError> for ApiError {
    fn from(e: academic_core::CoreError) -> Self {
        let message = e.to_string();
        ApiError::from_core_ref(&e, message)
    }
}

impl From<ai_engine::AiError> for ApiError {
    fn from(e: ai_engine::AiError) -> Self {
        ApiError::new(StatusCode::BAD_GATEWAY, e.to_string())
    }
}

/// Mapped per variant rather than blanket-502. A `DocumentError` can be the
/// caller's fault (empty material, a deck that does not exist) just as easily as
/// an upstream failure, and reporting "502 Bad Gateway" for "you sent no text"
/// tells the user the server is broken when their input was.
impl From<document_engine::DocumentError> for ApiError {
    fn from(e: document_engine::DocumentError) -> Self {
        use document_engine::DocumentError as D;
        let status = match &e {
            // Delegate to the CoreError mapping so validation stays a 400 and a
            // missing row stays a 404 wherever it is surfaced from.
            D::Core(inner) => return ApiError::from_core_ref(inner, e.to_string()),
            D::SyllabusNotFound(_) | D::FeedNotFound(_) | D::CourseNotFound(_) => StatusCode::NOT_FOUND,
            // The model ran but produced nothing usable. Not the caller's
            // fault, and not the gateway being unreachable either.
            D::EmptyGeneration(_) => StatusCode::UNPROCESSABLE_ENTITY,
            // The AI subprocess genuinely failed, timed out, or is unavailable.
            D::Ai(_) => StatusCode::BAD_GATEWAY,
            D::FeedFetch(_) => StatusCode::BAD_GATEWAY,
        };
        ApiError::new(status, e.to_string())
    }
}
