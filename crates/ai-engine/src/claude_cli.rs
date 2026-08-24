use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;

use crate::error::AiError;
use crate::provider::{AiProvider, CompletionRequest, CompletionResponse, Effort, ExtractionRequest};

const DEFAULT_TIMEOUT_SECS: u64 = 180;
const DEFAULT_MAX_BUDGET_USD: f64 = 0.50;

/// Invokes the user's own Claude Code CLI (`claude -p`, headless/print mode)
/// as a subprocess for every AI feature in the app. This is deliberate: the
/// app must not require a separate Anthropic API key or bill against a
/// separate account — it rides on whatever Claude Code authentication
/// (subscription or API key) the user already has configured on this
/// machine, the same way running `claude` in a terminal would. Tool access
/// is disabled (`--tools ""`) and no settings/MCP config is loaded, so each
/// call is a pure prompt-in/text-or-JSON-out request with no ability to
/// read or write files on the user's machine.
#[derive(Debug, Clone)]
pub struct ClaudeCliProvider {
    binary: String,
    model: Option<String>,
    max_budget_usd: Option<f64>,
    timeout_secs: u64,
    working_dir: PathBuf,
}

impl Default for ClaudeCliProvider {
    fn default() -> Self {
        Self {
            binary: "claude".into(),
            model: None,
            max_budget_usd: Some(DEFAULT_MAX_BUDGET_USD),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            working_dir: std::env::temp_dir(),
        }
    }
}

impl ClaudeCliProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// The directory `claude` is spawned from. Kept away from the app's own
    /// source tree so no unrelated CLAUDE.md/hooks/project settings from
    /// wherever the binary happens to run can influence these calls.
    pub fn with_working_dir(mut self, dir: PathBuf) -> Self {
        self.working_dir = dir;
        self
    }

    /// On Windows, `npm install -g` publishes `claude` as a `claude.cmd`
    /// shim (confirmed via `where claude`) — `Command::new("claude")` calls
    /// `CreateProcessW` directly and does not perform `cmd.exe`'s own
    /// PATHEXT extension search, so the bare name alone fails to resolve
    /// even though the shim is genuinely on PATH. Try the configured name
    /// first (covers Unix and anyone who already set an explicit `.exe`/
    /// `.cmd`), then fall back to common Windows shim extensions.
    fn candidate_binaries(&self) -> Vec<String> {
        if cfg!(windows) && !self.binary.contains('.') {
            vec![self.binary.clone(), format!("{}.cmd", self.binary), format!("{}.exe", self.binary)]
        } else {
            vec![self.binary.clone()]
        }
    }

    fn base_command(&self, binary: &str) -> Command {
        let mut cmd = Command::new(binary);
        cmd.current_dir(&self.working_dir)
            .arg("-p")
            .arg("--output-format")
            .arg("json")
            .arg("--tools")
            .arg("")
            .arg("--no-session-persistence")
            .arg("--strict-mcp-config")
            .arg("--setting-sources")
            .arg("")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(model) = &self.model {
            cmd.arg("--model").arg(model);
        }
        if let Some(budget) = self.max_budget_usd {
            cmd.arg("--max-budget-usd").arg(budget.to_string());
        }
        cmd
    }

    /// `--system-prompt` and `--json-schema` both take arbitrary text as a
    /// single CLI argument. On Windows, spawning the `claude.cmd` shim goes
    /// through Rust's automatic `cmd.exe /C` wrapping for `.bat`/`.cmd`
    /// targets, which re-quotes the command line using `cmd.exe`'s own
    /// rules — those don't round-trip a JSON Schema's embedded `"`/`{`/`}`
    /// characters (observed failure: "--json-schema is not valid JSON:
    /// Unterminated string"). Folding both into the stdin-delivered prompt
    /// instead sidesteps argv quoting entirely, on every platform, at the
    /// cost of relying on instruction-following rather than the CLI's
    /// built-in schema enforcement — acceptable since callers already
    /// validate the parsed result (see `document_engine::syllabus_extraction`).
    fn compose_stdin_prompt(system_prompt: Option<&str>, json_schema: Option<&serde_json::Value>, prompt: &str) -> String {
        let mut full = String::new();
        if let Some(sp) = system_prompt {
            full.push_str(sp);
            full.push_str("\n\n---\n\n");
        }
        if let Some(schema) = json_schema {
            full.push_str(
                "Respond with ONLY valid JSON matching exactly this JSON Schema. \
                 No markdown code fences, no prose before or after the JSON.\n\n",
            );
            full.push_str(&serde_json::to_string_pretty(schema).unwrap_or_else(|_| schema.to_string()));
            full.push_str("\n\n---\n\n");
        }
        full.push_str(prompt);
        full
    }

    async fn run(
        &self,
        system_prompt: Option<&str>,
        prompt: &str,
        json_schema: Option<&serde_json::Value>,
        effort: Option<Effort>,
    ) -> Result<CliResult, AiError> {
        let stdin_prompt = Self::compose_stdin_prompt(system_prompt, json_schema, prompt);

        let mut child = None;
        for binary in self.candidate_binaries() {
            let mut cmd = self.base_command(&binary);
            if let Some(effort) = effort {
                cmd.arg("--effort").arg(effort.as_str());
            }
            match cmd.spawn() {
                Ok(spawned) => {
                    child = Some(spawned);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(AiError::Io(e)),
            }
        }
        let mut child = child.ok_or(AiError::ProviderUnavailable)?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(stdin_prompt.as_bytes()).await?;
        }

        let output = timeout(Duration::from_secs(self.timeout_secs), child.wait_with_output())
            .await
            .map_err(|_| AiError::Timeout(self.timeout_secs))??;

        parse_cli_output(&output.stdout, &output.stderr, output.status.success())
    }
}

