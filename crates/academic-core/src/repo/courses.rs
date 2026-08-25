use sqlx::{Row, SqlitePool};

use super::new_id;
use crate::error::CoreError;
use crate::models::{Course, NewCourse};

fn row_to_course(r: sqlx::sqlite::SqliteRow) -> Result<Course, CoreError> {
    Ok(Course {
        id: r.try_get("id")?,
        semester_id: r.try_get("semester_id")?,
        name: r.try_get("name")?,
        code: r.try_get("code")?,
        professor_name: r.try_get("professor_name")?,
        professor_email: r.try_get("professor_email")?,
        credit_hours: r.try_get("credit_hours")?,
        color: r.try_get("color")?,
        current_grade: r.try_get("current_grade")?,
        office_hours: r.try_get("office_hours")?,
        late_policy: r.try_get("late_policy")?,
        external_org_unit_id: r.try_get("external_org_unit_id")?,
    })
}

const COLUMNS: &str = "id, semester_id, name, code, professor_name, professor_email, credit_hours, color, current_grade, \
    office_hours, late_policy, external_org_unit_id";

pub async fn create(pool: &SqlitePool, input: NewCourse) -> Result<Course, CoreError> {
    if input.name.trim().is_empty() {
        return Err(CoreError::Validation("course name is required".into()));
    }
    let id = new_id();
    sqlx::query(
        "INSERT INTO courses (id, semester_id, name, code, professor_name, professor_email, credit_hours, color, external_org_unit_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&input.semester_id)
    .bind(&input.name)
    .bind(&input.code)
    .bind(&input.professor_name)
    .bind(&input.professor_email)
    .bind(input.credit_hours)
    .bind(input.color.unwrap_or_else(|| "#8b5cf6".to_string()))
    .bind(&input.external_org_unit_id)
    .execute(pool)
    .await?;
    get(pool, &id).await?.ok_or_else(|| CoreError::NotFound(id))
}

pub async fn find_by_org_unit_id(pool: &SqlitePool, org_unit_id: &str) -> Result<Option<Course>, CoreError> {
    let row = sqlx::query(&format!("SELECT {COLUMNS} FROM courses WHERE external_org_unit_id = ?"))
        .bind(org_unit_id)
        .fetch_optional(pool)
        .await?;
    row.map(row_to_course).transpose()
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<Course>, CoreError> {
    let row = sqlx::query(&format!("SELECT {COLUMNS} FROM courses WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(row_to_course).transpose()
}

pub async fn list_by_semester(pool: &SqlitePool, semester_id: &str) -> Result<Vec<Course>, CoreError> {
    let rows = sqlx::query(&format!("SELECT {COLUMNS} FROM courses WHERE semester_id = ? ORDER BY name"))
        .bind(semester_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(row_to_course).collect()
}

pub async fn list_all(pool: &SqlitePool) -> Result<Vec<Course>, CoreError> {
    let rows = sqlx::query(&format!("SELECT {COLUMNS} FROM courses ORDER BY name"))
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(row_to_course).collect()
}

pub async fn update_grade(pool: &SqlitePool, id: &str, grade: Option<String>) -> Result<(), CoreError> {
    let result = sqlx::query("UPDATE courses SET current_grade = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?")
        .bind(grade)
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(CoreError::NotFound(id.to_string()));
    }
    Ok(())
}
