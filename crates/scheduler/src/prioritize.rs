use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// A deliberately minimal, engine-independent view of an assignment — the
/// scheduler crate has no dependency on `academic-core` so its scoring logic
/// can be unit tested in isolation, per the spec's "testable independently
/// of the UI [and persistence]" requirement. `academic-core::Assignment`
/// maps onto this at the call site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulableItem {
    pub id: String,
    pub title: String,
    pub course_id: String,
    pub due_date: Option<NaiveDate>,
    pub difficulty: Difficulty,
    pub priority: Priority,
    pub status: SchedulableStatus,
    pub completion_percentage: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    Medium,
    High,
    Urgent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulableStatus {
    NotStarted,
    InProgress,
    Waiting,
    Completed,
    Submitted,
    Overdue,
}

impl SchedulableStatus {
    fn is_done(&self) -> bool {
        matches!(self, SchedulableStatus::Completed | SchedulableStatus::Submitted)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrioritizedItem {
    pub id: String,
    pub title: String,
    pub course_id: String,
    pub score: f64,
    pub days_until_due: Option<i64>,
    pub is_overdue: bool,
    pub reason: String,
}

fn difficulty_weight(d: Difficulty) -> f64 {
    match d {
        Difficulty::Easy => 0.0,
        Difficulty::Medium => 7.0,
        Difficulty::Hard => 15.0,
    }
}

fn priority_weight(p: Priority) -> f64 {
    match p {
        Priority::Low => 0.0,
        Priority::Medium => 10.0,
        Priority::High => 25.0,
        Priority::Urgent => 40.0,
    }
}

fn urgency_weight(days_until_due: Option<i64>) -> f64 {
    match days_until_due {
        None => 5.0,
        Some(days) if days < 0 => 120.0 + (-days) as f64 * 5.0,
        Some(days) => (60.0 - (days as f64 * 4.0)).max(0.0),
    }
}

fn priority_label(p: Priority) -> &'static str {
    match p {
        Priority::Low => "low priority",
        Priority::Medium => "medium priority",
        Priority::High => "high priority",
        Priority::Urgent => "urgent",
    }
}

fn difficulty_label(d: Difficulty) -> &'static str {
    match d {
        Difficulty::Easy => "easy",
        Difficulty::Medium => "medium difficulty",
        Difficulty::Hard => "hard",
    }
}

/// Scores and sorts assignments into "what should I do right now" order.
/// This is intentionally a simple, explainable weighted-sum model for the
/// first vertical slice — spaced repetition, workload balancing across
/// available study time, and recovery-from-missed-work all build on top of
/// this in a later phase, not inside it.
pub fn prioritize(items: &[SchedulableItem], today: NaiveDate) -> Vec<PrioritizedItem> {
    let mut scored: Vec<PrioritizedItem> = items
        .iter()
        .filter(|item| !item.status.is_done())
        .map(|item| {
            let days_until_due = item.due_date.map(|d| (d - today).num_days());
            let is_overdue = days_until_due.map(|d| d < 0).unwrap_or(false);
            let in_progress_bonus = if item.status == SchedulableStatus::InProgress { 5.0 } else { 0.0 };
            let remaining_work_factor = 1.0 - (item.completion_percentage.clamp(0, 100) as f64 / 200.0);

            let base = urgency_weight(days_until_due) + difficulty_weight(item.difficulty) + priority_weight(item.priority) + in_progress_bonus;
            let score = (base * remaining_work_factor * 10.0).round() / 10.0;

            let reason = match days_until_due {
                Some(d) if d < 0 => format!("Overdue by {} day{} · {} · {}", -d, if -d == 1 { "" } else { "s" }, priority_label(item.priority), difficulty_label(item.difficulty)),
                Some(0) => format!("Due today · {} · {}", priority_label(item.priority), difficulty_label(item.difficulty)),
                Some(d) => format!("Due in {d} day{} · {} · {}", if d == 1 { "" } else { "s" }, priority_label(item.priority), difficulty_label(item.difficulty)),
                None => format!("No due date · {} · {}", priority_label(item.priority), difficulty_label(item.difficulty)),
            };

            PrioritizedItem {
                id: item.id.clone(),
                title: item.title.clone(),
                course_id: item.course_id.clone(),
                score,
                days_until_due,
                is_overdue,
                reason,
            }
        })
        .collect();

    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, days_from_today: Option<i64>, today: NaiveDate, difficulty: Difficulty, priority: Priority, status: SchedulableStatus) -> SchedulableItem {
        SchedulableItem {
            id: id.into(),
            title: id.into(),
            course_id: "course-1".into(),
            due_date: days_from_today.map(|d| today + chrono::Duration::days(d)),
            difficulty,
            priority,
            status,
            completion_percentage: 0,
        }
    }

    #[test]
    fn overdue_items_rank_above_everything_else() {
        let today = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let items = vec![
            item("far-future", Some(30), today, Difficulty::Hard, Priority::Urgent, SchedulableStatus::NotStarted),
            item("overdue", Some(-2), today, Difficulty::Easy, Priority::Low, SchedulableStatus::NotStarted),
        ];
        let ranked = prioritize(&items, today);
        assert_eq!(ranked[0].id, "overdue");
        assert!(ranked[0].is_overdue);
    }

    #[test]
    fn completed_and_submitted_items_are_excluded() {
        let today = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let items = vec![
            item("done", Some(-5), today, Difficulty::Hard, Priority::Urgent, SchedulableStatus::Completed),
            item("submitted", Some(1), today, Difficulty::Hard, Priority::Urgent, SchedulableStatus::Submitted),
            item("active", Some(1), today, Difficulty::Easy, Priority::Low, SchedulableStatus::NotStarted),
        ];
        let ranked = prioritize(&items, today);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].id, "active");
    }

    #[test]
    fn closer_deadline_outranks_farther_deadline_all_else_equal() {
        let today = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let items = vec![
            item("far", Some(10), today, Difficulty::Medium, Priority::Medium, SchedulableStatus::NotStarted),
            item("near", Some(1), today, Difficulty::Medium, Priority::Medium, SchedulableStatus::NotStarted),
        ];
        let ranked = prioritize(&items, today);
        assert_eq!(ranked[0].id, "near");
    }

    #[test]
    fn harder_assignment_outranks_easier_at_the_same_deadline() {
        let today = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let items = vec![
            item("easy", Some(3), today, Difficulty::Easy, Priority::Medium, SchedulableStatus::NotStarted),
            item("hard", Some(3), today, Difficulty::Hard, Priority::Medium, SchedulableStatus::NotStarted),
        ];
        let ranked = prioritize(&items, today);
        assert_eq!(ranked[0].id, "hard");
    }
}
