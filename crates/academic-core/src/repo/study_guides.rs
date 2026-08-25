use sqlx::{Row, SqlitePool};

use super::new_id;
use crate::error::CoreError;
use crate::models::{StudyGuide, StudyGuideKind};

const COLUMNS: &str = "id, course_id, kind, title, content, created_at";

fn row_to_guide(r: sqlx::sqlite::SqliteRow) -> Result<StudyGuide, CoreError> {
    Ok(StudyGuide {
        id: r.try_get("id")?,
        course_id: r.try_get("course_id")?,
        kind: StudyGuideKind::parse(&r.try_get::<String, _>("kind")?),
        title: r.try_get("title")?,
        content: r.try_get("content")?,
        created_at: r.try_get("created_at")?,
    })
}

pub async fn create(pool: &SqlitePool, course_id: &str, kind: StudyGuideKind, title: &str, content: &str) -> Result<StudyGuide, CoreError> {
    let id = new_id();
    sqlx::query("INSERT INTO study_guides (id, course_id, kind, title, content) VALUES (?, ?, ?, ?, ?)")
        .bind(&id)
        .bind(course_id)
        .bind(kind.as_str())
        .bind(title)
        .bind(content)
        .execute(pool)
        .await?;
    get(pool, &id).await?.ok_or_else(|| CoreError::NotFound(id))
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<StudyGuide>, CoreError> {
    let row = sqlx::query(&format!("SELECT {COLUMNS} FROM study_guides WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(row_to_guide).transpose()
}

pub async fn list_by_course(pool: &SqlitePool, course_id: &str) -> Result<Vec<StudyGuide>, CoreError> {
    let rows = sqlx::query(&format!("SELECT {COLUMNS} FROM study_guides WHERE course_id = ? ORDER BY created_at DESC"))
        .bind(course_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(row_to_guide).collect()
}
