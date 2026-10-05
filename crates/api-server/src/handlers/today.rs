use academic_core::models::AssignmentKind;
use academic_core::repo::{analytics, assignments, courses, reviews};
use axum::extract::{Query, State};
use axum::Json;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct TodayQuery {
    pub local_date: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExamCountdown {
    pub assignment_id: String,
    pub title: String,
    pub course_id: String,
    pub course_name: Option<String>,
    pub kind: String,
    pub due_date: String,
    pub days_away: i64,
}

#[derive(Debug, Serialize)]
pub struct TodayResponse {
    pub due_count: usize,
    pub new_count: usize,
    pub reviews_today: i64,
    pub studied_today: bool,
    /// Exams and quizzes still ahead, soonest first.
    pub exams: Vec<ExamCountdown>,
    /// One sentence for the dashboard, or `None` when there is nothing to say.
    pub nudge: Option<String>,
}

/// Everything the dashboard needs to say "here is where you are today".
///
/// Computed entirely on read — no scheduler, no background job, no stored
/// counters to drift. The container is recreated on every deploy, so anything
/// that depended on a running timer would silently reset; a query cannot.
pub async fn today(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(query): Query<TodayQuery>,
) -> Result<Json<TodayResponse>, ApiError> {
    let today = query
        .local_date
        .as_deref()
        .and_then(|s| NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
        .unwrap_or_else(|| chrono::Utc::now().date_naive());
    let today_str = today.format("%Y-%m-%d").to_string();

    let queue = reviews::due_queue(&state.pool, today, reviews::QueueLimits::default()).await?;
    let due_count = queue.iter().filter(|c| !c.is_new).count();
    let new_count = queue.iter().filter(|c| c.is_new).count();

    let reviews_today = analytics::accuracy_history(&state.pool, None, 365)
        .await?
        .into_iter()
        .find(|d| d.day == today_str)
        .map(|d| d.total)
        .unwrap_or(0);

    let all_courses = courses::list_all(&state.pool).await?;
    let mut exams: Vec<ExamCountdown> = assignments::list_all(&state.pool)
        .await?
        .into_iter()
        // Flashcards prepare you for an exam or a quiz; an essay deadline is a
        // different kind of problem and does not belong in this countdown.
        .filter(|a| matches!(a.kind, AssignmentKind::Exam | AssignmentKind::Quiz))
        .filter_map(|a| {
            let due = NaiveDate::parse_from_str(a.due_date.as_deref()?, "%Y-%m-%d").ok()?;
            let days_away = (due - today).num_days();
            if days_away < 0 {
                return None;
            }
            let course_name = all_courses.iter().find(|c| c.id == a.course_id).map(|c| c.name.clone());
            Some(ExamCountdown {
                assignment_id: a.id,
                title: a.title,
                course_id: a.course_id,
                course_name,
                kind: a.kind.as_str().to_string(),
                due_date: due.to_string(),
                days_away,
            })
        })
        .collect();
    exams.sort_by_key(|e| e.days_away);

    let studied_today = reviews_today > 0;
    let total_waiting = due_count + new_count;
    let nudge = match (studied_today, total_waiting, exams.first()) {
        // Nothing waiting and nothing imminent: say nothing rather than
        // manufacturing a notification.
        (_, 0, None) => None,
        (false, 0, Some(e)) if e.days_away <= 7 => {
            Some(format!("{} in {} days — nothing due today, but worth a look.", e.title, e.days_away))
        }
        (_, 0, Some(_)) => None,
        (false, n, Some(e)) if e.days_away <= 7 => {
            Some(format!("{n} cards waiting, and {} is in {} days.", e.title, e.days_away))
        }
        (false, n, _) => Some(format!("{n} cards waiting and you haven't studied yet today.")),
        (true, n, _) if n > 0 => Some(format!("{reviews_today} done today · {n} still waiting.")),
        (true, _, _) => Some(format!("{reviews_today} done today — you're clear.")),
    };

    Ok(Json(TodayResponse { due_count, new_count, reviews_today, studied_today, exams, nudge }))
}
