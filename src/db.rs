// =============================================================================
// db.rs — the SQLite database
//
// One file holds everything EasyDecoy keeps: settings, events, accounts,
// sessions and the audit trail. It is opened in WAL mode, so the GUI reads
// while events are written, and the schema in migrations/ is applied on every
// start, so an upgraded binary brings its database up to date by itself.
//
// The file is created readable by its owner only: it holds password hashes
// and the GUI's TLS key.
// =============================================================================

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use sqlx::sqlite::{
    SqliteAutoVacuum, SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
};

// ─────────────────────────────────────────────────────────────────────────────
// open(path)
// Opens (creating if needed) the database at `path` and applies any schema
// change it has not had yet.
// ─────────────────────────────────────────────────────────────────────────────
pub async fn open(path: &str) -> Result<SqlitePool> {
    let new = !Path::new(path).exists();
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        // NORMAL is safe in WAL mode: a power cut can lose the last moments,
        // never corrupt the file.
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5))
        // Lets retention hand the space of pruned events back to the disk. It
        // only takes effect on a database created with it, so it is set always.
        .auto_vacuum(SqliteAutoVacuum::Incremental);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .with_context(|| format!("opening database {path}"))?;

    if new {
        restrict(path);
    }
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .with_context(|| format!("upgrading database {path}"))?;
    Ok(pool)
}

// ─────────────────────────────────────────────────────────────────────────────
// close(pool)
// Folds the write-ahead log back into the database file and closes it, so the
// .db file on its own is a complete copy once EasyDecoy has stopped.
// ─────────────────────────────────────────────────────────────────────────────
pub async fn close(pool: &SqlitePool) {
    let _ = sqlx::raw_sql("PRAGMA wal_checkpoint(TRUNCATE)").execute(pool).await;
    pool.close().await;
}

#[cfg(unix)]
fn restrict(path: &str) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        tracing::warn!("could not make {path} private to its owner: {e}");
    }
}

#[cfg(not(unix))]
fn restrict(_path: &str) {}

/// A pool on a fresh database in memory, for tests.
#[cfg(test)]
pub async fn memory() -> SqlitePool {
    let options = SqliteConnectOptions::new().filename(":memory:").foreign_keys(true);
    // One connection: each in-memory connection would be a separate database.
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(options).await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}
