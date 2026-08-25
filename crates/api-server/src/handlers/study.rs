use academic_core::models::{PracticeAttempt, PracticeDifficulty, PracticeQuestion, PracticeTest, StudyGuide, StudyGuideKind, SubmittedAnswer};
use academic_core::repo::{practice_tests, study_guides};
use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct GenerateStudyGuideBody {
    pub kind: StudyGuideKind,
    #[serde(default)]
    pub material: Option<String>,
}

pub async fn generate_study_guide(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(course_id): Path<String>,
    Json(body): Json<GenerateStudyGuideBody>,
) -> Result<Json<StudyGuide>, ApiError> {
    let guide = document_engine::generate_study_guide(&state.pool, state.ai.as_ref(), &course_id, body.kind, body.material.as_deref()).await?;
    Ok(Json(guide))
}

pub async fn list_study_guides(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(course_id): Path<String>,
) -> Result<Json<Vec<StudyGuide>>, ApiError> {
    Ok(Json(study_guides::list_by_course(&state.pool, &course_id).await?))
}

pub async fn get_study_guide(State(state): State<AppState>, _user: AuthUser, Path(id): Path<String>) -> Result<Json<Option<StudyGuide>>, ApiError> {
    Ok(Json(study_guides::get(&state.pool, &id).await?))
}

#[derive(Debug, Deserialize)]
pub struct GeneratePracticeTestBody {
    #[serde(default)]
    pub difficulty: Option<PracticeDifficulty>,
    #[serde(default)]
    pub question_count: Option<u32>,
    #[serde(default)]
    pub material: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PracticeTestWithQuestions {
    #[serde(flatten)]
    pub test: PracticeTest,
    pub questions: Vec<PracticeQuestion>,
}

pub async fn generate_practice_test(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(course_id): Path<String>,
    Json(body): Json<GeneratePracticeTestBody>,
) -> Result<Json<PracticeTestWithQuestions>, ApiError> {
    let (test, questions) = document_engine::generate_practice_test(
        &state.pool,
        state.ai.as_ref(),
        &course_id,
        body.difficulty.unwrap_or(PracticeDifficulty::Medium),
        body.question_count.unwrap_or(8),
        body.material.as_deref(),
    )
    .await?;
    Ok(Json(PracticeTestWithQuestions { test, questions }))
}

pub async fn list_practice_tests(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(course_id): Path<String>,
) -> Result<Json<Vec<PracticeTest>>, ApiError> {
    Ok(Json(practice_tests::list_by_course(&state.pool, &course_id).await?))
}

pub async fn get_practice_test(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Option<PracticeTestWithQuestions>>, ApiError> {
    let Some(test) = practice_tests::get_test(&state.pool, &id).await? else {
        return Ok(Json(None));
    };
    let questions = practice_tests::list_questions(&state.pool, &id).await?;
    Ok(Json(Some(PracticeTestWithQuestions { test, questions })))
}

#[derive(Debug, Deserialize)]
pub struct SubmitAttemptBody {
    pub answers: Vec<SubmittedAnswer>,
}

pub async fn submit_attempt(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<SubmitAttemptBody>,
) -> Result<Json<PracticeAttempt>, ApiError> {
    let attempt = document_engine::grade_attempt(&state.pool, state.ai.as_ref(), &id, body.answers).await?;
    Ok(Json(attempt))
}

pub async fn list_attempts(State(state): State<AppState>, _user: AuthUser, Path(id): Path<String>) -> Result<Json<Vec<PracticeAttempt>>, ApiError> {
    Ok(Json(practice_tests::list_attempts(&state.pool, &id).await?))
}
