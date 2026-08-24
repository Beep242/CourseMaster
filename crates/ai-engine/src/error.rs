#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("the `claude` CLI was not found on PATH — install Claude Code and sign in to enable AI features")]
    ProviderUnavailable,
    #[error("claude CLI exited with an error: {0}")]
    ProcessFailed(String),
    #[error("claude CLI produced output that could not be parsed: {0}")]
    InvalidOutput(String),
    #[error("AI request timed out after {0}s")]
    Timeout(u64),
    #[error("io error spawning claude CLI: {0}")]
    Io(#[from] std::io::Error),
}
