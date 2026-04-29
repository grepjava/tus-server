mod app_state;
mod config;
mod dashboard;
mod manager;
mod tus;
mod webhook;

use std::sync::Arc;

use tokio::{net::TcpListener, sync::broadcast};
use tracing_subscriber::EnvFilter;

use app_state::AppState;
use config::Config;
use tus::UploadEvent;
use webhook::{SqliteWebhookRepository, WebhookDispatcher};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    dotenvy::dotenv().ok();

    let config = Config::from_env()?;

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&config.db_url)
                .create_if_missing(true),
        )
        .await?;
    sqlx::migrate!().run(&pool).await?;

    let (event_tx, _) = broadcast::channel::<UploadEvent>(256);
    let state = AppState::new(pool.clone(), config.clone(), event_tx.clone());

    let webhook_repo = Arc::new(SqliteWebhookRepository::new(pool));
    let dispatcher = Arc::new(WebhookDispatcher::new(
        webhook_repo,
        state.upload_service.clone(),
        config.storage_dir.clone(),
    ));
    tokio::spawn(dispatcher.run(event_tx.subscribe()));

    tokio::spawn(manager::run_worker(state.clone()));
    tokio::spawn(manager::run_cleanup(state.clone()));

    let app = axum::Router::new()
        .nest("/files", tus::tus_router(state.clone()))
        .merge(dashboard::dashboard_router(state.clone()));

    let listener = TcpListener::bind(&config.bind_addr).await?;
    tracing::info!("listening on {}", config.bind_addr);
    axum::serve(listener, app).await?;

    Ok(())
}
