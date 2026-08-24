use academic_core::models::{ExtractionEdits, Syllabus, SyllabusExtraction};
use academic_core::repo::syllabus;
use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SubmitSyllabusBody {
    pub course_id: String,
    pub raw_text: String,
}

pub async fn submit_syllabus(
    State(state): State<AppState>,
    _user: AuthUser,
    Json(body): Json<SubmitSyllabusBody>,
) -> Result<Json<Syllabus>, ApiError> {
    let created = syllabus::create_syllabus(&state.pool, &body.course_id, &body.raw_text).await?;
    if let Err(err) = document_engine::process_syllabus(&state.pool, state.ai.as_ref(), &created.id).await {
        tracing::warn!("syllabus extraction failed for {}: {err}", created.id);
    }
    let refreshed = syllabus::get_syllabus(&state.pool, &created.id)
        .await?
        .ok_or_else(|| ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "syllabus vanished after processing"))?;
    Ok(Json(refreshed))
}

pub async fn list_syllabi(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(course_id): Path<String>,
) -> Result<Json<Vec<Syllabus>>, ApiError> {
    Ok(Json(syllabus::list_by_course(&state.pool, &course_id).await?))
}

pub async fn list_extractions(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(syllabus_id): Path<String>,
) -> Result<Json<Vec<SyllabusExtraction>>, ApiError> {
    Ok(Json(syllabus::list_extractions(&state.pool, &syllabus_id).await?))
}

#[derive(Debug, Deserialize, Default)]
pub struct ApproveBody {
    #[serde(default)]
    pub edits: Option<ExtractionEdits>,
}

pub async fn approve_extraction(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<ApproveBody>,
) -> Result<Json<String>, ApiError> {
    Ok(Json(syllabus::approve_extraction(&state.pool, &id, body.edits).await?))
}

pub async fn reject_extraction(State(state): State<AppState>, _user: AuthUser, Path(id): Path<String>) -> Result<(), ApiError> {
    Ok(syllabus::reject_extraction(&state.pool, &id).await?)
}

#[derive(Debug, Deserialize)]
pub struct AskBody {
    pub question: String,
}

pub async fn ask_syllabus(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(syllabus_id): Path<String>,
    Json(body): Json<AskBody>,
) -> Result<Json<String>, ApiError> {
    let record = syllabus::get_syllabus(&state.pool, &syllabus_id)
        .await?
        .ok_or_else(|| ApiError::new(axum::http::StatusCode::NOT_FOUND, format!("syllabus {syllabus_id} not found")))?;
    let answer = document_engine::ask_syllabus(state.ai.as_ref(), &record.raw_text, &body.question).await?;
    Ok(Json(answer))
}
