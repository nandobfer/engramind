//! Métricas Prometheus (desenho §20, decisão 0022), portadas do sap-middleman: recorder
//! instalado uma vez e exposição em `GET /metrics`, aqui protegida por token `admin`·`read`.
//! Os rótulos são sempre fechados (rota parametrizada, operação do catálogo, provedor,
//! finalidade, tipo); nunca slug, id, URL ou texto.

use std::sync::{Arc, OnceLock};

use axum::{extract::State, http::header};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use thiserror::Error;

use crate::{
    AppState,
    auth::Auth,
    core::status::MetricsSnapshot,
    error::{ApiError, ErrorResponse},
};

static METRICS_HANDLE: OnceLock<Result<PrometheusHandle, String>> = OnceLock::new();

#[derive(Debug, Error)]
pub enum MetricsError {
    #[error("falha ao inicializar metricas: {0}")]
    Install(String),
}

/// Instala o recorder global (uma vez por processo) e devolve o handle de exposição.
pub fn init() -> Result<PrometheusHandle, MetricsError> {
    match METRICS_HANDLE.get_or_init(|| {
        PrometheusBuilder::new()
            .install_recorder()
            .map_err(|error| error.to_string())
    }) {
        Ok(handle) => Ok(handle.clone()),
        Err(error) => Err(MetricsError::Install(error.clone())),
    }
}

/// Operação do catálogo atendida pela requisição, preenchida pelo REST (rota) ou pela
/// ferramenta MCP; o middleware de log usa como rótulo das métricas HTTP.
#[derive(Clone, Default)]
pub struct OperationSlot(Arc<OnceLock<&'static str>>);

impl OperationSlot {
    pub fn set(&self, operation: &'static str) {
        let _ = self.0.set(operation);
    }

    pub fn get(&self) -> Option<&'static str> {
        self.0.get().copied()
    }
}

pub fn record_http(method: &str, route: &str, operation: &str, status: u16, seconds: f64) {
    metrics::counter!(
        "http_requests_total",
        "method" => method.to_owned(),
        "route" => route.to_owned(),
        "status" => status.to_string(),
        "operation" => operation.to_owned()
    )
    .increment(1);
    metrics::histogram!(
        "http_request_duration_seconds",
        "method" => method.to_owned(),
        "route" => route.to_owned(),
        "operation" => operation.to_owned()
    )
    .record(seconds);
}

/// Motivo fechado de uma falha de autenticação.
pub fn auth_failure(reason: &'static str) {
    metrics::counter!("engramind_auth_failures_total", "reason" => reason).increment(1);
}

pub fn job_failed(kind: &'static str) {
    metrics::counter!("engramind_jobs_failed_total", "kind" => kind).increment(1);
}

pub fn llm_call(
    purpose: &str,
    provider: &str,
    outcome: &str,
    tokens: (Option<i64>, Option<i64>),
    cost_usd: Option<f64>,
) {
    metrics::counter!(
        "engramind_llm_calls_total",
        "purpose" => purpose.to_owned(),
        "provider" => provider.to_owned(),
        "outcome" => outcome.to_owned()
    )
    .increment(1);
    for (direction, count) in [("input", tokens.0), ("output", tokens.1)] {
        if let Some(count) = count.and_then(|count| u64::try_from(count).ok()) {
            metrics::counter!(
                "engramind_llm_tokens_total",
                "purpose" => purpose.to_owned(),
                "direction" => direction
            )
            .increment(count);
        }
    }
    if let Some(cost) = cost_usd.filter(|cost| cost.is_finite() && *cost > 0.0) {
        // Contadores do `metrics` são inteiros: o custo vai em micro-dólares.
        metrics::counter!(
            "engramind_llm_cost_microusd_total",
            "purpose" => purpose.to_owned(),
            "provider" => provider.to_owned()
        )
        .increment((cost * 1_000_000.0).round() as u64);
    }
}

pub fn embedding(provider: &str, kind: &'static str, seconds: f64, ok: bool) {
    metrics::histogram!(
        "engramind_embedding_duration_seconds",
        "provider" => provider.to_owned(),
        "kind" => kind
    )
    .record(seconds);
    if !ok {
        metrics::counter!(
            "engramind_embedding_failures_total",
            "provider" => provider.to_owned(),
            "kind" => kind
        )
        .increment(1);
    }
}

pub fn webhook_delivery(outcome: &'static str) {
    metrics::counter!("engramind_webhook_deliveries_total", "outcome" => outcome).increment(1);
}

pub fn retention_removed(table: &'static str, removed: u64) {
    metrics::counter!("engramind_retention_removed_total", "table" => table).increment(removed);
}

fn set_gauges(snapshot: &MetricsSnapshot) {
    metrics::gauge!("engramind_build_info", "version" => env!("CARGO_PKG_VERSION")).set(1.0);
    metrics::gauge!("service_ready").set(if snapshot.ready { 1.0 } else { 0.0 });
    for count in &snapshot.jobs {
        metrics::gauge!(
            "engramind_jobs",
            "kind" => count.kind.clone(),
            "state" => count.state.as_str()
        )
        .set(count.count as f64);
    }
    if let Some(size) = snapshot.database_size_bytes {
        metrics::gauge!("engramind_database_size_bytes").set(size as f64);
    }
    if let Some(age) = snapshot.replica_age_seconds {
        metrics::gauge!("engramind_backup_replica_age_seconds").set(age as f64);
    }
}

#[utoipa::path(
    get,
    path = "/metrics",
    operation_id = "get_metrics",
    tag = "Operations",
    summary = "Expõe as métricas Prometheus",
    description = "Formato de exposição de texto do Prometheus. Exige token admin com `read` (o Prometheus usa `authorization.credentials`). Rótulos sem slugs, ids nem URLs; nomes e consultas no guia `operacao`.",
    security(("bearerAuth" = [])),
    responses(
        (status = 200, description = "Exposição de texto do Prometheus", content_type = "text/plain", body = String),
        (status = 401, description = "`AUTH_INVALID`", body = ErrorResponse),
        (status = 403, description = "`SCOPE_REQUIRED`", body = ErrorResponse),
        (status = 429, description = "`RATE_LIMITED`", body = ErrorResponse),
        (status = 503, description = "`DATABASE_UNAVAILABLE`", body = ErrorResponse),
        (status = 504, description = "`REQUEST_TIMEOUT`", body = ErrorResponse)
    )
)]
pub async fn render(
    State(state): State<AppState>,
    Auth(principal): Auth,
) -> Result<([(header::HeaderName, &'static str); 1], String), ApiError> {
    let snapshot = state.core.metrics_snapshot(&principal).await?;
    let handle = init().map_err(|_| ApiError::internal())?;
    set_gauges(&snapshot);
    Ok((
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        handle.render(),
    ))
}
