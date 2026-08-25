use sqlx::{Row, SqlitePool};

use super::new_id;
use crate::error::CoreError;
use crate::models::{
    AssignmentKind, Difficulty, ExtractionEdits, Priority, ReviewStatus, Syllabus, SyllabusExtraction, SyllabusSource,
    SyllabusStatus,
};

const SYLLABUS_COLUMNS: &str = "id, course_id, calendar_feed_id, source, raw_text, status, error_message, imported_at";
const EXTRACTION_COLUMNS: &str = "id, syllabus_id, course_id, kind, title, description, due_date, due_time, \
    source_excerpt, confidence, review_status, resulting_assignment_id, external_uid, external_org_unit_id";

fn row_to_syllabus(r: sqlx::sqlite::SqliteRow) -> Result<Syllabus, CoreError> {
    Ok(Syllabus {
        id: r.try_get("id")?,
        course_id: r.try_get("course_id")?,
        calendar_feed_id: r.try_get("calendar_feed_id")?,
        source: SyllabusSource::parse(&r.try_get::<String, _>("source")?),
        raw_text: r.try_get("raw_text")?,
        status: SyllabusStatus::parse(&r.try_get::<String, _>("status")?),
        error_message: r.try_get("error_message")?,
        imported_at: r.try_get("imported_at")?,
    })
}

fn row_to_extraction(r: sqlx::sqlite::SqliteRow) -> Result<SyllabusExtraction, CoreError> {
    Ok(SyllabusExtraction {
        id: r.try_get("id")?,
        syllabus_id: r.try_get("syllabus_id")?,
        course_id: r.try_get("course_id")?,
        kind: AssignmentKind::parse(&r.try_get::<String, _>("kind")?),
        title: r.try_get("title")?,
        description: r.try_get("description")?,
        due_date: r.try_get("due_date")?,
        due_time: r.try_get("due_time")?,
        source_excerpt: r.try_get("source_excerpt")?,
        confidence: r.try_get("confidence")?,
        review_status: ReviewStatus::parse(&r.try_get::<String, _>("review_status")?),
        resulting_assignment_id: r.try_get("resulting_assignment_id")?,
        external_uid: r.try_get("external_uid")?,
        external_org_unit_id: r.try_get("external_org_unit_id")?,
    })
}

pub async fn create_syllabus(pool: &SqlitePool, course_id: &str, raw_text: &str) -> Result<Syllabus, CoreError> {
    if raw_text.trim().is_empty() {
        return Err(CoreError::Validation("syllabus text is empty".into()));
    }
    let id = new_id();
    sqlx::query("INSERT INTO syllabi (id, course_id, source, raw_text, status) VALUES (?, ?, 'paste', ?, 'processing')")
        .bind(&id)
        .bind(course_id)
        .bind(raw_text)
        .execute(pool)
        .await?;
    get_syllabus(pool, &id).await?.ok_or_else(|| CoreError::NotFound(id))
}

/// Creates a batch representing one calendar-feed sync run. Unlike a pasted
/// syllabus, this has no single course — `raw_text` holds the feed URL for
/// reference, and each extraction below carries its own course guess.
pub async fn create_calendar_batch(pool: &SqlitePool, feed_id: &str, raw_text: &str) -> Result<Syllabus, CoreError> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO syllabi (id, calendar_feed_id, source, raw_text, status) VALUES (?, ?, 'calendar_feed', ?, 'processing')",
    )
    .bind(&id)
    .bind(feed_id)
    .bind(raw_text)
    .execute(pool)
    .await?;
    get_syllabus(pool, &id).await?.ok_or_else(|| CoreError::NotFound(id))
}

