use academic_core::models::{CardReview, DueCard, ReviewOutcome};
use academic_core::repo::reviews::{self, QueueLimits, DEFAULT_NEW_PER_DAY, DEFAULT_REVIEWS_PER_DAY};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::NaiveDate;
use serde::Deserialize;
use srs::Rating;

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::state::AppState;

/// The client's own calendar date, because the server has none that means
/// anything to the student: it runs in UTC, so an 11pm review in Eastern time
/// is already tomorrow by the server's clock. "Due today" and, later, a daily
/// streak are local-day concepts.
///
/// A missing or unparseable value falls back to the server's UTC date rather
/// than failing the review — losing a day boundary is better than losing the
/// answer.
fn resolve_today(supplied: Option<&str>) -> NaiveDate {
    supplied
        .and_then(|s| NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
        .unwrap_or_else(|| chrono::Utc::now().date_naive())
}

#[derive(Debug, Deserialize)]
pub struct QueueQuery {
    pub course_id: Option<String>,
    pub local_date: Option<String>,
    pub new_per_day: Option<i64>,
    pub reviews_per_day: Option<i64>,
}

/// Everything due across every deck and course — the cross-course entry point
/// that makes this a study system rather than a per-deck toy.
pub async fn due_queue(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(query): Query<QueueQuery>,
) -> Result<Json<Vec<DueCard>>, ApiError> {
    let today = resolve_today(query.local_date.as_deref());
    let limits = QueueLimits {
        // Clamped rather than trusted: a client asking for 10,000 new cards is
        // asking for an unusable session, and a negative is nonsense.
        new_per_day: query.new_per_day.unwrap_or(DEFAULT_NEW_PER_DAY).clamp(0, 500),
        reviews_per_day: query.reviews_per_day.unwrap_or(DEFAULT_REVIEWS_PER_DAY).clamp(0, 2000),
        course_id: query.course_id.filter(|c| !c.is_empty()),
    };
    Ok(Json(reviews::due_queue(&state.pool, today, limits).await?))
}

#[derive(Debug, Deserialize)]
pub struct ReviewBody {
    pub rating: String,
    #[serde(default)]
    pub local_date: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<i64>,
}

pub async fn submit_review(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(card_id): Path<String>,
    Json(body): Json<ReviewBody>,
) -> Result<Json<ReviewOutcome>, ApiError> {
    // `Rating::parse` fails toward `Again`, which shows the card again sooner
    // rather than hiding it for months — the safe direction for bad input.
    let rating = Rating::parse(body.rating.trim());
    let today = resolve_today(body.local_date.as_deref());
    let local_date = today.format("%Y-%m-%d").to_string();
    Ok(Json(reviews::record_review(&state.pool, &card_id, rating, today, &local_date, body.duration_ms).await?))
}

#[derive(Debug, Deserialize)]
pub struct SuspendBody {
    pub suspended: bool,
    #[serde(default)]
    pub local_date: Option<String>,
}

pub async fn set_suspended(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(card_id): Path<String>,
    Json(body): Json<SuspendBody>,
) -> Result<(), ApiError> {
    let today = resolve_today(body.local_date.as_deref());
    Ok(reviews::set_suspended(&state.pool, &card_id, body.suspended, today).await?)
}

pub async fn card_history(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(card_id): Path<String>,
) -> Result<Json<Vec<CardReview>>, ApiError> {
    Ok(Json(reviews::list_reviews_for_card(&state.pool, &card_id).await?))
}

/// A card's current schedule, for showing "next due" on a card the student is
/// looking at outside a session.
pub async fn card_schedule(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(card_id): Path<String>,
) -> Result<Json<academic_core::models::CardSchedule>, ApiError> {
    reviews::get_schedule(&state.pool, &card_id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "this card has never been reviewed"))
}

#[derive(Debug, Deserialize)]
pub struct CheckAnswerBody {
    pub answer: String,
}

#[derive(Debug, serde::Serialize)]
pub struct CheckAnswerResponse {
    /// "correct" | "incorrect" | "undecided"
    pub verdict: String,
    /// The stored answer, returned so an `undecided` result can be judged
    /// side by side without a second request.
    pub accepted: String,
    /// What rating the UI should pre-select. `None` for `undecided`, where
    /// guessing on the student's behalf is the whole thing to avoid.
    pub suggested_rating: Option<String>,
}

/// Grades a typed answer with `crates/grading` — normalise, compare as numbers,
/// allow a length-scaled typo budget — and **never calls the AI**.
///
/// The deliberate design is that only a genuinely ambiguous answer comes back
/// `undecided`, and that case shows both answers for the student to judge
/// rather than spending money to have a model confirm the obvious. A one-word
/// mismatch is simply incorrect.
pub async fn check_answer(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(card_id): Path<String>,
    Json(body): Json<CheckAnswerBody>,
) -> Result<Json<CheckAnswerResponse>, ApiError> {
    let card = academic_core::repo::cards::get(&state.pool, &card_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, format!("no card {card_id}")))?;

    let verdict = match card.kind {
        academic_core::models::CardKind::TrueFalse => grading::grade_true_false(&body.answer, &card.back),
        academic_core::models::CardKind::MultipleChoice => grading::grade_choice(&body.answer, &card.back),
        _ => grading::grade_short_answer(&body.answer, &[card.back.as_str()]),
    };

    Ok(Json(CheckAnswerResponse {
        verdict: verdict.as_str().to_string(),
        accepted: card.back,
        suggested_rating: match verdict {
            grading::Verdict::Correct => Some("good".to_string()),
            grading::Verdict::Incorrect => Some("again".to_string()),
            grading::Verdict::Undecided => None,
        },
    }))
}

#[derive(Debug, Deserialize)]
pub struct ExplainBody {
    /// What the student wrote. Empty is meaningful — "I didn't know" is a
    /// distinct thing to explain.
    #[serde(default)]
    pub answer: String,
}

/// Explains a wrong answer, grounded in the card's own source material.
///
/// Cached per normalised mistake, so getting a card wrong the same way twice
/// costs nothing the second time.
pub async fn explain(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(card_id): Path<String>,
    Json(body): Json<ExplainBody>,
) -> Result<Json<academic_core::models::CardExplanation>, ApiError> {
    Ok(Json(document_engine::explain_mistake(&state.pool, state.ai.as_ref(), &card_id, &body.answer).await?))
}
