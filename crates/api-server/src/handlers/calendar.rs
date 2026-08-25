use academic_core::models::{CalendarFeed, NewCalendarFeed, Syllabus};
use academic_core::repo::{calendar_feeds, syllabus};
use axum::extract::{Path, State};
use axum::Json;

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
