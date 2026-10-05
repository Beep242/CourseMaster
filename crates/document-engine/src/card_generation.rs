use academic_core::models::{CardImport, CardKind, ImportSourceKind, ImportStatus};
use academic_core::repo::card_imports::{self, NewCandidate};
use academic_core::SqlitePool;
use ai_engine::{AiProvider, ExtractionRequest};

use crate::error::DocumentError;

/// Bounds one generation's cost and latency. Asking for 200 cards from one
/// paste is a request for a 180-second timeout, not for 200 good cards.
const MAX_CARDS: usize = 40;
const DEFAULT_CARDS: usize = 12;

const SYSTEM_PROMPT: &str = "You write flashcards for a university student revising from their own notes. \
Each card tests ONE idea and has a short, unambiguous answer — a card asking two things at once cannot be \
graded or scheduled. Prefer the student's own wording and notation. Write cards only for material that is \
actually present in the source; do not add facts from your general knowledge, and if the source is too thin \
for the number requested, return fewer good cards rather than padding. Every card must carry a \
`source_excerpt` quoting the passage it came from, verbatim, so the student can check it. Use plain Unicode \
for mathematics (², √, ×, Δ, ≤) rather than LaTeX. Respond with structured JSON only.";

fn generation_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "cards": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "front": { "type": "string", "description": "the prompt; one idea only" },
                        "back": { "type": "string", "description": "a short, unambiguous answer" },
                        "explanation": { "type": "string", "description": "one or two sentences of why, for when the student gets it wrong" },
                        "source_excerpt": { "type": "string", "description": "the verbatim passage from the source this card came from" },
                        "tags": { "type": "array", "items": { "type": "string" }, "description": "1-3 short topic labels" }
                    },
                    "required": ["front", "back", "source_excerpt"]
                }
            }
        },
        "required": ["cards"]
    })
}

fn non_empty(value: Option<&serde_json::Value>) -> Option<String> {
    let text = value?.as_str()?.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// Reads the model's JSON defensively: a card missing a front or a back is
/// dropped rather than failing the whole generation, and a malformed `tags`
/// degrades to none. Same posture as `practice_test::parse_questions` — the
/// student should get the eleven good cards rather than nothing because the
/// twelfth came back malformed.
fn parse_candidates(value: &serde_json::Value) -> Vec<NewCandidate> {
    let Some(items) = value.get("cards").and_then(|c| c.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let front = non_empty(item.get("front"))?;
            let back = non_empty(item.get("back"))?;
            let tags = item.get("tags").and_then(|t| t.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|t| t.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string))
                    .collect::<Vec<String>>()
            });
            Some(NewCandidate {
                // Everything generated here is a basic two-sided card.
                // Multiple-choice distractors are generated later, at the
                // point a card is actually studied that way (increment 18),
                // so a plain flip-through never pays for them.
                kind: CardKind::Basic,
                front,
                back,
                options: None,
                explanation: non_empty(item.get("explanation")),
                tags: tags.filter(|t| !t.is_empty()),
                source_excerpt: non_empty(item.get("source_excerpt")),
            })
        })
        .collect()
}

fn build_prompt(material: &str, requested: usize, deck_name: &str) -> String {
    format!(
        "Write up to {requested} flashcards for the deck \"{deck_name}\" from the material below.\n\n\
         ---MATERIAL---\n{material}"
    )
}

