use sqlx::{Row, SqlitePool};

use super::new_id;
use crate::error::CoreError;
use crate::models::{Assignment, AssignmentKind, AssignmentStatus, AssignmentUpdate, Difficulty, NewAssignment, Priority};

const COLUMNS: &str = "id, course_id, title, description, kind, due_date, due_time, difficulty, \
    estimated_duration_minutes, priority, status, completion_percentage, source_extraction_id, notes";

fn row_to_assignment(r: sqlx::sqlite::SqliteRow) -> Result<Assignment, CoreError> {
    Ok(Assignment {
        id: r.try_get("id")?,
        course_id: r.try_get("course_id")?,
        title: r.try_get("title")?,
        description: r.try_get("description")?,
        kind: AssignmentKind::parse(&r.try_get::<String, _>("kind")?),
        due_date: r.try_get("due_date")?,
        due_time: r.try_get("due_time")?,
        difficulty: Difficulty::parse(&r.try_get::<String, _>("difficulty")?),
        estimated_duration_minutes: r.try_get("estimated_duration_minutes")?,
        priority: Priority::parse(&r.try_get::<String, _>("priority")?),
        status: AssignmentStatus::parse(&r.try_get::<String, _>("status")?),
        completion_percentage: r.try_get("completion_percentage")?,
        source_extraction_id: r.try_get("source_extraction_id")?,
        notes: r.try_get("notes")?,
    })
}

pub async fn create(pool: &SqlitePool, input: NewAssignment) -> Result<Assignment, CoreError> {
    if input.title.trim().is_empty() {
        return Err(CoreError::Validation("assignment title is required".into()));
    }
    let id = new_id();
    sqlx::query(
        "INSERT INTO assignments (id, course_id, title, description, kind, due_date, due_time, difficulty, \
         estimated_duration_minutes, priority) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&input.course_id)
    .bind(&input.title)
    .bind(&input.description)
    .bind(input.kind.as_str())
    .bind(&input.due_date)
    .bind(&input.due_time)
    .bind(input.difficulty.as_str())
    .bind(input.estimated_duration_minutes)
    .bind(input.priority.as_str())
    .execute(pool)
    .await?;
    get(pool, &id).await?.ok_or_else(|| CoreError::NotFound(id))
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<Assignment>, CoreError> {
    let row = sqlx::query(&format!("SELECT {COLUMNS} FROM assignments WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(row_to_assignment).transpose()
}

pub async fn list_by_course(pool: &SqlitePool, course_id: &str) -> Result<Vec<Assignment>, CoreError> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM assignments WHERE course_id = ? ORDER BY due_date IS NULL, due_date ASC"
    ))
    .bind(course_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(row_to_assignment).collect()
}

pub async fn list_all(pool: &SqlitePool) -> Result<Vec<Assignment>, CoreError> {
    let rows = sqlx::query(&format!("SELECT {COLUMNS} FROM assignments ORDER BY due_date IS NULL, due_date ASC"))
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(row_to_assignment).collect()
}

pub async fn update(pool: &SqlitePool, id: &str, patch: AssignmentUpdate) -> Result<Assignment, CoreError> {
    let existing = get(pool, id).await?.ok_or_else(|| CoreError::NotFound(id.to_string()))?;

    let course_id = patch.course_id.unwrap_or(existing.course_id);
    let title = patch.title.unwrap_or(existing.title);
    let description = patch.description.or(existing.description);
    let due_date = patch.due_date.or(existing.due_date);
    let due_time = patch.due_time.or(existing.due_time);
    let difficulty = patch.difficulty.unwrap_or(existing.difficulty);
    let estimated_duration_minutes = patch.estimated_duration_minutes.or(existing.estimated_duration_minutes);
    let priority = patch.priority.unwrap_or(existing.priority);
    let status = patch.status.unwrap_or(existing.status);
    let completion_percentage = patch.completion_percentage.unwrap_or(existing.completion_percentage).clamp(0, 100);
    let notes = patch.notes.or(existing.notes);

    sqlx::query(
        "UPDATE assignments SET course_id=?, title=?, description=?, due_date=?, due_time=?, difficulty=?, \
         estimated_duration_minutes=?, priority=?, status=?, completion_percentage=?, notes=?, \
         updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?",
    )
    .bind(&course_id)
    .bind(&title)
    .bind(&description)
    .bind(&due_date)
    .bind(&due_time)
    .bind(difficulty.as_str())
    .bind(estimated_duration_minutes)
    .bind(priority.as_str())
    .bind(status.as_str())
    .bind(completion_percentage)
    .bind(&notes)
    .bind(id)
    .execute(pool)
    .await?;

    get(pool, id).await?.ok_or_else(|| CoreError::NotFound(id.to_string()))
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), CoreError> {
    let result = sqlx::query("DELETE FROM assignments WHERE id = ?").bind(id).execute(pool).await?;
    if result.rows_affected() == 0 {
        return Err(CoreError::NotFound(id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::models::{NewCourse, NewSemester};
    use crate::repo::{courses, semesters};

    async fn seed_course(pool: &SqlitePool) -> String {
        let sem = semesters::create(pool, NewSemester { name: "Fall".into(), start_date: None, end_date: None })
            .await
            .unwrap();
        let course = courses::create(
            pool,
            NewCourse {
                semester_id: sem.id,
                name: "Algorithms".into(),
                code: Some("CS301".into()),
                professor_name: None,
                professor_email: None,
                credit_hours: Some(3.0),
                color: None,
                external_org_unit_id: None,
            },
        )
        .await
        .unwrap();
        course.id
    }

    #[tokio::test]
    async fn create_and_update_round_trips() {
        let pool = connect_in_memory().await.unwrap();
        let course_id = seed_course(&pool).await;

        let a = create(
            &pool,
            NewAssignment {
                course_id: course_id.clone(),
                title: "Problem set 1".into(),
                description: None,
                kind: AssignmentKind::Assignment,
                due_date: Some("2026-09-01".into()),
                due_time: None,
                difficulty: Difficulty::Medium,
                estimated_duration_minutes: Some(90),
                priority: Priority::High,
            },
        )
        .await
        .unwrap();
        assert_eq!(a.status, AssignmentStatus::NotStarted);

        let updated = update(
            &pool,
            &a.id,
            AssignmentUpdate {
                course_id: None,
                title: None,
                description: None,
                due_date: None,
                due_time: None,
                difficulty: None,
                estimated_duration_minutes: None,
                priority: None,
                status: Some(AssignmentStatus::InProgress),
                completion_percentage: Some(150),
                notes: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.status, AssignmentStatus::InProgress);
        assert_eq!(updated.completion_percentage, 100, "completion percentage must clamp to 100");
    }

    #[tokio::test]
    async fn rejects_blank_title() {
        let pool = connect_in_memory().await.unwrap();
        let course_id = seed_course(&pool).await;
        let err = create(
            &pool,
            NewAssignment {
                course_id,
                title: "   ".into(),
                description: None,
                kind: AssignmentKind::Assignment,
                due_date: None,
                due_time: None,
                difficulty: Difficulty::Medium,
                estimated_duration_minutes: None,
                priority: Priority::Medium,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, CoreError::Validation(_)));
    }
}
