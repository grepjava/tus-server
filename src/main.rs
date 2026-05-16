mod app_state;
mod audit;
mod auth;
mod config;
mod context;
mod dashboard;
mod login_throttle;
mod manager;
mod metrics;
mod rate_limit;
mod security_headers;
mod trusted_proxy;
mod tus;
mod webhook;

use manager::ProcessorPipeline;

use std::{net::SocketAddr, sync::Arc};

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
use tus::{context_tus_router, FilesystemStorage, S3Storage, StorageBackend, UploadEvent};
use webhook::WebhookDispatcher;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    dotenvy::dotenv().ok();

    let config = Config::from_env()?;
    let pipeline = Arc::new(ProcessorPipeline::from_env()?);
    let rate_limiter = rate_limit::from_config(config.rate_limit_rps, config.rate_limit_burst);
    let login_throttle = Arc::new(login_throttle::LoginThrottle::new(
        config.login_max_attempts,
        config.login_lockout_secs,
    ));
    let dummy_hash: Arc<str> = Arc::from(
        tokio::task::spawn_blocking(|| {
            bcrypt::hash("__tuskar_timing_noop__", bcrypt::DEFAULT_COST)
                .expect("bcrypt dummy hash init")
        })
        .await?,
    );

    let oidc_config = dashboard::build_oidc_config(&config).await?;

    let context_cache = context::ContextCache::new();

    let storage: Arc<dyn StorageBackend> = match config.storage_backend.as_str() {
        "s3" => {
            tracing::info!("using S3 storage backend");
            Arc::new(S3Storage::from_config(&config).await?)
        }
        _ => {
            Arc::new(FilesystemStorage::new(config.storage_dir.clone()))
        }
    };
    if rate_limiter.is_some() {
        let burst = if config.rate_limit_burst > 0 { config.rate_limit_burst } else { config.rate_limit_rps };
        tracing::info!(rps = config.rate_limit_rps, burst, "per-IP rate limiting enabled");
    }

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&config.db_url)
                .create_if_missing(true)
                .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
        )
        .await?;
    sqlx::migrate!().run(&pool).await?;
    dashboard::seed_admin_user(&pool).await?;
    context_cache.load_all(&pool).await?;

    let (event_tx, _) = broadcast::channel::<UploadEvent>(256);

    let mut registry = prometheus_client::registry::Registry::default();
    let app_metrics = Arc::new(metrics::AppMetrics::new(&mut registry));
    let metrics_registry = Arc::new(registry);

    let state = AppState::new(
        pool.clone(),
        config.clone(),
        event_tx.clone(),
        app_metrics.clone(),
        metrics_registry,
        pipeline,
        rate_limiter,
        storage,
        login_throttle,
        dummy_hash,
        oidc_config,
        context_cache,
    );

    let dispatcher = Arc::new(WebhookDispatcher::new(
        state.webhook_repo.clone(),
        state.upload_service.clone(),
        config.storage_dir.clone(),
        state.config.clone(),
        app_metrics,
    ));
    tokio::spawn(dispatcher.run(event_tx.subscribe()));

    tokio::spawn(manager::run_worker(state.clone()));
    tokio::spawn(manager::run_cleanup(state.clone()));

    let request_id = HeaderName::from_static("x-request-id");

    let app = axum::Router::new()
        .nest("/files", tus::tus_router(state.clone()))
        .merge(context_tus_router(state.clone()))
        .merge(
            axum::Router::new()
                .route("/metrics", axum::routing::get(metrics::metrics_handler))
                .with_state(state.clone()),
        )
        .merge(dashboard::dashboard_router(state.clone()))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            security_headers::security_headers_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit::rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            audit::audit_middleware,
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
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
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
