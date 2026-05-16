use axum::{extract::State, response::IntoResponse};
use prometheus_client::{
    encoding::{text::encode, EncodeLabelSet},
    metrics::{counter::Counter, family::Family, gauge::Gauge},
    registry::Registry,
};

use crate::app_state::AppState;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct DeliveryLabels {
    pub outcome: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct ContextLabels {
    pub context: String,
}

pub struct AppMetrics {
    pub uploads_created_total: Family<ContextLabels, Counter>,
    pub uploads_completed_total: Family<ContextLabels, Counter>,
    pub processing_failures_total: Family<ContextLabels, Counter>,
    pub bytes_uploaded_total: Family<ContextLabels, Counter>,
    pub webhook_deliveries_total: Family<DeliveryLabels, Counter>,
    pub active_uploads: Gauge,
    pub processing_uploads: Gauge,
    pub active_uploads_by_context: Family<ContextLabels, Gauge>,
    pub processing_uploads_by_context: Family<ContextLabels, Gauge>,
}

impl AppMetrics {
    pub fn new(registry: &mut Registry) -> Self {
        let uploads_created_total = Family::<ContextLabels, Counter>::default();
        let uploads_completed_total = Family::<ContextLabels, Counter>::default();
        let processing_failures_total = Family::<ContextLabels, Counter>::default();
        let bytes_uploaded_total = Family::<ContextLabels, Counter>::default();
        let webhook_deliveries_total = Family::<DeliveryLabels, Counter>::default();
        let active_uploads = Gauge::default();
        let processing_uploads = Gauge::default();
        let active_uploads_by_context = Family::<ContextLabels, Gauge>::default();
        let processing_uploads_by_context = Family::<ContextLabels, Gauge>::default();

        registry.register(
            "tus_uploads_created",
            "Total uploads created, labelled by context",
            uploads_created_total.clone(),
        );
        registry.register(
            "tus_uploads_completed",
            "Total uploads where all bytes were received, labelled by context",
            uploads_completed_total.clone(),
        );
        registry.register(
            "tus_processing_failures",
            "Total uploads that failed the processing step, labelled by context",
            processing_failures_total.clone(),
        );
        registry.register(
            "tus_bytes_uploaded",
            "Total bytes received across all uploads, labelled by context",
            bytes_uploaded_total.clone(),
        );
        registry.register(
            "tus_webhook_deliveries",
            "Total webhook delivery attempts labelled by outcome (success|failure)",
            webhook_deliveries_total.clone(),
        );
        registry.register(
            "tus_active_uploads",
            "Current uploads in Created or Uploading state (global total)",
            active_uploads.clone(),
        );
        registry.register(
            "tus_processing_uploads",
            "Current uploads in Processing state (global total)",
            processing_uploads.clone(),
        );
        registry.register(
            "tus_active_uploads_by_context",
            "Current uploads in Created or Uploading state, labelled by context",
            active_uploads_by_context.clone(),
        );
        registry.register(
            "tus_processing_uploads_by_context",
            "Current uploads in Processing state, labelled by context",
            processing_uploads_by_context.clone(),
        );

        Self {
            uploads_created_total,
            uploads_completed_total,
            processing_failures_total,
            bytes_uploaded_total,
            webhook_deliveries_total,
            active_uploads,
            processing_uploads,
            active_uploads_by_context,
            processing_uploads_by_context,
        }
    }
}

pub async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    // Refresh global point-in-time gauges.
    if let Ok(n) = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM uploads WHERE status IN ('Created', 'Uploading')",
    )
    .fetch_one(&state.db_pool)
    .await
    {
        state.metrics.active_uploads.set(n);
    }

    if let Ok(n) = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM uploads WHERE status = 'Processing'",
    )
    .fetch_one(&state.db_pool)
    .await
    {
        state.metrics.processing_uploads.set(n);
    }

    // Refresh per-context gauges. Zero all known contexts first to clear stale values,
    // then fill in actual counts from the DB.
    let all_contexts = state.context_cache.all().await;
    for ctx in &all_contexts {
        let lbl = ContextLabels { context: ctx.slug.clone() };
        state.metrics.active_uploads_by_context.get_or_create(&lbl).set(0);
        state.metrics.processing_uploads_by_context.get_or_create(&lbl).set(0);
    }
    state.metrics.active_uploads_by_context
        .get_or_create(&ContextLabels { context: "global".into() }).set(0);
    state.metrics.processing_uploads_by_context
        .get_or_create(&ContextLabels { context: "global".into() }).set(0);

    type CtxCount = (String, i64);
    if let Ok(rows) = sqlx::query_as::<_, CtxCount>(
        "SELECT COALESCE(c.slug, 'global'), COUNT(*) \
         FROM uploads u LEFT JOIN contexts c ON u.context_id = c.id \
         WHERE u.status IN ('Created', 'Uploading') GROUP BY u.context_id",
    )
    .fetch_all(&state.db_pool)
    .await
    {
        for (ctx, n) in rows {
            state.metrics.active_uploads_by_context
                .get_or_create(&ContextLabels { context: ctx }).set(n);
        }
    }

    if let Ok(rows) = sqlx::query_as::<_, CtxCount>(
        "SELECT COALESCE(c.slug, 'global'), COUNT(*) \
         FROM uploads u LEFT JOIN contexts c ON u.context_id = c.id \
         WHERE u.status = 'Processing' GROUP BY u.context_id",
    )
    .fetch_all(&state.db_pool)
    .await
    {
        for (ctx, n) in rows {
            state.metrics.processing_uploads_by_context
                .get_or_create(&ContextLabels { context: ctx }).set(n);
        }
    }

    let mut body = String::new();
    encode(&mut body, &state.metrics_registry).expect("metrics encoding failed");

    (
        [(
            axum::http::header::CONTENT_TYPE,
            "application/openmetrics-text; version=1.0.0; charset=utf-8",
        )],
        body,
    )
}
