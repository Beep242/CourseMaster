mod auth;
mod error;
mod handlers;
mod state;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use ai_engine::{AiProvider, ClaudeCliProvider};
use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use state::AppState;

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let db_path = env_or("SQLITE_PATH", "./data/coursemaster.db");
    let pool = academic_core::db::connect(std::path::Path::new(&db_path))
        .await
        .expect("database connects and migrations run");

    let ai_scratch = PathBuf::from(env_or("AI_SCRATCH_DIR", "./data/ai-scratch"));
    std::fs::create_dir_all(&ai_scratch).ok();
    let mut provider = ClaudeCliProvider::new().with_working_dir(ai_scratch);
    if let Ok(model) = std::env::var("CLAUDE_MODEL") {
        provider = provider.with_model(model);
    }
    let ai: Arc<dyn AiProvider> = Arc::new(provider);

    let state = AppState {
        pool,
        ai,
        jwt_secret: std::env::var("CROSS_APP_JWT_SECRET").expect("CROSS_APP_JWT_SECRET must be set"),
        jwt_issuer: env_or("CROSS_APP_JWT_ISSUER", "iambeep.com"),
        jwt_audience: env_or("CROSS_APP_JWT_AUDIENCE", "coursemaster"),
        owner_email: std::env::var("OWNER_EMAIL").expect("OWNER_EMAIL must be set"),
    };

    let api_routes = Router::new()
        .route("/me", get(handlers::ai::me))
        .route("/ai/status", get(handlers::ai::ai_status))
        .route("/profile", get(handlers::profile::get_profile).put(handlers::profile::save_profile))
        .route("/semesters", get(handlers::courses::list_semesters).post(handlers::courses::create_semester))
        .route("/courses", get(handlers::courses::list_courses).post(handlers::courses::create_course))
        .route("/courses/{id}", get(handlers::courses::get_course))
        .route("/courses/{id}/grade", axum::routing::patch(handlers::courses::update_course_grade))
        .route("/courses/{id}/syllabi", get(handlers::syllabus::list_syllabi))
        .route(
            "/assignments",
            get(handlers::assignments::list_assignments).post(handlers::assignments::create_assignment),
        )
        .route(
            "/assignments/{id}",
            axum::routing::patch(handlers::assignments::update_assignment).delete(handlers::assignments::delete_assignment),
        )
        .route("/assignments/{id}/subtasks", get(handlers::assignments::list_subtasks))
        .route("/subtasks", post(handlers::assignments::create_subtask))
        .route("/syllabi", post(handlers::syllabus::submit_syllabus))
        .route("/syllabi/{id}/extractions", get(handlers::syllabus::list_extractions))
        .route("/syllabi/{id}/ask", post(handlers::syllabus::ask_syllabus))
        .route("/extractions/{id}/approve", post(handlers::syllabus::approve_extraction))
        .route("/extractions/{id}/reject", post(handlers::syllabus::reject_extraction))
        .route("/prioritized", get(handlers::scheduler::prioritized_today));

    let allowed_origins: Vec<_> = env_or("WEB_ALLOWED_ORIGINS", "http://localhost:1430")
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed_origins))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let static_dir = env_or("STATIC_DIR", "../../ui/dist");
    let index_path = format!("{static_dir}/index.html");
    let serve_dir = ServeDir::new(&static_dir).not_found_service(ServeFile::new(index_path));

    let app = Router::new()
        .nest("/api", api_routes)
        .fallback_service(serve_dir)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let port: u16 = env_or("PORT", "8080").parse().expect("PORT must be a number");
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("CourseMaster API listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind listener");
    axum::serve(listener, app).await.expect("server error");
}
