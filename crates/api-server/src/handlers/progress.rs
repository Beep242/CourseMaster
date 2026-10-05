use academic_core::models::{DayAccuracy, DeckMastery, StudyTotals, WeakCard};
use academic_core::repo::analytics;
use axum::extract::{Query, State};
use axum::Json;
use chrono::NaiveDate;
use examready::{assess, Readiness, ReadinessInput};
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ProgressQuery {
    /// Omitted means the whole library, which is the useful default for
    /// someone taking seven courses.
    pub course_id: Option<String>,
    pub days: Option<i64>,
    pub limit: Option<i64>,
    /// The client's local date, so "days until exam" is counted from the
    /// student's day rather than the server's UTC one.
    pub local_date: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProgressResponse {
    pub totals: StudyTotals,
    pub history: Vec<DayAccuracy>,
    pub weak_cards: Vec<WeakCard>,
    pub decks: Vec<DeckMastery>,
    pub readiness: Readiness,
    /// The soonest exam or quiz counted, so the score's reason can be checked.
    pub next_exam: Option<NextExam>,
}

#[derive(Debug, Serialize)]
pub struct NextExam {
    pub title: String,
    pub due_date: String,
    pub days_away: i64,
}

/// One request backing the whole Progress page. Four small queries server-side
/// beats four round trips from a phone.
pub async fn progress(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(query): Query<ProgressQuery>,
) -> Result<Json<ProgressResponse>, ApiError> {
    let course_id = query.course_id.filter(|c| !c.is_empty());
    let course = course_id.as_deref();

    let totals = analytics::study_totals(&state.pool, course).await?;
    let history = analytics::accuracy_history(&state.pool, course, query.days.unwrap_or(30)).await?;
    let weak_cards = analytics::weak_cards(&state.pool, course, query.limit.unwrap_or(10)).await?;
    let decks = analytics::deck_mastery(&state.pool, course).await?;

    let today = query
        .local_date
        .as_deref()
        .and_then(|s| NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
        .unwrap_or_else(|| chrono::Utc::now().date_naive());

    // The next exam or quiz still ahead, which is what readiness is measured
    // against. Assignments are deliberately excluded: an essay deadline is not
    // something flashcards prepare you for.
    let next_exam = academic_core::repo::assignments::list_all(&state.pool)
        .await?
        .into_iter()
        .filter(|a| matches!(a.kind, academic_core::models::AssignmentKind::Exam | academic_core::models::AssignmentKind::Quiz))
        .filter(|a| course.is_none_or(|c| a.course_id == c))
        .filter_map(|a| {
            let due = NaiveDate::parse_from_str(a.due_date.as_deref()?, "%Y-%m-%d").ok()?;
            let days = (due - today).num_days();
            (days >= 0).then_some(NextExam { title: a.title, due_date: due.to_string(), days_away: days })
        })
        .min_by_key(|e| e.days_away);

    let total_cards: i64 = decks.iter().map(|d| d.total_cards).sum();
    let seen_cards: i64 = decks.iter().map(|d| d.seen_cards).sum();
    let mature_cards: i64 = decks.iter().map(|d| d.mature_cards).sum();

    let readiness = assess(ReadinessInput {
        total_cards,
        seen_cards,
        mature_cards,
        total_reviews: totals.reviews,
        correct_reviews: totals.correct,
        days_until_exam: next_exam.as_ref().map(|e| e.days_away),
    });

    Ok(Json(ProgressResponse { totals, history, weak_cards, decks, readiness, next_exam }))
}
