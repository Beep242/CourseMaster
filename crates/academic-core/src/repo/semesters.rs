use sqlx::{Row, SqlitePool};

use super::new_id;
use crate::error::CoreError;
use crate::models::{NewSemester, Semester};

fn row_to_semester(r: sqlx::sqlite::SqliteRow) -> Result<Semester, CoreError> {
    Ok(Semester {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        start_date: r.try_get("start_date")?,
        end_date: r.try_get("end_date")?,
        is_active: r.try_get::<i64, _>("is_active")? != 0,
    })
}

pub async fn create(pool: &SqlitePool, input: NewSemester) -> Result<Semester, CoreError> {
    let id = new_id();
    sqlx::query("INSERT INTO semesters (id, name, start_date, end_date) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(&input.name)
        .bind(&input.start_date)
        .bind(&input.end_date)
        .execute(pool)
        .await?;
    get(pool, &id).await?.ok_or_else(|| CoreError::NotFound(id))
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<Semester>, CoreError> {
    let row = sqlx::query("SELECT id, name, start_date, end_date, is_active FROM semesters WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(row_to_semester).transpose()
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<Semester>, CoreError> {
    let rows = sqlx::query("SELECT id, name, start_date, end_date, is_active FROM semesters ORDER BY created_at DESC")
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(row_to_semester).collect()
}

/// Cascades to every course in the semester, and from there to their
/// assignments/syllabi/study tools — the API layer requires explicit
/// confirmation before this is ever reached.
pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), CoreError> {
    let result = sqlx::query("DELETE FROM semesters WHERE id = ?").bind(id).execute(pool).await?;
    if result.rows_affected() == 0 {
        return Err(CoreError::NotFound(id.to_string()));
    }
    Ok(())
}
