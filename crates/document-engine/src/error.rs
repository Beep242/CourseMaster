#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error("syllabus {0} not found")]
    SyllabusNotFound(String),
    #[error(transparent)]
    Ai(#[from] ai_engine::AiError),
    #[error(transparent)]
    Core(#[from] academic_core::CoreError),
}
