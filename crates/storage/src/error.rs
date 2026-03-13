use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("sqlite error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("invalid database path: {0}")]
    InvalidPath(String),

    #[error("database integrity check failed: {details}")]
    IntegrityFailed { details: String },

    #[error("database recovery failed, backup at: {backup_path:?}")]
    RecoveryFailed { backup_path: PathBuf },
}
