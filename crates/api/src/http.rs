use std::net::SocketAddr;
use std::path::PathBuf;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::Json;
use axum::Router;
use serde::Serialize;

use crate::{StartTaskRequest, StartTaskResponse};
use job_agent_storage::{
    add_service_provider, delete_service_provider, init_or_recover_database, list_service_providers,
    NewServiceProvider, ServiceProvider,
};

#[derive(Clone)]
struct AppState {
    db_path: PathBuf,
}

#[derive(Serialize)]
struct DeleteResult {
    deleted: bool,
}

#[derive(Serialize)]
struct IdResult {
    id: i64,
}

pub async fn run_http_server(db_path: PathBuf, addr: SocketAddr) -> anyhow::Result<()> {
    let report = init_or_recover_database(&db_path).await?;
    if report.recovered_from_corruption {
        eprintln!("database recovered: {}", report.db_path.display());
    }

    let state = AppState { db_path };

    let app = Router::new()
        .route("/health", get(health))
        .route("/services", get(list_services).post(add_service))
        .route("/services/:provider", get(get_service))
        .route("/services/id/:id", delete(delete_service))
        .route("/tasks/start", post(start_task))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> &'static str {
    "ok"
}

async fn list_services(
    State(state): State<AppState>,
) -> Result<Json<Vec<ServiceProvider>>, StatusCode> {
    let items = list_service_providers(&state.db_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(items))
}

async fn get_service(
    State(state): State<AppState>,
    Path(provider): Path<String>,
) -> Result<Json<ServiceProvider>, StatusCode> {
    let items = list_service_providers(&state.db_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let found = items
        .into_iter()
        .find(|s| s.provider_name.eq_ignore_ascii_case(&provider));

    match found {
        Some(svc) => Ok(Json(svc)),
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn add_service(
    State(state): State<AppState>,
    Json(input): Json<NewServiceProvider>,
) -> Result<Json<IdResult>, StatusCode> {
    let id = add_service_provider(&state.db_path, input)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(IdResult { id }))
}

async fn delete_service(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<DeleteResult>, StatusCode> {
    let deleted = delete_service_provider(&state.db_path, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(DeleteResult { deleted }))
}

async fn start_task(
    Json(input): Json<StartTaskRequest>,
) -> Result<Json<StartTaskResponse>, StatusCode> {
    let requirement = input.requirement.trim().to_string();
    if requirement.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    Ok(Json(StartTaskResponse {
        accepted: true,
        requirement,
    }))
}

