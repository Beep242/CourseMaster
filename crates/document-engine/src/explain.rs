//! "Explain why I got this wrong."
//!
//! Grounded in the card's own `source_excerpt` — the passage the card was
//! generated from — rather than in the model's general knowledge. That is what
//! `card_generation` requires a `source_excerpt` on every card for: an
//! explanation that quietly contradicts the student's own lecture notes is
//! worse than none, because they will be examined on the notes.
//!
//! There is no vector store and no embedding model in this stack, and none is
//! needed: the relevant passage was already captured when the card was made, so
//! "retrieval" here is a column read.

use academic_core::models::CardExplanation;
use academic_core::repo::{cards, explanations};
use academic_core::SqlitePool;
use ai_engine::{AiProvider, CompletionRequest};

use crate::error::DocumentError;

const SYSTEM_PROMPT: &str = "You are tutoring a university student who has just answered a flashcard incorrectly. \
Explain, in two or three sentences, what the right answer means and why theirs is not it — name the specific \
confusion rather than restating the answer. Address the student directly and stay encouraging; never be sarcastic \
about a wrong answer. If source material is provided, ground your explanation in it and do not contradict it, even \
if you would phrase the idea differently. Plain prose, no markdown headings, no preamble.";

fn build_prompt(front: &str, correct: &str, submitted: &str, source: Option<&str>) -> String {
    let mut prompt = format!("QUESTION: {front}\n\nCORRECT ANSWER: {correct}\n\n");
    if submitted.trim().is_empty() {
        prompt.push_str("THE STUDENT DID NOT ANSWER. Explain what the answer means and how to remember it.\n\n");
    } else {
        prompt.push_str(&format!("THE STUDENT ANSWERED: {submitted}\n\n"));
    }
    if let Some(source) = source.map(str::trim).filter(|s| !s.is_empty()) {
        prompt.push_str(&format!("SOURCE MATERIAL (from the student's own notes — stay consistent with this):\n{source}\n"));
    }
    prompt
}

/// Returns an explanation, from cache when this exact mistake has been seen
/// before.
///
/// The cache key is `grading::normalize` of the submitted answer, so
/// "Mitochondria" and "mitochondria." are one misunderstanding rather than two
/// paid calls — the same folding the grader uses to decide the answer was wrong
/// in the first place.
pub async fn explain_mistake(
    pool: &SqlitePool,
    ai: &dyn AiProvider,
    card_id: &str,
    submitted: &str,
) -> Result<CardExplanation, DocumentError> {
    let card = cards::get(pool, card_id)
        .await?
        .ok_or_else(|| DocumentError::CourseNotFound(format!("no card {card_id}")))?;

    let key = grading::normalize(submitted);
    if let Some(cached) = explanations::find(pool, card_id, &key).await? {
        return Ok(cached);
    }

    let response = ai
        .complete(CompletionRequest {
            system_prompt: Some(SYSTEM_PROMPT.to_string()),
            prompt: build_prompt(&card.front, &card.back, submitted, card.source_excerpt.as_deref()),
            effort: None,
        })
        .await?;

    let text = response.text.trim();
    if text.is_empty() {
        return Err(DocumentError::EmptyGeneration("explanation".into()));
    }

    Ok(explanations::store(pool, card_id, &key, submitted, text, response.total_cost_usd).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_carries_the_question_the_answer_and_the_mistake() {
        let prompt = build_prompt("What is the powerhouse?", "mitochondria", "nucleus", None);
        assert!(prompt.contains("QUESTION: What is the powerhouse?"));
        assert!(prompt.contains("CORRECT ANSWER: mitochondria"));
        assert!(prompt.contains("THE STUDENT ANSWERED: nucleus"));
        assert!(!prompt.contains("SOURCE MATERIAL"));
    }

    /// The whole point of requiring a source excerpt at generation time.
    #[test]
    fn the_prompt_grounds_itself_in_the_students_own_material() {
        let prompt = build_prompt("q", "a", "b", Some("A mole is 6.022e23 particles."));
        assert!(prompt.contains("SOURCE MATERIAL"));
        assert!(prompt.contains("6.022e23"));
        assert!(prompt.contains("stay consistent with this"));
    }

    /// Not answering is a different situation from answering wrongly, and
    /// "why is your answer wrong" reads badly when there was no answer.
    #[test]
    fn a_blank_answer_asks_a_different_question() {
        let prompt = build_prompt("q", "a", "   ", None);
        assert!(prompt.contains("DID NOT ANSWER"));
        assert!(!prompt.contains("THE STUDENT ANSWERED"));
    }

    #[test]
    fn a_blank_source_excerpt_is_omitted_rather_than_sent_empty() {
        let prompt = build_prompt("q", "a", "b", Some("   "));
        assert!(!prompt.contains("SOURCE MATERIAL"));
    }

    /// The cache key must collapse the same misunderstanding written different
    /// ways, or a repeat mistake pays twice.
    #[test]
    fn the_cache_key_folds_case_and_punctuation() {
        assert_eq!(grading::normalize("Nucleus!"), grading::normalize("  nucleus  "));
        assert_eq!(grading::normalize(""), grading::normalize("   "));
    }
}
