use std::env;
use std::path::PathBuf;

use anyhow::Result;
use job_agent_storage::{default_db_path, init_or_recover_database, reset_database};

#[tokio::main]
async fn main() -> Result<()> {
    let mut db_path = default_db_path();
    let mut force_reset = false;

    for arg in env::args().skip(1) {
        if arg == "--force-reset" {
            force_reset = true;
            continue;
        }

        if let Some(path) = arg.strip_prefix("--db-path=") {
            db_path = PathBuf::from(path);
        }
    }

    if force_reset {
        let path = reset_database(&db_path).await?;
        println!("database reset finished: {}", path.display());
        return Ok(());
    }

    let report = init_or_recover_database(&db_path).await?;
    println!("database ready: {}", report.db_path.display());
    println!("created_or_rebuilt: {}", report.created_or_rebuilt);
    println!(
        "recovered_from_corruption: {}",
        report.recovered_from_corruption
    );
    if let Some(backup) = report.backup_path {
        println!("corrupted backup saved at: {}", backup.display());
    }

    Ok(())
}
