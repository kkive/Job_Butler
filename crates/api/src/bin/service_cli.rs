use std::env;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use job_agent_api::get_service_provider_via_api;
use job_agent_storage::default_db_path;
use serde_json::json;

fn parse_arg_value(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|v| v == key)
        .and_then(|idx| args.get(idx + 1))
        .cloned()
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let command = args.get(1).map(String::as_str).unwrap_or("help");

    match command {
        "get" => {
            let provider = parse_arg_value(&args, "--provider")
                .ok_or_else(|| anyhow!("missing --provider"))?;
            let db_path = parse_arg_value(&args, "--db-path")
                .map(PathBuf::from)
                .unwrap_or_else(default_db_path);

            let rt = tokio::runtime::Runtime::new()?;
            let svc = rt.block_on(get_service_provider_via_api(&db_path, &provider))?;
            let out = json!({
                "provider_name": svc.provider_name,
                "model_name": svc.model_name,
                "api_url": svc.api_url,
                "api_key": svc.api_key,
            });
            println!("{}", out.to_string());
        }
        _ => {
            eprintln!("Usage: service_cli get --provider <name> [--db-path <path>]");
        }
    }

    Ok(())
}
