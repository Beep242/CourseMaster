use academic_core::models::{CalendarFeed, Course, NewCalendarFeed, Syllabus};
use academic_core::repo::{calendar_feeds, syllabus};
use axum::extract::{Path, State};
use axum::Json;
use document_engine::DetectedCourseGroup;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::state::AppState;

pub async fn create_feed(
    State(state): State<AppState>,
    _user: AuthUser,
    Json(mut input): Json<NewCalendarFeed>,
) -> Result<Json<CalendarFeed>, ApiError> {
    input.ics_url = document_engine::calendar_feed::normalize_ics_url(&input.ics_url);
    Ok(Json(calendar_feeds::create(&state.pool, input).await?))
}

pub async fn list_feeds(State(state): State<AppState>, _user: AuthUser) -> Result<Json<Vec<CalendarFeed>>, ApiError> {
    Ok(Json(calendar_feeds::list(&state.pool).await?))
}

pub async fn sync_feed(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<usize>, ApiError> {
    Ok(Json(document_engine::sync_feed(&state.pool, &id).await?))
}

pub async fn list_feed_batches(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Vec<Syllabus>>, ApiError> {
    Ok(Json(syllabus::list_by_feed(&state.pool, &id).await?))
}

pub async fn detected_courses(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Vec<DetectedCourseGroup>>, ApiError> {
    Ok(Json(document_engine::detect_course_groups(&state.pool, &id).await?))
}

#[derive(Debug, Deserialize)]
pub struct LinkCourseBody {
    pub semester_id: String,
    pub name: String,
    pub code: Option<String>,
    pub org_unit_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LinkCourseResponse {
    pub course: Course,
    pub backfilled_extractions: u64,
}

pub async fn link_course(
    State(state): State<AppState>,
    _user: AuthUser,
    Json(body): Json<LinkCourseBody>,
) -> Result<Json<LinkCourseResponse>, ApiError> {
    let (course, backfilled_extractions) = document_engine::link_course_from_group(
        &state.pool,
        &body.semester_id,
        body.org_unit_id.as_deref(),
        &body.name,
        body.code.as_deref(),
    )
    .await?;
    Ok(Json(LinkCourseResponse { course, backfilled_extractions }))
}