/// Generates cards from pasted material into a deck's review queue.
///
/// Nothing reaches `cards`: candidates land in `card_candidates` as pending and
/// only `card_imports::approve_candidate` promotes one, exactly as
/// `approve_extraction` is the only path from a syllabus extraction to an
/// assignment.
///
/// The import row is written *before* the AI call, so a generation that times
/// out or returns garbage leaves a `failed` row carrying the reason rather than
/// vanishing — and a generation that succeeded is durable the moment it is
/// parsed, so closing the tab mid-review does not throw away work that cost
/// money.
pub async fn generate_cards(
    pool: &SqlitePool,
    ai: &dyn AiProvider,
    deck_id: &str,
    source_label: Option<&str>,
    deck_name: &str,
    material: &str,
    requested: Option<usize>,
) -> Result<CardImport, DocumentError> {
    let requested = requested.unwrap_or(DEFAULT_CARDS).clamp(1, MAX_CARDS);
    let import = card_imports::create(pool, deck_id, ImportSourceKind::Paste, source_label, material).await?;

    let response = ai
        .extract_structured(ExtractionRequest {
            system_prompt: Some(SYSTEM_PROMPT.to_string()),
            prompt: build_prompt(material, requested, deck_name),
            json_schema: generation_schema(),
        })
        .await;

    let response = match response {
        Ok(r) => r,
        Err(err) => {
            // Recorded rather than only returned, so the deck page can show
            // what went wrong instead of an import stuck at "generating".
            card_imports::set_status(pool, &import.id, ImportStatus::Failed, Some(&err.to_string()), None).await?;
            return Err(DocumentError::Ai(err));
        }
    };

    let mut candidates = parse_candidates(&response.value);
    candidates.truncate(requested);
    if candidates.is_empty() {
        let message = "the model returned no usable cards for this material";
        card_imports::set_status(pool, &import.id, ImportStatus::Failed, Some(message), response.total_cost_usd).await?;
        return Err(DocumentError::EmptyGeneration("flashcards".into()));
    }

    card_imports::insert_candidates(pool, &import.id, candidates).await?;
    card_imports::set_status(pool, &import.id, ImportStatus::ReadyForReview, None, response.total_cost_usd).await?;

    card_imports::get(pool, &import.id)
        .await?
        .ok_or_else(|| DocumentError::FeedNotFound(import.id.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_well_formed_cards() {
        let value = serde_json::json!({
            "cards": [
                { "front": "What is a mole?", "back": "6.022e23 particles", "explanation": "Avogadro's number.",
                  "source_excerpt": "A mole is 6.022e23 particles.", "tags": ["stoichiometry", "units"] }
            ]
        });
        let parsed = parse_candidates(&value);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].front, "What is a mole?");
        assert_eq!(parsed[0].explanation.as_deref(), Some("Avogadro's number."));
        assert_eq!(parsed[0].tags.as_ref().unwrap().len(), 2);
        assert_eq!(parsed[0].kind, CardKind::Basic);
    }

    #[test]
    fn missing_cards_array_yields_empty_not_a_panic() {
        assert!(parse_candidates(&serde_json::json!({})).is_empty());
        assert!(parse_candidates(&serde_json::json!({ "cards": "not an array" })).is_empty());
        assert!(parse_candidates(&serde_json::json!(null)).is_empty());
        assert!(parse_candidates(&serde_json::json!([])).is_empty());
    }

    /// One malformed card must not cost the student the good ones.
    #[test]
    fn drops_only_the_unusable_cards() {
        let value = serde_json::json!({
            "cards": [
                { "front": "good", "back": "answer", "source_excerpt": "x" },
                { "back": "no front", "source_excerpt": "x" },
                { "front": "no back", "source_excerpt": "x" },
                { "front": "   ", "back": "blank front", "source_excerpt": "x" },
                { "front": "blank back", "back": "\t\n", "source_excerpt": "x" },
                { "front": "also good", "back": "answer", "source_excerpt": "y" }
            ]
        });
        let parsed = parse_candidates(&value);
        assert_eq!(parsed.iter().map(|c| c.front.as_str()).collect::<Vec<_>>(), vec!["good", "also good"]);
    }

    #[test]
    fn whitespace_is_trimmed_and_optional_fields_degrade_to_none() {
        let value = serde_json::json!({
            "cards": [{ "front": "  padded  ", "back": "  also  ", "explanation": "   ", "tags": [], "source_excerpt": "" }]
        });
        let parsed = parse_candidates(&value);
        assert_eq!(parsed[0].front, "padded");
        assert_eq!(parsed[0].back, "also");
        assert!(parsed[0].explanation.is_none(), "a blank explanation is no explanation");
        assert!(parsed[0].tags.is_none(), "an empty tag list is no tags");
        assert!(parsed[0].source_excerpt.is_none());
    }

    #[test]
    fn malformed_tags_degrade_rather_than_dropping_the_card() {
        let value = serde_json::json!({
            "cards": [
                { "front": "a", "back": "b", "source_excerpt": "x", "tags": "not an array" },
                { "front": "c", "back": "d", "source_excerpt": "x", "tags": [1, 2, "  ", "kept"] }
            ]
        });
        let parsed = parse_candidates(&value);
        assert_eq!(parsed.len(), 2);
        assert!(parsed[0].tags.is_none());
        assert_eq!(parsed[1].tags.as_ref().unwrap(), &vec!["kept".to_string()]);
    }

    #[test]
    fn the_prompt_carries_the_material_and_the_deck_name() {
        let prompt = build_prompt("Avogadro's number is 6.022e23.", 8, "Chapter 3");
        assert!(prompt.contains("up to 8 flashcards"));
        assert!(prompt.contains("Chapter 3"));
        assert!(prompt.contains("6.022e23"));
        assert!(prompt.contains("---MATERIAL---"));
    }

    #[test]
    fn the_schema_requires_provenance_on_every_card() {
        let schema = generation_schema();
        let required = schema["properties"]["cards"]["items"]["required"].as_array().unwrap();
        let required: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(required.contains(&"front"));
        assert!(required.contains(&"back"));
        // Without this the review screen cannot show where a card came from,
        // and nothing downstream can ground an explanation in the source.
        assert!(required.contains(&"source_excerpt"));
    }
}
