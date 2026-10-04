use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::timeout;

use crate::error::AiError;
use crate::provider::{AiProvider, CompletionRequest, CompletionResponse, Effort, ExtractionRequest, ExtractionResponse};

const DEFAULT_TIMEOUT_SECS: u64 = 180;
const DEFAULT_MAX_BUDGET_USD: f64 = 0.50;
/// Roughly 60k tokens of prompt — generous for a lecture handout or a pasted
/// chapter, and small enough that one call cannot run away with the budget or
/// the clock. Enforced *before* spawning, so an oversized input fails fast
/// with a message that says what to do instead of timing out after 180s.
const DEFAULT_MAX_INPUT_CHARS: usize = 240_000;

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
    max_input_chars: usize,
    working_dir: PathBuf,
}

impl Default for ClaudeCliProvider {
    fn default() -> Self {
        Self {
            binary: "claude".into(),
            model: None,
            max_budget_usd: Some(DEFAULT_MAX_BUDGET_USD),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_input_chars: DEFAULT_MAX_INPUT_CHARS,
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

    /// Wall-clock ceiling for one call, covering writing the prompt as well as
    /// waiting for the answer. A zero is ignored rather than making every call
    /// fail instantly, so a misconfigured env var degrades to the default.
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        if secs > 0 {
            self.timeout_secs = secs;
        }
        self
    }

    /// Spend ceiling handed to the CLI's own `--max-budget-usd`. `None`
    /// removes the flag entirely, which means *no* cap — only do that
    /// deliberately.
    pub fn with_max_budget_usd(mut self, budget: Option<f64>) -> Self {
        self.max_budget_usd = budget.filter(|b| *b > 0.0);
        self
    }

    /// Largest prompt (after the system prompt and schema are folded in) this
    /// provider will even try to send. See `DEFAULT_MAX_INPUT_CHARS`.
    pub fn with_max_input_chars(mut self, chars: usize) -> Self {
        if chars > 0 {
            self.max_input_chars = chars;
        }
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

        // Checked on the composed prompt, not just the caller's text, since the
        // system prompt and a JSON Schema both count against the same budget.
        // Rejecting here costs nothing; letting it through costs up to the full
        // timeout and whatever the call spends before giving up.
        let chars = stdin_prompt.chars().count();
        if chars > self.max_input_chars {
            return Err(AiError::InputTooLarge { chars, max: self.max_input_chars });
        }

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

        let stdin = child.stdin.take();
        let mut stdout = child.stdout.take().ok_or(AiError::ProviderUnavailable)?;
        let mut stderr = child.stderr.take().ok_or(AiError::ProviderUnavailable)?;

        // Writing the prompt, draining both output pipes, and waiting for exit
        // all happen concurrently and all inside one timeout. Doing the write
        // first and *then* waiting — as this used to — deadlocks on a large
        // prompt: the write blocks once the OS pipe buffer (64 KiB on Linux)
        // fills while nothing is reading the child's stdout, and because that
        // write sat outside the timeout there was no upper bound on the hang.
        // It is exactly the document-import path that produces prompts that big.
        let settled = {
            let run = async {
                let write = async {
                    if let Some(mut stdin) = stdin {
                        stdin.write_all(stdin_prompt.as_bytes()).await?;
                        // Closing the pipe is what signals end-of-prompt;
                        // without it `claude` waits for more input forever.
                        stdin.shutdown().await?;
                    }
                    Ok::<(), std::io::Error>(())
                };
                let mut out_buf = Vec::new();
                let mut err_buf = Vec::new();
                let (wrote, read_out, read_err) =
                    tokio::join!(write, stdout.read_to_end(&mut out_buf), stderr.read_to_end(&mut err_buf));
                wrote?;
                read_out?;
                read_err?;
                let status = child.wait().await?;
                Ok::<_, std::io::Error>((out_buf, err_buf, status))
            };
            timeout(Duration::from_secs(self.timeout_secs), run).await
        };

        match settled {
            Ok(Ok((out_buf, err_buf, status))) => parse_cli_output(&out_buf, &err_buf, status.success()),
            Ok(Err(e)) => Err(AiError::Io(e)),
            Err(_elapsed) => {
                // Without this the abandoned `claude` process keeps running,
                // still spending against the budget nobody is waiting on.
                let _ = child.kill().await;
                Err(AiError::Timeout(self.timeout_secs))
            }
        }
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

/// Shortens `s` to at most `max` *bytes*, stepping back to the nearest
/// character boundary first. Slicing a `&str` at a byte index that lands
/// inside a multi-byte character panics — and this is only ever called on the
/// malformed-output path, where the bytes are most likely to be exactly the
/// kind of content that breaks it: maths symbols, Greek letters, em-dashes.
/// A panic there is not a contained failure: the release profile sets
/// `panic = "abort"`, `tower-http` is built without `catch-panic` so no
/// `CatchPanicLayer` is possible, and the same binary serves the frontend —
/// so one odd model response would take the entire site down.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
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

    async fn extract_structured(&self, request: ExtractionRequest) -> Result<ExtractionResponse, AiError> {
        let result = self
            .run(request.system_prompt.as_deref(), &request.prompt, Some(&request.json_schema), None)
            .await?;
        let total_cost_usd = result.total_cost_usd;
        let text = result.result.unwrap_or_default();
        let cleaned = strip_code_fence(&text);
        let value =
            serde_json::from_str(cleaned).map_err(|e| AiError::InvalidOutput(format!("{e}: {}", truncate(&text, 300))))?;
        Ok(ExtractionResponse { value, total_cost_usd })
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

    #[test]
    fn truncate_leaves_short_strings_alone() {
        assert_eq!(truncate("short", 300), "short");
        // Exactly at the limit is not over it.
        assert_eq!(truncate("abc", 3), "abc");
    }

    /// The regression: `&s[..max]` panicked whenever the byte at `max` landed
    /// inside a multi-byte character — and because the release profile aborts
    /// on panic and this binary also serves the frontend, that took the whole
    /// site down. `max` is a *byte* budget, so the boundaries below follow
    /// UTF-8 widths: `Ω` is 2 bytes, `🎓` is 4.
    #[test]
    fn truncate_never_splits_a_multibyte_character() {
        assert_eq!(truncate("Ωmega", 1), "…"); // mid-Ω, steps back to nothing
        assert_eq!(truncate("Ωmega", 2), "Ω…"); // exactly after Ω
        assert_eq!(truncate("Ωmega", 3), "Ωm…");
        assert_eq!(truncate("🎓graduate", 3), "…"); // mid-emoji
        assert_eq!(truncate("🎓graduate", 4), "🎓…"); // exactly after it
        for limit in 0..60 {
            // The real contract: never panic, whatever the limit, and never
            // return something that is not valid UTF-8.
            let out = truncate("∫ƒ(x)dx = Ω · 5 — naïve café 🎓", limit);
            assert!(out.is_char_boundary(out.len()));
        }
    }

    /// Real model output that fails to parse: maths symbols are exactly the
    /// content most likely to be in a malformed response, which is the only
    /// path that calls `truncate`.
    #[test]
    fn invalid_output_error_survives_multibyte_model_output() {
        let long_unicode = "Ω".repeat(400);
        let err = parse_cli_output(long_unicode.as_bytes(), b"", true).unwrap_err();
        assert!(matches!(err, AiError::InvalidOutput(_)));
    }

    #[tokio::test]
    async fn oversized_input_is_rejected_before_spawning() {
        // Deliberately does not need `claude` on PATH: the guard runs before
        // the spawn, so this asserts the cheap failure actually is cheap.
        let provider = ClaudeCliProvider::new().with_max_input_chars(50);
        let err = provider
            .complete(CompletionRequest { system_prompt: None, prompt: "x".repeat(500), effort: None })
            .await
            .unwrap_err();
        match err {
            AiError::InputTooLarge { chars, max } => {
                assert_eq!(chars, 500);
                assert_eq!(max, 50);
            }
            other => panic!("expected InputTooLarge, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_size_guard_counts_the_system_prompt_and_schema_too() {
        let provider = ClaudeCliProvider::new().with_max_input_chars(120);
        let err = provider
            .extract_structured(ExtractionRequest {
                system_prompt: Some("a".repeat(100)),
                prompt: "b".repeat(100),
                json_schema: serde_json::json!({"type": "object"}),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AiError::InputTooLarge { .. }));
    }

    #[test]
    fn nonsense_knob_values_fall_back_to_the_defaults() {
        // A misconfigured env var should degrade to the default rather than
        // making every AI call fail instantly or run uncapped.
        let p = ClaudeCliProvider::new().with_timeout_secs(0).with_max_input_chars(0);
        assert_eq!(p.timeout_secs, DEFAULT_TIMEOUT_SECS);
        assert_eq!(p.max_input_chars, DEFAULT_MAX_INPUT_CHARS);
        assert_eq!(ClaudeCliProvider::new().with_max_budget_usd(Some(-1.0)).max_budget_usd, None);
        assert_eq!(ClaudeCliProvider::new().with_timeout_secs(30).timeout_secs, 30);
        assert_eq!(ClaudeCliProvider::new().with_max_budget_usd(Some(2.0)).max_budget_usd, Some(2.0));
    }

    #[test]
    fn cost_is_carried_out_of_a_successful_call() {
        let stdout = br#"{"type":"result","is_error":false,"result":"{\"a\":1}","total_cost_usd":0.0042}"#;
        let parsed = parse_cli_output(stdout, b"", true).unwrap();
        assert_eq!(parsed.total_cost_usd, Some(0.0042));
    }
}
