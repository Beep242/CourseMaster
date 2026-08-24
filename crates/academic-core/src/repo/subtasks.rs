use sqlx::{Row, SqlitePool};

use super::new_id;
use crate::error::CoreError;
use crate::models::{AssignmentStatus, NewSubtask, Subtask};

fn row_to_subtask(r: sqlx::sqlite::SqliteRow) -> Result<Subtask, CoreError> {
    Ok(Subtask {
        id: r.try_get("id")?,
        assignment_id: r.try_get("assignment_id")?,
        title: r.try_get("title")?,
        status: AssignmentStatus::parse(&r.try_get::<String, _>("status")?),
        estimated_minutes: r.try_get("estimated_minutes")?,
        order_index: r.try_get("order_index")?,
    })
}

pub async fn create(pool: &SqlitePool, input: NewSubtask) -> Result<Subtask, CoreError> {
    let id = new_id();
    sqlx::query("INSERT INTO subtasks (id, assignment_id, title, estimated_minutes, order_index) VALUES (?, ?, ?, ?, ?)")
        .bind(&id)
        .bind(&input.assignment_id)
        .bind(&input.title)
        .bind(input.estimated_minutes)
        .bind(input.order_index)
        .execute(pool)
        .await?;
    list_by_assignment(pool, &input.assignment_id)
        .await?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or(CoreError::NotFound(id))
}

pub async fn list_by_assignment(pool: &SqlitePool, assignment_id: &str) -> Result<Vec<Subtask>, CoreError> {
    let rows = sqlx::query(
        "SELECT id, assignment_id, title, status, estimated_minutes, order_index FROM subtasks \
         WHERE assignment_id = ? ORDER BY order_index",
    )
    .bind(assignment_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(row_to_subtask).collect()
}

pub async fn set_status(pool: &SqlitePool, id: &str, status: AssignmentStatus) -> Result<(), CoreError> {
    let result = sqlx::query("UPDATE subtasks SET status = ? WHERE id = ?")
        .bind(status.as_str())
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(CoreError::NotFound(id.to_string()));
    }
    Ok(())
}
