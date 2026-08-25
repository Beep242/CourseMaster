use academic_core::models::{PracticeAttempt, PracticeDifficulty, PracticeQuestion, QuestionKind, SubmittedAnswer};
use academic_core::models::PracticeTest;
use academic_core::repo::courses;
use academic_core::repo::practice_tests::{self, NewPracticeQuestion};
use academic_core::SqlitePool;
use ai_engine::{AiProvider, ExtractionRequest};

use crate::error::DocumentError;

const GENERATE_SYSTEM_PROMPT: &str = "You are an expert exam writer for a college course. Generate original, \
high-quality practice questions that test real understanding, not trivia. Every question must have a clear, \
unambiguous correct answer. For multiple_choice questions, provide exactly 4 options containing exactly one correct \
answer, and make the wrong options plausible, not obviously silly. Tag each question with a short `topic` (2-5 \
words) naming the specific concept it tests, so a student can see which topics they're weak on later.";

fn difficulty_instructions(difficulty: PracticeDifficulty) -> &'static str {
    match difficulty {
        PracticeDifficulty::Easy => "Easy difficulty: test recall of basic definitions and straightforward application.",
        PracticeDifficulty::Medium => "Medium difficulty: require applying concepts to a new situation, not just recall.",
        PracticeDifficulty::Hard => "Hard difficulty: multi-step reasoning, edge cases, and synthesis across concepts.",
        PracticeDifficulty::ExamSimulation => {
            "Exam-simulation difficulty: mixed difficulty mirroring a real exam, including at least one short_answer question."
        }
    }
}

fn generation_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "questions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "enum": ["multiple_choice", "true_false", "short_answer"] },
                        "topic": { "type": "string" },
                        "question_text": { "type": "string" },
                        "options": {
                            "type": ["array", "null"],
                            "items": { "type": "string" },
                            "description": "exactly 4 options for multiple_choice; null for every other kind"
                        },
                        "correct_answer": {
                            "type": "string",
                            "description": "the exact correct option text for multiple_choice/true_false, or a model answer for short_answer"
                        },
                        "explanation": { "type": "string" }
                    },
                    "required": ["kind", "topic", "question_text", "correct_answer", "explanation"]
                }
            }
        },
        "required": ["questions"]
    })
}

fn parse_questions(value: &serde_json::Value) -> Vec<NewPracticeQuestion> {
    let Some(items) = value.get("questions").and_then(|v| v.as_array()) else {
        tracing::warn!("practice test generation response had no `questions` array: {value}");
        return Vec::new();
    };

    items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let kind = item.get("kind").and_then(|v| v.as_str()).map(QuestionKind::parse)?;
            let question_text = item.get("question_text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            let correct_answer = item.get("correct_answer").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            if question_text.is_empty() || correct_answer.is_empty() {
                tracing::warn!("skipping practice question missing text or answer: {item}");
                return None;
            }
            let options: Option<Vec<String>> = item
                .get("options")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|o| o.as_str().map(str::to_string)).collect());
            if kind == QuestionKind::MultipleChoice && options.as_ref().map(Vec::len).unwrap_or(0) < 2 {
                tracing::warn!("skipping multiple_choice question with fewer than 2 options: {item}");
                return None;
            }
            Some(NewPracticeQuestion {
                order_index: i as i64,
                kind,
                topic: item.get("topic").and_then(|v| v.as_str()).map(str::to_string),
                question_text,
                options,
                correct_answer,
                explanation: item.get("explanation").and_then(|v| v.as_str()).map(str::to_string),
            })
        })
        .collect()
}

pub async fn generate_practice_test(
    pool: &SqlitePool,
    ai: &dyn AiProvider,
    course_id: &str,
    difficulty: PracticeDifficulty,
    question_count: u32,
    material: Option<&str>,
) -> Result<(PracticeTest, Vec<PracticeQuestion>), DocumentError> {
    let course = courses::get(pool, course_id).await?.ok_or_else(|| DocumentError::CourseNotFound(course_id.to_string()))?;
    let question_count = question_count.clamp(3, 20);

    let mut prompt = format!(
        "Course: {}{}\n\nGenerate exactly {question_count} practice questions. {}",
        course.name,
        course.code.as_ref().map(|c| format!(" ({c})")).unwrap_or_default(),
        difficulty_instructions(difficulty),
    );
    if let Some(material) = material.filter(|m| !m.trim().is_empty()) {
        prompt.push_str("\n\n---STUDENT-PROVIDED MATERIAL TO FOCUS ON---\n");
        prompt.push_str(material);
    }

    let response = ai
        .extract_structured(ExtractionRequest {
            system_prompt: Some(GENERATE_SYSTEM_PROMPT.to_string()),
            prompt,
            json_schema: generation_schema(),
        })
        .await?;

    let items = parse_questions(&response);
    if items.is_empty() {
        return Err(DocumentError::EmptyGeneration("practice test".into()));
    }

    let title = format!("{} — {} practice test", course.name, difficulty.as_str().replace('_', " "));
    let test = practice_tests::create_test(pool, course_id, &title, difficulty).await?;
    practice_tests::insert_questions(pool, &test.id, items).await?;
    let questions = practice_tests::list_questions(pool, &test.id).await?;
    Ok((test, questions))
}

