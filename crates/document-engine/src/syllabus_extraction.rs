use academic_core::models::{AssignmentKind, SyllabusStatus};
use academic_core::repo::syllabus::{self, NewExtraction};
use academic_core::SqlitePool;
use ai_engine::{AiProvider, ExtractionRequest};

use crate::error::DocumentError;

const SYSTEM_PROMPT: &str = "You are a precise academic syllabus analyst. You extract only concrete, \
gradeable deliverables (assignments, exams, quizzes, projects) that are explicitly present in the \
provided text. Never invent a title or a date that isn't supported by the text. If a due date is not \
stated or cannot be confidently inferred, set due_date to null rather than guessing. Every item's \
source_excerpt must be a verbatim substring of the input text — never paraphrase it. Set confidence \
lower (below 0.6) when the date or title is ambiguous, implied, or spread across multiple sentences, \
and higher (above 0.85) only when both the title and date are stated plainly and unambiguously.";

const USER_PREAMBLE: &str = "Extract every assignment, exam, quiz, and project deliverable from the \
syllabus text below. Respond with structured JSON matching the provided schema only — no other prose.";

fn extraction_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "enum": ["assignment", "exam", "quiz", "project"] },
                        "title": { "type": "string" },
                        "description": { "type": ["string", "null"] },
                        "due_date": {
                            "type": ["string", "null"],
                            "description": "ISO 8601 date YYYY-MM-DD if explicitly stated or confidently inferable, else null"
                        },
                        "due_time": { "type": ["string", "null"], "description": "24-hour HH:MM if explicitly stated, else null" },
                        "source_excerpt": { "type": "string", "description": "verbatim text this item was extracted from" },
                        "confidence": { "type": "number", "minimum": 0, "maximum": 1 }
                    },
                    "required": ["kind", "title", "source_excerpt", "confidence"]
                }
            }
        },
        "required": ["items"]
    })
}

fn parse_extraction_response(value: &serde_json::Value) -> Vec<NewExtraction> {
    let Some(items) = value.get("items").and_then(|v| v.as_array()) else {
        tracing::warn!("syllabus extraction response had no `items` array: {value}");
        return Vec::new();
    };

    items
        .iter()
        .filter_map(|item| {
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").trim();
            let source_excerpt = item.get("source_excerpt").and_then(|v| v.as_str()).unwrap_or("").trim();
            if title.is_empty() || source_excerpt.is_empty() {
                tracing::warn!("skipping extraction item missing title or source_excerpt: {item}");
                return None;
            }
            let kind = item
                .get("kind")
                .and_then(|v| v.as_str())
                .map(AssignmentKind::parse)
                .unwrap_or(AssignmentKind::Assignment);
            let confidence = item.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.5).clamp(0.0, 1.0);
            let description = item.get("description").and_then(|v| v.as_str()).map(str::to_string);
            let due_date = item.get("due_date").and_then(|v| v.as_str()).map(str::to_string);
            let due_time = item.get("due_time").and_then(|v| v.as_str()).map(str::to_string);

            Some(NewExtraction {
                kind,
                title: title.to_string(),
                description,
                due_date,
                due_time,
                source_excerpt: source_excerpt.to_string(),
                confidence,
                course_id: None,
                external_uid: None,
                external_org_unit_id: None,
            })
        })
        .collect()
}

/// Runs AI extraction over a previously-submitted syllabus and stores the
/// results as `syllabus_extractions` rows awaiting human review. Never
/// touches the `assignments` table directly — see
/// `academic_core::repo::syllabus::approve_extraction` for the only path
/// that turns a reviewed extraction into a real, schedulable assignment.
pub async fn process_syllabus(pool: &SqlitePool, ai: &dyn AiProvider, syllabus_id: &str) -> Result<usize, DocumentError> {
    let record = syllabus::get_syllabus(pool, syllabus_id)
        .await?
        .ok_or_else(|| DocumentError::SyllabusNotFound(syllabus_id.to_string()))?;

    let request = ExtractionRequest {
        system_prompt: Some(SYSTEM_PROMPT.to_string()),
        prompt: format!("{USER_PREAMBLE}\n\n---SYLLABUS TEXT---\n{}", record.raw_text),
        json_schema: extraction_schema(),
    };

    let response = match ai.extract_structured(request).await {
        Ok(value) => value,
        Err(err) => {
            syllabus::set_status(pool, syllabus_id, SyllabusStatus::Failed, Some(&err.to_string())).await?;
            return Err(DocumentError::Ai(err));
        }
    };

    let items = parse_extraction_response(&response);
    let count = items.len();
    if count > 0 {
        syllabus::insert_extractions(pool, syllabus_id, items).await?;
    }
    syllabus::set_status(pool, syllabus_id, SyllabusStatus::ReadyForReview, None).await?;
    Ok(count)
}

const ASK_SYSTEM_PROMPT: &str = "You answer questions about a college course using ONLY the syllabus \
text provided below. Quote or closely paraphrase the exact relevant sentence in your answer so the \
student can see where it came from. If the syllabus does not address the question, say plainly \
\"The syllabus doesn't specify this\" — never guess or fabricate a policy, date, or number that isn't \
in the text.";

/// "Ask My Syllabus" — grounds every answer in the syllabus text itself
/// rather than the model's general knowledge, since a wrong guess about a
/// grading policy or deadline is worse than no answer.
pub async fn ask_syllabus(ai: &dyn AiProvider, raw_text: &str, question: &str) -> Result<String, DocumentError> {
    let request = ai_engine::CompletionRequest {
        system_prompt: Some(ASK_SYSTEM_PROMPT.to_string()),
        prompt: format!("SYLLABUS TEXT:\n{raw_text}\n\nQUESTION: {question}"),
        effort: None,
    };
    let response = ai.complete(request).await?;
    Ok(response.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_well_formed_items() {
        let value = serde_json::json!({
            "items": [
                {"kind": "exam", "title": "Midterm", "source_excerpt": "Midterm exam on Oct 12", "due_date": "2026-10-12", "confidence": 0.95}
            ]
        });
        let items = parse_extraction_response(&value);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, AssignmentKind::Exam);
        assert_eq!(items[0].due_date.as_deref(), Some("2026-10-12"));
    }

    #[test]
    fn drops_items_missing_required_fields() {
        let value = serde_json::json!({ "items": [{"kind": "assignment", "confidence": 0.5}] });
        assert!(parse_extraction_response(&value).is_empty());
    }

    #[test]
    fn clamps_out_of_range_confidence() {
        let value = serde_json::json!({
            "items": [{"kind": "quiz", "title": "Quiz 1", "source_excerpt": "quiz 1", "confidence": 4.2}]
        });
        let items = parse_extraction_response(&value);
        assert_eq!(items[0].confidence, 1.0);
    }

    #[test]
    fn missing_items_array_yields_empty_not_a_panic() {
        let value = serde_json::json!({ "unexpected": true });
        assert!(parse_extraction_response(&value).is_empty());
    }
}
