use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::error::CoreError;

pub async fn connect(db_path: &Path) -> Result<SqlitePool, CoreError> {
    if let Some(parent) = db_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path.display()))?
        .create_if_missing(true)
        .foreign_keys(true)
        // The default rollback journal lets a single writer block every reader,
        // which is the wrong trade for a review session writing once per
        // answered card while the queue is being read. WAL lets readers carry
        // on during a write. It changes the on-disk representation (`-wal` and
        // `-shm` files appear beside the database), which is why
        // scripts/restore-db.sh deletes those sidecars during a restore — they
        // describe the database being replaced.
        .journal_mode(SqliteJournalMode::Wal)
        // Without this a concurrent write fails instantly with SQLITE_BUSY
        // rather than waiting its turn.
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new().max_connections(5).connect_with(opts).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
pub async fn connect_in_memory() -> Result<SqlitePool, CoreError> {
    // No journal_mode here: WAL is meaningless for an in-memory database, and
    // asking for it makes SQLite silently fall back anyway.
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
