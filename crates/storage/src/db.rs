use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Pool, Sqlite};

use crate::error::StorageError;

const SCHEMA_SQL: &str = include_str!("../../../migrations/0001_init.sql");

#[derive(Debug, Clone)]
pub struct DatabaseInitReport {
    pub db_path: PathBuf,
    pub created_or_rebuilt: bool,
    pub recovered_from_corruption: bool,
    pub backup_path: Option<PathBuf>,
}

pub async fn init_or_recover_database<P: AsRef<Path>>(
    db_path: P,
) -> Result<DatabaseInitReport, StorageError> {
    let db_path = db_path.as_ref().to_path_buf();
    ensure_parent_dir(&db_path).await?;

    let existed_before = db_path.exists();
    let mut recovered = false;
    let mut backup_path = None;

    if existed_before {
        let healthy = is_database_healthy(&db_path).await?;
        if !healthy {
            let backup = backup_corrupted_database(&db_path).await?;
            recovered = true;
            backup_path = Some(backup);
        }
    }

    let pool = open_pool(&db_path, true).await?;
    apply_pragmas(&pool).await?;
    apply_schema(&pool).await?;
    pool.close().await;

    Ok(DatabaseInitReport {
        db_path,
        created_or_rebuilt: !existed_before || recovered,
        recovered_from_corruption: recovered,
        backup_path,
    })
}

pub async fn reset_database<P: AsRef<Path>>(db_path: P) -> Result<PathBuf, StorageError> {
    let db_path = db_path.as_ref().to_path_buf();
    ensure_parent_dir(&db_path).await?;

    let backup = if db_path.exists() {
        Some(backup_existing_database(&db_path, "reset").await?)
    } else {
        None
    };

    let report = init_or_recover_database(&db_path).await?;
    if !report.created_or_rebuilt {
        return Err(StorageError::RecoveryFailed {
            backup_path: backup.unwrap_or(db_path),
        });
    }

    Ok(report.db_path)
}

async fn is_database_healthy(db_path: &Path) -> Result<bool, StorageError> {
    let pool = match open_pool(db_path, false).await {
        Ok(pool) => pool,
        Err(_) => return Ok(false),
    };

    let result: Result<(String,), sqlx::Error> = sqlx::query_as("PRAGMA quick_check;")
        .fetch_one(&pool)
        .await;

    pool.close().await;

    match result {
        Ok((status,)) => Ok(status.eq_ignore_ascii_case("ok")),
        Err(_) => Ok(false),
    }
}

async fn ensure_parent_dir(db_path: &Path) -> Result<(), StorageError> {
    let parent = db_path
        .parent()
        .ok_or_else(|| StorageError::InvalidPath(db_path.display().to_string()))?;

    tokio::fs::create_dir_all(parent).await?;
    Ok(())
}

async fn backup_corrupted_database(db_path: &Path) -> Result<PathBuf, StorageError> {
    backup_existing_database(db_path, "corrupt").await
}

async fn backup_existing_database(db_path: &Path, suffix: &str) -> Result<PathBuf, StorageError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| StorageError::InvalidPath(format!("system clock error: {e}")))?
        .as_secs();

    let file_name = db_path
        .file_name()
        .ok_or_else(|| StorageError::InvalidPath(db_path.display().to_string()))?
        .to_string_lossy();

    let backup_name = format!("{file_name}.{suffix}.{timestamp}");
    let backup_path = db_path
        .parent()
        .ok_or_else(|| StorageError::InvalidPath(db_path.display().to_string()))?
        .join(backup_name);

    tokio::fs::rename(db_path, &backup_path).await?;
    Ok(backup_path)
}

async fn open_pool(db_path: &Path, create_if_missing: bool) -> Result<Pool<Sqlite>, StorageError> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(create_if_missing)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;

    Ok(pool)
}

async fn apply_pragmas(pool: &Pool<Sqlite>) -> Result<(), StorageError> {
    sqlx::query("PRAGMA journal_mode=WAL;").execute(pool).await?;
    sqlx::query("PRAGMA foreign_keys=ON;").execute(pool).await?;
    Ok(())
}

async fn apply_schema(pool: &Pool<Sqlite>) -> Result<(), StorageError> {
    for statement in SCHEMA_SQL.split(';') {
        let sql = statement.trim();
        if sql.is_empty() {
            continue;
        }

        sqlx::query(sql).execute(pool).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn init_creates_database_and_tables() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let db_path = tmp.path().join("job_agent.db");

        let report = init_or_recover_database(&db_path)
            .await
            .expect("init database");

        assert!(report.created_or_rebuilt);
        assert!(!report.recovered_from_corruption);
        assert!(db_path.exists());

        let pool = open_pool(&db_path, false).await.expect("open db");
        let row: (String,) = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='task';",
        )
        .fetch_one(&pool)
        .await
        .expect("query table");
        assert_eq!(row.0, "task");
        pool.close().await;
    }

    #[tokio::test]
    async fn init_recovers_corrupted_database_file() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let db_path = tmp.path().join("job_agent.db");

        tokio::fs::write(&db_path, b"not-a-sqlite-db")
            .await
            .expect("write corrupted bytes");

        let report = init_or_recover_database(&db_path)
            .await
            .expect("recover database");

        assert!(report.created_or_rebuilt);
        assert!(report.recovered_from_corruption);
        assert!(report.backup_path.as_ref().is_some_and(|p| p.exists()));

        let pool = open_pool(&db_path, false).await.expect("open db");
        let row: (String,) = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='application';",
        )
        .fetch_one(&pool)
        .await
        .expect("query table");
        assert_eq!(row.0, "application");
        pool.close().await;
    }
}
