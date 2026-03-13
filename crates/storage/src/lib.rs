use std::path::PathBuf;

pub mod db;
pub mod error;

pub use db::{
    add_service_provider, delete_service_provider, init_or_recover_database, list_service_providers,
    reset_database, DatabaseInitReport, NewServiceProvider, ServiceProvider,
};
pub use error::StorageError;

pub fn default_db_path() -> PathBuf {
    PathBuf::from("data/job_agent.db")
}