pub async fn set_status(pool: &SqlitePool, id: &str, status: SyllabusStatus, error_message: Option<&str>) -> Result<(), CoreError> {
    sqlx::query("UPDATE syllabi SET status = ?, error_message = ? WHERE id = ?")
        .bind(status.as_str())
        .bind(error_message)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_syllabus(pool: &SqlitePool, id: &str) -> Result<Option<Syllabus>, CoreError> {
    let row = sqlx::query(&format!("SELECT {SYLLABUS_COLUMNS} FROM syllabi WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(row_to_syllabus).transpose()
}

pub async fn list_by_course(pool: &SqlitePool, course_id: &str) -> Result<Vec<Syllabus>, CoreError> {
    let rows = sqlx::query(&format!(
        "SELECT {SYLLABUS_COLUMNS} FROM syllabi WHERE course_id = ? ORDER BY imported_at DESC"
    ))
    .bind(course_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(row_to_syllabus).collect()
}

pub async fn list_by_feed(pool: &SqlitePool, feed_id: &str) -> Result<Vec<Syllabus>, CoreError> {
    let rows = sqlx::query(&format!(
        "SELECT {SYLLABUS_COLUMNS} FROM syllabi WHERE calendar_feed_id = ? ORDER BY imported_at DESC"
    ))
    .bind(feed_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(row_to_syllabus).collect()
}

/// UIDs of every extraction already imported (any review state) for a given
/// feed, across all of its past sync batches — lets a sync skip events it's
/// already seen instead of re-inserting duplicates every run.
pub async fn known_external_uids_for_feed(pool: &SqlitePool, feed_id: &str) -> Result<Vec<String>, CoreError> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT se.external_uid FROM syllabus_extractions se \
         JOIN syllabi s ON s.id = se.syllabus_id \
         WHERE s.calendar_feed_id = ? AND se.external_uid IS NOT NULL",
    )
    .bind(feed_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub struct NewExtraction {
    pub kind: AssignmentKind,
    pub title: String,
    pub description: Option<String>,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub source_excerpt: String,
    pub confidence: f64,
    pub course_id: Option<String>,
    pub external_uid: Option<String>,
    pub external_org_unit_id: Option<String>,
}

pub async fn insert_extractions(pool: &SqlitePool, syllabus_id: &str, items: Vec<NewExtraction>) -> Result<(), CoreError> {
    let mut tx = pool.begin().await?;
    for item in items {
        sqlx::query(
            "INSERT INTO syllabus_extractions (id, syllabus_id, course_id, kind, title, description, due_date, due_time, \
             source_excerpt, confidence, external_uid, external_org_unit_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(syllabus_id)
        .bind(&item.course_id)
        .bind(item.kind.as_str())
        .bind(&item.title)
        .bind(&item.description)
        .bind(&item.due_date)
        .bind(&item.due_time)
        .bind(&item.source_excerpt)
        .bind(item.confidence)
        .bind(&item.external_uid)
        .bind(&item.external_org_unit_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Called after a course is linked to an LMS org unit (see
/// `document_engine::calendar_feed::link_course_from_group`) — resolves any
/// extraction that was already sitting pending review with a matching
/// org-unit id but no course guess, so the reviewer doesn't have to go
/// back and manually assign a course to items that arrived before the
/// course existed.
pub async fn backfill_course_for_org_unit(pool: &SqlitePool, org_unit_id: &str, course_id: &str) -> Result<u64, CoreError> {
    let result = sqlx::query(
        "UPDATE syllabus_extractions SET course_id = ? \
         WHERE external_org_unit_id = ? AND course_id IS NULL AND review_status = 'pending'",
    )
    .bind(course_id)
    .bind(org_unit_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn list_extractions(pool: &SqlitePool, syllabus_id: &str) -> Result<Vec<SyllabusExtraction>, CoreError> {
    let rows = sqlx::query(&format!(
        "SELECT {EXTRACTION_COLUMNS} FROM syllabus_extractions WHERE syllabus_id = ? ORDER BY due_date IS NULL, due_date ASC"
    ))
    .bind(syllabus_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(row_to_extraction).collect()
}

pub async fn get_extraction(pool: &SqlitePool, id: &str) -> Result<Option<SyllabusExtraction>, CoreError> {
    let row = sqlx::query(&format!("SELECT {EXTRACTION_COLUMNS} FROM syllabus_extractions WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(row_to_extraction).transpose()
}

/// Promotes a pending extraction into a real `Assignment` row. This is the
/// only path by which AI- or calendar-feed-extracted data becomes
/// schedulable — the spec requires explicit human approval before anything
/// reaches the tracker, so there is deliberately no other way to create an
/// `assignments` row from a syllabus or feed import. The extraction and the
/// new assignment are written in one transaction so a crash never leaves an
/// "approved" extraction with no corresponding assignment (or vice versa).
pub async fn approve_extraction(pool: &SqlitePool, id: &str, edits: Option<ExtractionEdits>) -> Result<String, CoreError> {
    let extraction = get_extraction(pool, id).await?.ok_or_else(|| CoreError::NotFound(id.to_string()))?;
    if extraction.review_status != ReviewStatus::Pending {
        return Err(CoreError::Validation(format!(
            "extraction {id} is already {}",
            extraction.review_status.as_str()
        )));
    }

    let title = edits.as_ref().and_then(|e| e.title.clone()).unwrap_or(extraction.title);
    let description = edits.as_ref().and_then(|e| e.description.clone()).or(extraction.description);
    let due_date = edits.as_ref().and_then(|e| e.due_date.clone()).or(extraction.due_date);
    let due_time = edits.as_ref().and_then(|e| e.due_time.clone()).or(extraction.due_time);
    let kind = edits.as_ref().and_then(|e| e.kind).unwrap_or(extraction.kind);
    let was_edited = edits.is_some();

    let syllabus_course_id: Option<String> = sqlx::query_scalar("SELECT course_id FROM syllabi WHERE id = ?")
        .bind(&extraction.syllabus_id)
        .fetch_one(pool)
        .await?;
    let course_id = edits
        .as_ref()
        .and_then(|e| e.course_id.clone())
        .or(extraction.course_id.clone())
        .or(syllabus_course_id)
        .ok_or_else(|| CoreError::Validation("select a course before approving this item".into()))?;

    let assignment_id = new_id();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO assignments (id, course_id, title, description, kind, due_date, due_time, difficulty, priority, source_extraction_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&assignment_id)
    .bind(&course_id)
    .bind(&title)
    .bind(&description)
    .bind(kind.as_str())
    .bind(&due_date)
    .bind(&due_time)
    .bind(Difficulty::Medium.as_str())
    .bind(Priority::Medium.as_str())
    .bind(id)
    .execute(&mut *tx)
    .await?;

    let new_status = if was_edited { ReviewStatus::Edited } else { ReviewStatus::Approved };
    sqlx::query("UPDATE syllabus_extractions SET review_status = ?, resulting_assignment_id = ? WHERE id = ?")
        .bind(new_status.as_str())
        .bind(&assignment_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(assignment_id)
}

pub async fn reject_extraction(pool: &SqlitePool, id: &str) -> Result<(), CoreError> {
    let result = sqlx::query("UPDATE syllabus_extractions SET review_status = 'rejected' WHERE id = ? AND review_status = 'pending'")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(CoreError::NotFound(id.to_string()));
    }
    Ok(())
}
