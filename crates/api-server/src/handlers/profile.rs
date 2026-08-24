use academic_core::models::UserProfile;
use academic_core::repo::profile;
use axum::extract::State;
use axum::Json;

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::state::AppState;

pub async fn get_profile(State(state): State<AppState>, _user: AuthUser) -> Result<Json<Option<UserProfile>>, ApiError> {
    Ok(Json(profile::get(&state.pool).await?))
}

pub async fn save_profile(
    State(state): State<AppState>,
    _user: AuthUser,
    Json(input): Json<UserProfile>,
) -> Result<Json<UserProfile>, ApiError> {
    profile::upsert(&state.pool, &input).await?;
    Ok(Json(input))
}
