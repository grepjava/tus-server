mod app_state;
mod auth;
mod config;
mod dashboard;
mod manager;
mod tus;
mod webhook;

use std::sync::Arc;

use axum::http::HeaderName;
use tokio::{net::TcpListener, sync::broadcast};
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use tracing_subscriber::EnvFilter;

use app_state::AppState;
use config::Config;
use tus::UploadEvent;
use webhook::WebhookDispatcher;

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

    let dispatcher = Arc::new(WebhookDispatcher::new(
        state.webhook_repo.clone(),
        state.upload_service.clone(),
        config.storage_dir.clone(),
    ));
    tokio::spawn(dispatcher.run(event_tx.subscribe()));

    tokio::spawn(manager::run_worker(state.clone()));
    tokio::spawn(manager::run_cleanup(state.clone()));

    let request_id = HeaderName::from_static("x-request-id");

    let app = axum::Router::new()
        .nest("/files", tus::tus_router(state.clone()))
        .merge(dashboard::dashboard_router(state.clone()))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|req: &axum::extract::Request| {
                    let request_id = req
                        .headers()
                        .get("x-request-id")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("-");
                    tracing::info_span!(
                        "request",
                        method = %req.method(),
                        uri = %req.uri(),
                        request_id = %request_id,
                    )
                })
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(PropagateRequestIdLayer::new(request_id.clone()))
        .layer(SetRequestIdLayer::new(request_id, MakeRequestUuid));

    let listener = TcpListener::bind(&config.bind_addr).await?;
    tracing::info!("listening on {}", config.bind_addr);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("server stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let sigterm = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received Ctrl+C"),
        _ = sigterm => tracing::info!("received SIGTERM"),
    }
}
