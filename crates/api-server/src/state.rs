use std::sync::Arc;

use academic_core::SqlitePool;
use ai_engine::AiProvider;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub ai: Arc<dyn AiProvider>,
    pub jwt_secret: String,
    pub jwt_issuer: String,
    pub jwt_audience: String,
    pub owner_email: String,
}
