use axum::extract::State;
use axum::Json;

use crate::auth::AuthUser;
use crate::state::AppState;

pub async fn ai_status(State(state): State<AppState>, _user: AuthUser) -> Json<bool> {
    Json(state.ai.is_available().await)
}

pub async fn me(user: AuthUser) -> Json<AuthUser> {
    Json(user)
}
