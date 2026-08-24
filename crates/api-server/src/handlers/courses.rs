use academic_core::models::{Course, NewCourse, NewSemester, Semester};
use academic_core::repo::{courses, semesters};
use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::state::AppState;

pub async fn create_semester(
    State(state): State<AppState>,
    _user: AuthUser,
    Json(input): Json<NewSemester>,
) -> Result<Json<Semester>, ApiError> {
    Ok(Json(semesters::create(&state.pool, input).await?))
}

pub async fn list_semesters(State(state): State<AppState>, _user: AuthUser) -> Result<Json<Vec<Semester>>, ApiError> {
    Ok(Json(semesters::list(&state.pool).await?))
}

pub async fn create_course(
    State(state): State<AppState>,
    _user: AuthUser,
    Json(input): Json<NewCourse>,
) -> Result<Json<Course>, ApiError> {
    Ok(Json(courses::create(&state.pool, input).await?))
}

#[derive(Debug, Deserialize)]
pub struct CourseQuery {
    pub semester_id: Option<String>,
}

pub async fn list_courses(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(query): Query<CourseQuery>,
) -> Result<Json<Vec<Course>>, ApiError> {
    match query.semester_id {
        Some(id) => Ok(Json(courses::list_by_semester(&state.pool, &id).await?)),
        None => Ok(Json(courses::list_all(&state.pool).await?)),
    }
}

pub async fn get_course(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Option<Course>>, ApiError> {
    Ok(Json(courses::get(&state.pool, &id).await?))
}

#[derive(Debug, Deserialize)]
pub struct GradeUpdate {
    pub grade: Option<String>,
}

pub async fn update_course_grade(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<GradeUpdate>,
) -> Result<(), ApiError> {
    Ok(courses::update_grade(&state.pool, &id, body.grade).await?)
}
