//! Multiple-choice distractors.
//!
//! Generated **once, for a whole deck, ahead of time** — never during a review.
//! A review that waited on `claude -p` would cost money per card and stall for
//! seconds in the middle of a study session; options live in
//! `cards.options_json` so answering one is a pure comparison.
//!
//! One call covers the whole deck rather than one call per card: the per-call
//! overhead dominates at this size, and a 30-card deck would otherwise be 30
//! subprocess spawns against a $0.50-per-call cap.

use academic_core::models::{Card, CardKind, CardUpdate};
use academic_core::repo::cards;
use academic_core::SqlitePool;
use ai_engine::{AiProvider, ExtractionRequest};

use crate::error::DocumentError;

/// Cards per AI call. Keeps one request inside the input guard and the budget
/// cap even for a large deck, at the cost of a few more calls.
const BATCH_SIZE: usize = 25;
const DISTRACTORS_PER_CARD: usize = 3;

const SYSTEM_PROMPT: &str = "You write multiple-choice distractors for a university student's flashcards. For each \
card you are given the question and its CORRECT answer. Return exactly three WRONG answers that are plausible to \
someone who has studied but not mastered the material: the same kind of thing as the right answer, similar in form \
and length, and wrong for a specific reason — a common confusion, an off-by-one, a swapped term. Never return an \
answer that is also correct, never a joke option, and never a restatement of the correct answer. Respond with \
structured JSON only.";

fn schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "cards": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "the card id exactly as given" },
                        "distractors": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "exactly three plausible wrong answers"
                        }
                    },
                    "required": ["id", "distractors"]
                }
            }
        },
        "required": ["cards"]
    })
}

fn build_prompt(batch: &[&Card]) -> String {
    let mut prompt = String::from("Write three wrong answers for each card.\n\n");
    for card in batch {
        prompt.push_str(&format!("---\nid: {}\nquestion: {}\ncorrect answer: {}\n", card.id, card.front, card.back));
    }
    prompt
}

