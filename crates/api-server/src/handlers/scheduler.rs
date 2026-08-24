use academic_core::models::{Assignment, AssignmentStatus, Difficulty as CoreDifficulty, Priority as CorePriority};
use academic_core::repo::assignments;
use axum::extract::State;
use axum::Json;
use chrono::NaiveDate;
use scheduler::{prioritize, Difficulty, PrioritizedItem, Priority, SchedulableItem, SchedulableStatus};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::state::AppState;

fn map_difficulty(d: CoreDifficulty) -> Difficulty {
    match d {
        CoreDifficulty::Easy => Difficulty::Easy,
        CoreDifficulty::Medium => Difficulty::Medium,
        CoreDifficulty::Hard => Difficulty::Hard,
    }
}

fn map_priority(p: CorePriority) -> Priority {
    match p {
        CorePriority::Low => Priority::Low,
        CorePriority::Medium => Priority::Medium,
        CorePriority::High => Priority::High,
        CorePriority::Urgent => Priority::Urgent,
    }
}

fn map_status(s: AssignmentStatus) -> SchedulableStatus {
    match s {
        AssignmentStatus::NotStarted => SchedulableStatus::NotStarted,
        AssignmentStatus::InProgress => SchedulableStatus::InProgress,
        AssignmentStatus::Waiting => SchedulableStatus::Waiting,
        AssignmentStatus::Completed => SchedulableStatus::Completed,
        AssignmentStatus::Submitted => SchedulableStatus::Submitted,
        AssignmentStatus::Overdue => SchedulableStatus::Overdue,
    }
}

fn to_schedulable(a: &Assignment) -> SchedulableItem {
    SchedulableItem {
        id: a.id.clone(),
        title: a.title.clone(),
        course_id: a.course_id.clone(),
        due_date: a.due_date.as_deref().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()),
        difficulty: map_difficulty(a.difficulty),
        priority: map_priority(a.priority),
        status: map_status(a.status),
        completion_percentage: a.completion_percentage,
    }
}

pub async fn prioritized_today(State(state): State<AppState>, _user: AuthUser) -> Result<Json<Vec<PrioritizedItem>>, ApiError> {
    let all = assignments::list_all(&state.pool).await?;
    let items: Vec<SchedulableItem> = all.iter().map(to_schedulable).collect();
    let today = chrono::Local::now().date_naive();
    Ok(Json(prioritize(&items, today)))
}
