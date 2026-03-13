use std::path::PathBuf;

pub mod db;
pub mod error;

pub use db::{init_or_recover_database, reset_database, DatabaseInitReport};
pub use error::StorageError;

pub fn default_db_path() -> PathBuf {
    PathBuf::from("data/job_agent.db")
}