/// Parses the reply, keeping only distractors that are actually usable.
///
/// Drops any option equal to the correct answer (the model occasionally
/// restates it, which would make the question unanswerable) and any duplicate,
/// comparing with `grading::normalize` so "Mitochondria." does not sneak past
/// "mitochondria".
fn parse_distractors(value: &serde_json::Value, card: &Card) -> Option<Vec<String>> {
    let items = value.get("cards")?.as_array()?;
    let entry = items.iter().find(|i| i.get("id").and_then(|v| v.as_str()) == Some(card.id.as_str()))?;
    let raw = entry.get("distractors")?.as_array()?;

    let correct = grading::normalize(&card.back);
    let mut seen = vec![correct.clone()];
    let mut out = Vec::new();
    for option in raw {
        let Some(text) = option.as_str().map(str::trim).filter(|s| !s.is_empty()) else { continue };
        let key = grading::normalize(text);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(text.to_string());
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

#[derive(Debug, Default)]
pub struct DistractorReport {
    pub considered: usize,
    pub prepared: usize,
    pub skipped: usize,
    pub total_cost_usd: Option<f64>,
}

/// Fills in multiple-choice options for every card in a deck that lacks them.
///
/// Cards that already have options are left alone, so running this twice is
/// cheap and does not rewrite work the student may have edited.
pub async fn prepare_deck(
    pool: &SqlitePool,
    ai: &dyn AiProvider,
    deck_id: &str,
) -> Result<DistractorReport, DocumentError> {
    let all = cards::list_by_deck(pool, deck_id).await?;
    let pending: Vec<&Card> = all
        .iter()
        .filter(|c| c.options.as_ref().is_none_or(|o| o.len() < 2))
        // A true/false card's options are implicit, and a card with a very long
        // answer makes a terrible multiple-choice question.
        .filter(|c| c.kind != CardKind::TrueFalse && c.back.chars().count() <= 160)
        .collect();

    let mut report = DistractorReport { considered: pending.len(), ..Default::default() };
    if pending.is_empty() {
        return Ok(report);
    }

    let mut cost = 0.0f64;
    let mut saw_cost = false;

    for batch in pending.chunks(BATCH_SIZE) {
        let response = ai
            .extract_structured(ExtractionRequest {
                system_prompt: Some(SYSTEM_PROMPT.to_string()),
                prompt: build_prompt(batch),
                json_schema: schema(),
            })
            .await?;
        if let Some(c) = response.total_cost_usd {
            cost += c;
            saw_cost = true;
        }

        for card in batch {
            match parse_distractors(&response.value, card) {
                Some(distractors) => {
                    // The correct answer first; the UI shuffles for display, so
                    // storage order carries no information a student could use.
                    let mut options = vec![card.back.clone()];
                    options.extend(distractors.into_iter().take(DISTRACTORS_PER_CARD));
                    cards::update(
                        pool,
                        &card.id,
                        CardUpdate { options: Some(Some(options)), kind: Some(CardKind::MultipleChoice), ..Default::default() },
                    )
                    .await?;
                    report.prepared += 1;
                }
                // A card the model returned nothing usable for stays a plain
                // card rather than becoming a broken question.
                None => report.skipped += 1,
            }
        }
    }

    report.total_cost_usd = saw_cost.then_some(cost);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, back: &str) -> Card {
        Card {
            id: id.into(),
            deck_id: "d".into(),
            order_index: 0,
            kind: CardKind::Basic,
            front: "q".into(),
            back: back.into(),
            options: None,
            explanation: None,
            tags: vec![],
            source_excerpt: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn parses_distractors_for_the_matching_card() {
        let value = serde_json::json!({ "cards": [
            { "id": "a", "distractors": ["nucleus", "ribosome", "golgi body"] },
            { "id": "b", "distractors": ["x", "y", "z"] }
        ]});
        let parsed = parse_distractors(&value, &card("a", "mitochondria")).unwrap();
        assert_eq!(parsed, vec!["nucleus", "ribosome", "golgi body"]);
    }

    /// The model occasionally restates the correct answer, which would make the
    /// question unanswerable — two right options and no way to pick.
    #[test]
    fn drops_a_distractor_equal_to_the_correct_answer() {
        let value = serde_json::json!({ "cards": [
            { "id": "a", "distractors": ["Mitochondria.", "nucleus", "ribosome"] }
        ]});
        let parsed = parse_distractors(&value, &card("a", "mitochondria")).unwrap();
        // Matched through normalisation, so case and punctuation do not hide it.
        assert_eq!(parsed, vec!["nucleus", "ribosome"]);
    }

    #[test]
    fn drops_duplicate_distractors() {
        let value = serde_json::json!({ "cards": [
            { "id": "a", "distractors": ["nucleus", "NUCLEUS", "  nucleus  ", "ribosome"] }
        ]});
        assert_eq!(parse_distractors(&value, &card("a", "mito")).unwrap(), vec!["nucleus", "ribosome"]);
    }

    #[test]
    fn a_card_with_no_usable_distractors_yields_none() {
        let value = serde_json::json!({ "cards": [{ "id": "a", "distractors": ["mitochondria", "  ", ""] }] });
        assert!(parse_distractors(&value, &card("a", "mitochondria")).is_none());
        // And a reply that omits the card entirely.
        assert!(parse_distractors(&serde_json::json!({ "cards": [] }), &card("a", "x")).is_none());
    }

    #[test]
    fn malformed_replies_yield_none_rather_than_panicking() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({ "cards": "nope" }),
            serde_json::json!(null),
            serde_json::json!({ "cards": [{ "id": "a" }] }),
            serde_json::json!({ "cards": [{ "id": "a", "distractors": "not an array" }] }),
            serde_json::json!({ "cards": [{ "distractors": ["x"] }] }),
        ] {
            assert!(parse_distractors(&value, &card("a", "x")).is_none());
        }
    }

    #[test]
    fn the_prompt_carries_every_card_in_the_batch() {
        let a = card("id-1", "answer one");
        let b = card("id-2", "answer two");
        let prompt = build_prompt(&[&a, &b]);
        assert!(prompt.contains("id: id-1"));
        assert!(prompt.contains("correct answer: answer one"));
        assert!(prompt.contains("id: id-2"));
        assert!(prompt.contains("answer two"));
    }

    #[test]
    fn the_schema_requires_an_id_so_replies_can_be_matched_back() {
        let s = schema();
        let required = s["properties"]["cards"]["items"]["required"].as_array().unwrap();
        let required: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(required.contains(&"id"));
        assert!(required.contains(&"distractors"));
    }
}