fn strip_code_fence(s: &str) -> &str {
    let trimmed = s.trim();
    for prefix in ["```json", "```"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest.strip_suffix("```").unwrap_or(rest).trim();
        }
    }
    trimmed
}

#[derive(Debug, Deserialize)]
struct CliResult {
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    total_cost_usd: Option<f64>,
    #[serde(default)]
    error: Option<String>,
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

fn parse_cli_output(stdout: &[u8], stderr: &[u8], process_success: bool) -> Result<CliResult, AiError> {
    let stdout_str = String::from_utf8_lossy(stdout);
    let stderr_str = String::from_utf8_lossy(stderr);
    if stdout_str.trim().is_empty() {
        return Err(AiError::ProcessFailed(if stderr_str.trim().is_empty() {
            "claude produced no output".to_string()
        } else {
            stderr_str.trim().to_string()
        }));
    }

    let parsed: CliResult = serde_json::from_str(stdout_str.trim())
        .map_err(|e| AiError::InvalidOutput(format!("{e}: {}", truncate(&stdout_str, 300))))?;

    if parsed.is_error || !process_success {
        let msg = parsed
            .error
            .clone()
            .or_else(|| parsed.result.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| truncate(&stderr_str, 500));
        return Err(AiError::ProcessFailed(msg));
    }

    Ok(parsed)
}

#[async_trait]
impl AiProvider for ClaudeCliProvider {
    async fn is_available(&self) -> bool {
        for binary in self.candidate_binaries() {
            if let Ok(output) = Command::new(&binary).arg("--version").output().await {
                if output.status.success() {
                    return true;
                }
            }
        }
        false
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AiError> {
        let result = self
            .run(request.system_prompt.as_deref(), &request.prompt, None, request.effort)
            .await?;
        Ok(CompletionResponse {
            text: result.result.unwrap_or_default(),
            total_cost_usd: result.total_cost_usd,
        })
    }

    async fn extract_structured(&self, request: ExtractionRequest) -> Result<serde_json::Value, AiError> {
        let result = self
            .run(request.system_prompt.as_deref(), &request.prompt, Some(&request.json_schema), None)
            .await?;
        let text = result.result.unwrap_or_default();
        let cleaned = strip_code_fence(&text);
        serde_json::from_str(cleaned).map_err(|e| AiError::InvalidOutput(format!("{e}: {}", truncate(&text, 300))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_code_fence_removes_json_fence() {
        assert_eq!(strip_code_fence("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fence("```\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fence("{\"a\":1}"), "{\"a\":1}");
    }

    #[test]
    fn parses_successful_result() {
        let stdout = br#"{"type":"result","is_error":false,"result":"hello","total_cost_usd":0.002}"#;
        let parsed = parse_cli_output(stdout, b"", true).unwrap();
        assert_eq!(parsed.result.as_deref(), Some("hello"));
        assert_eq!(parsed.total_cost_usd, Some(0.002));
    }

    #[test]
    fn surfaces_is_error_flag_as_process_failure() {
        let stdout = br#"{"type":"result","is_error":true,"result":"budget exceeded"}"#;
        let err = parse_cli_output(stdout, b"", true).unwrap_err();
        assert!(matches!(err, AiError::ProcessFailed(msg) if msg == "budget exceeded"));
    }

    #[test]
    fn empty_stdout_surfaces_stderr() {
        let err = parse_cli_output(b"", b"command not found", false).unwrap_err();
        assert!(matches!(err, AiError::ProcessFailed(msg) if msg == "command not found"));
    }

    #[test]
    fn malformed_json_is_invalid_output_not_a_panic() {
        let err = parse_cli_output(b"not json at all", b"", true).unwrap_err();
        assert!(matches!(err, AiError::InvalidOutput(_)));
    }
}
