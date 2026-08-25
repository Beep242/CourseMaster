use sqlx::{Row, SqlitePool};

use super::new_id;
use crate::error::CoreError;
use crate::models::{CalendarFeed, NewCalendarFeed};

const COLUMNS: &str = "id, name, ics_url, last_synced_at, last_sync_error";

fn row_to_feed(r: sqlx::sqlite::SqliteRow) -> Result<CalendarFeed, CoreError> {
    Ok(CalendarFeed {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        ics_url: r.try_get("ics_url")?,
        last_synced_at: r.try_get("last_synced_at")?,
        last_sync_error: r.try_get("last_sync_error")?,
    })
}

pub async fn create(pool: &SqlitePool, input: NewCalendarFeed) -> Result<CalendarFeed, CoreError> {
    if input.ics_url.trim().is_empty() {
        return Err(CoreError::Validation("calendar feed URL is required".into()));
    }
    let id = new_id();
    sqlx::query("INSERT INTO calendar_feeds (id, name, ics_url) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(&input.name)
        .bind(&input.ics_url)
        .execute(pool)
        .await?;
    get(pool, &id).await?.ok_or_else(|| CoreError::NotFound(id))
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<CalendarFeed>, CoreError> {
    let row = sqlx::query(&format!("SELECT {COLUMNS} FROM calendar_feeds WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(row_to_feed).transpose()
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<CalendarFeed>, CoreError> {
    let rows = sqlx::query(&format!("SELECT {COLUMNS} FROM calendar_feeds ORDER BY created_at DESC"))
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(row_to_feed).collect()
}

pub async fn record_sync_result(pool: &SqlitePool, id: &str, error: Option<&str>) -> Result<(), CoreError> {
    sqlx::query("UPDATE calendar_feeds SET last_synced_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'), last_sync_error = ? WHERE id = ?")
        .bind(error)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
