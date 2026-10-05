#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error("syllabus {0} not found")]
    SyllabusNotFound(String),
    #[error("calendar feed {0} not found")]
    FeedNotFound(String),
    #[error("couldn't reach the calendar feed: {0}")]
    FeedFetch(String),
    #[error("course {0} not found")]
    CourseNotFound(String),
    #[error("the AI didn't return any usable {0}")]
    EmptyGeneration(String),
    /// A file the caller supplied that cannot be read — the wrong format, a
    /// corrupt archive, or too large. Distinct from FeedFetch so it does not
    /// surface as "couldn't reach the calendar feed" when someone uploads a
    /// text file, and so it maps to 400 rather than 502.
    #[error("{0}")]
    Unreadable(String),
    #[error(transparent)]
    Ai(#[from] ai_engine::AiError),
    #[error(transparent)]
    Core(#[from] academic_core::CoreError),
}
