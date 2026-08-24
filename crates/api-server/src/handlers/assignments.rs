use academic_core::models::{Assignment, AssignmentUpdate, NewAssignment, NewSubtask, Subtask};
use academic_core::repo::{assignments, subtasks};
use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::state::AppState;

pub async fn create_assignment(
    State(state): State<AppState>,
    _user: AuthUser,
    Json(input): Json<NewAssignment>,
) -> Result<Json<Assignment>, ApiError> {
    Ok(Json(assignments::create(&state.pool, input).await?))
}

#[derive(Debug, Deserialize)]
pub struct AssignmentQuery {
    pub course_id: Option<String>,
}

pub async fn list_assignments(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(query): Query<AssignmentQuery>,
) -> Result<Json<Vec<Assignment>>, ApiError> {
    match query.course_id {
        Some(id) => Ok(Json(assignments::list_by_course(&state.pool, &id).await?)),
        None => Ok(Json(assignments::list_all(&state.pool).await?)),
    }
}

pub async fn update_assignment(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<String>,
    Json(patch): Json<AssignmentUpdate>,
) -> Result<Json<Assignment>, ApiError> {
    Ok(Json(assignments::update(&state.pool, &id, patch).await?))
}

pub async fn delete_assignment(State(state): State<AppState>, _user: AuthUser, Path(id): Path<String>) -> Result<(), ApiError> {
    Ok(assignments::delete(&state.pool, &id).await?)
}

pub async fn create_subtask(
    State(state): State<AppState>,
    _user: AuthUser,
    Json(input): Json<NewSubtask>,
) -> Result<Json<Subtask>, ApiError> {
    Ok(Json(subtasks::create(&state.pool, input).await?))
}

pub async fn list_subtasks(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(assignment_id): Path<String>,
) -> Result<Json<Vec<Subtask>>, ApiError> {
    Ok(Json(subtasks::list_by_assignment(&state.pool, &assignment_id).await?))
}
