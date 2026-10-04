use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AiError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    Low,
    Medium,
    High,
}

impl Effort {
    pub fn as_str(&self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CompletionRequest {
    pub system_prompt: Option<String>,
    pub prompt: String,
    pub effort: Option<Effort>,
}

#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub text: String,
    pub total_cost_usd: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct ExtractionRequest {
    pub system_prompt: Option<String>,
    pub prompt: String,
    pub json_schema: serde_json::Value,
}

/// The JSON a structured extraction produced, plus what the call cost.
/// Cost travels with the result rather than being discarded, so callers can
/// record and eventually cap real spend — the other half of that is
/// `ClaudeCliProvider::with_max_input_chars`, which bounds what a single call
/// can be asked to read in the first place.
#[derive(Debug, Clone)]
pub struct ExtractionResponse {
    pub value: serde_json::Value,
    pub total_cost_usd: Option<f64>,
}

/// The single seam between the rest of the app and however Claude actually
/// gets invoked. Nothing outside this crate should know or care whether
/// that's a CLI subprocess, a future local model, or anything else — see
/// `ClaudeCliProvider` for the implementation this app ships with.
#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn is_available(&self) -> bool;
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AiError>;
    async fn extract_structured(&self, request: ExtractionRequest) -> Result<ExtractionResponse, AiError>;
}