const GRADE_SYSTEM_PROMPT: &str = "You are grading a college student's short-answer response against a model answer. \
Judge whether it demonstrates correct understanding — it does not need to match word-for-word, just be substantively \
correct. Respond with structured JSON only.";

async fn grade_short_answer(ai: &dyn AiProvider, question: &PracticeQuestion, submitted: &str) -> (bool, Option<String>) {
    if submitted.trim().is_empty() {
        return (false, Some("No answer submitted.".to_string()));
    }
    let prompt = format!(
        "QUESTION: {}\n\nMODEL ANSWER: {}\n\nSTUDENT'S ANSWER: {}\n\nIs the student's answer substantively correct?",
        question.question_text, question.correct_answer, submitted
    );
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "is_correct": { "type": "boolean" },
            "feedback": { "type": "string", "description": "one or two sentences of specific, encouraging feedback" }
        },
        "required": ["is_correct", "feedback"]
    });
    match ai
        .extract_structured(ExtractionRequest { system_prompt: Some(GRADE_SYSTEM_PROMPT.to_string()), prompt, json_schema: schema })
        .await
    {
        Ok(value) => {
            let is_correct = value.get("is_correct").and_then(|v| v.as_bool()).unwrap_or(false);
            let feedback = value.get("feedback").and_then(|v| v.as_str()).map(str::to_string);
            (is_correct, feedback)
        }
        Err(err) => {
            tracing::warn!("short-answer grading failed: {err}");
            (false, Some("Couldn't grade this automatically — review it yourself against the explanation.".to_string()))
        }
    }
}

/// Grades multiple_choice/true_false by exact (case-insensitive) match
/// against the stored correct answer — no AI call needed, no ambiguity to
/// judge. short_answer genuinely needs judgment, so those go through the
/// model one at a time. "Teaching, not just marking wrong" (per spec) means
/// every question carries feedback either way, not just a checkmark.
pub async fn grade_attempt(
    pool: &SqlitePool,
    ai: &dyn AiProvider,
    test_id: &str,
    answers: Vec<SubmittedAnswer>,
) -> Result<PracticeAttempt, DocumentError> {
    let questions = practice_tests::list_questions(pool, test_id).await?;
    if questions.is_empty() {
        return Err(DocumentError::CourseNotFound(format!("no questions found for test {test_id}")));
    }

    let mut graded = Vec::with_capacity(questions.len());
    let mut correct_count = 0usize;

    for question in &questions {
        let submitted = answers.iter().find(|a| a.question_id == question.id).map(|a| a.response.clone()).unwrap_or_default();
        let (is_correct, feedback) = match question.kind {
            QuestionKind::MultipleChoice | QuestionKind::TrueFalse => {
                (submitted.trim().eq_ignore_ascii_case(question.correct_answer.trim()), question.explanation.clone())
            }
            QuestionKind::ShortAnswer => grade_short_answer(ai, question, &submitted).await,
        };
        if is_correct {
            correct_count += 1;
        }
        graded.push(serde_json::json!({
            "question_id": question.id,
            "question_text": question.question_text,
            "kind": question.kind.as_str(),
            "topic": question.topic,
            "submitted": submitted,
            "correct_answer": question.correct_answer,
            "is_correct": is_correct,
            "feedback": feedback,
        }));
    }

    let score_percentage = (correct_count as f64 / questions.len() as f64) * 100.0;
    let answers_value = serde_json::Value::Array(graded);
    Ok(practice_tests::record_attempt(pool, test_id, score_percentage, &answers_value).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_well_formed_questions() {
        let value = serde_json::json!({
            "questions": [
                {"kind": "multiple_choice", "topic": "Ohm's law", "question_text": "V = ?", "options": ["IR", "I/R", "R/I", "I+R"], "correct_answer": "IR", "explanation": "V=IR"}
            ]
        });
        let items = parse_questions(&value);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, QuestionKind::MultipleChoice);
    }

    #[test]
    fn drops_multiple_choice_with_too_few_options() {
        let value = serde_json::json!({
            "questions": [
                {"kind": "multiple_choice", "topic": "x", "question_text": "q", "options": ["only one"], "correct_answer": "only one", "explanation": "e"}
            ]
        });
        assert!(parse_questions(&value).is_empty());
    }

    #[test]
    fn drops_questions_missing_required_fields() {
        let value = serde_json::json!({ "questions": [{"kind": "short_answer", "topic": "x"}] });
        assert!(parse_questions(&value).is_empty());
    }

    #[test]
    fn missing_questions_array_yields_empty_not_a_panic() {
        assert!(parse_questions(&serde_json::json!({ "unexpected": true })).is_empty());
    }
}
