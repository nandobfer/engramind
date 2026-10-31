use std::{env, io, time::Instant};

use axum::{
    extract::{MatchedPath, Request},
    http::{HeaderValue, Method},
    middleware::Next,
    response::Response,
};
use thiserror::Error;
use tracing::{Instrument, field};
use tracing_subscriber::{
    EnvFilter,
    fmt::{time::ChronoLocal, writer::BoxMakeWriter},
    layer::SubscriberExt,
    util::SubscriberInitExt,
};
use uuid::Uuid;

use crate::{
    core::operations,
    http::REQUEST_ID_HEADER,
    metrics::{self, OperationSlot},
};

const TIMESTAMP_FORMAT: &str = "%d/%m/%Y - %H:%M:%S";
const DEFAULT_FILTER: &str = "engramind=info,tower_http=warn";

#[derive(Clone, Copy)]
enum LogFormat {
    Compact,
    Json,
}

/// Destino dos logs: o servidor escreve no stdout (logs do contêiner); os comandos
/// locais da CLI escrevem no stderr para não misturar logs com a saída do comando.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogTarget {
    Stdout,
    Stderr,
}

#[derive(Debug, Error)]
pub enum ObservabilityError {
    #[error("LOG_FORMAT invalido: use 'compact' ou 'json', recebido '{0}'")]
    InvalidFormat(String),
    #[error("LOG_ANSI invalido: use 'true' ou 'false', recebido '{0}'")]
    InvalidAnsi(String),
    #[error("RUST_LOG invalido: {0}")]
    InvalidFilter(String),
    #[error("falha ao inicializar observabilidade: {0}")]
    Init(String),
}

pub fn init_logging(target: LogTarget) -> Result<(), ObservabilityError> {
    let format = parse_format(env::var("LOG_FORMAT").ok().as_deref())?;
    let ansi = parse_ansi(env::var("LOG_ANSI").ok().as_deref())?;
    let filter = log_filter(env::var("RUST_LOG").as_deref().unwrap_or(DEFAULT_FILTER))?;
    let writer = || match target {
        LogTarget::Stdout => BoxMakeWriter::new(io::stdout),
        LogTarget::Stderr => BoxMakeWriter::new(io::stderr),
    };

    match format {
        LogFormat::Compact => tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .compact()
                    .with_timer(ChronoLocal::new(TIMESTAMP_FORMAT.to_owned()))
                    .with_ansi(ansi)
                    .with_target(false)
                    .with_file(false)
                    .with_line_number(false)
                    .with_writer(writer()),
            )
            .try_init()
            .map_err(|error| ObservabilityError::Init(error.to_string()))?,
        LogFormat::Json => tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_timer(ChronoLocal::new(TIMESTAMP_FORMAT.to_owned()))
                    .with_writer(writer()),
            )
            .try_init()
            .map_err(|error| ObservabilityError::Init(error.to_string()))?,
    }
    Ok(())
}

pub async fn assign_request_id(mut request: Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get(&REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Uuid::parse_str(value).ok())
        .unwrap_or_else(Uuid::new_v4);
    let header = HeaderValue::from_str(&request_id.to_string()).expect("UUID is a valid header");
    request
        .headers_mut()
        .insert(REQUEST_ID_HEADER.clone(), header.clone());
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(REQUEST_ID_HEADER.clone(), header);
    response
}

pub async fn log_request(mut request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("<unmatched>", MatchedPath::as_str)
        .to_owned();
    let request_id = request
        .headers()
        .get(&REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown")
        .to_owned();
    let noisy = is_noisy(&method, &path);
    // Rótulo `operation` das métricas: a rota REST do catálogo ou a ferramenta MCP.
    let slot = OperationSlot::default();
    if let Some(operation) = operations::by_route(method.as_str(), &route) {
        slot.set(operation.name);
    }
    request.extensions_mut().insert(slot.clone());
    let span = tracing::info_span!(
        "http.request",
        request_id = %request_id,
        method = %method,
        route = %route,
        app = field::Empty,
        agent = field::Empty,
        token_id = field::Empty,
        operation = field::Empty,
    );
    let started = Instant::now();
    let response = next.run(request).instrument(span.clone()).await;
    let latency_ms = started.elapsed().as_millis() as u64;
    let status = response.status().as_u16();
    metrics::record_http(
        method.as_str(),
        &route,
        slot.get().unwrap_or(""),
        status,
        started.elapsed().as_secs_f64(),
    );

    // `get_context` com `304` é o poll de cada turno (005 FR-010).
    if (noisy && status < 500) || status == 304 {
        tracing::debug!(parent: &span, event = "http.request.completed", status, latency_ms, "requisicao concluida");
    } else if status >= 500 {
        tracing::error!(parent: &span, event = "http.request.completed", status, latency_ms, "requisicao concluida com falha");
    } else {
        tracing::info!(parent: &span, event = "http.request.completed", status, latency_ms, "requisicao concluida");
    }
    response
}

/// Third-party targets whose verbose levels record request payloads. They are
/// capped regardless of `RUST_LOG`: `rmcp` logs full JSON-RPC messages,
/// including MCP tool arguments, at `DEBUG`.
const REDACTED_TARGET_DIRECTIVES: &[&str] = &["rmcp=warn"];

/// Builds the application filter from a `RUST_LOG`-style expression with the
/// mandatory payload redaction caps applied.
pub fn log_filter(directives: &str) -> Result<EnvFilter, ObservabilityError> {
    EnvFilter::try_new(directives)
        .map(with_mandatory_redaction)
        .map_err(|error| ObservabilityError::InvalidFilter(error.to_string()))
}

fn with_mandatory_redaction(mut filter: EnvFilter) -> EnvFilter {
    for directive in REDACTED_TARGET_DIRECTIVES {
        filter = filter.add_directive(directive.parse().expect("static log directive"));
    }
    filter
}

fn parse_format(value: Option<&str>) -> Result<LogFormat, ObservabilityError> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("compact") => Ok(LogFormat::Compact),
        Some("json") => Ok(LogFormat::Json),
        Some(value) => Err(ObservabilityError::InvalidFormat(value.to_owned())),
    }
}

fn parse_ansi(value: Option<&str>) -> Result<bool, ObservabilityError> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(value) => Err(ObservabilityError::InvalidAnsi(value.to_owned())),
    }
}

fn is_noisy(method: &Method, path: &str) -> bool {
    *method == Method::OPTIONS
        || path == "/"
        || path.starts_with("/health/")
        || path.starts_with("/docs")
        || path == "/openapi.json"
        || path == "/metrics"
        || path == "/mcp"
}

#[cfg(test)]
mod tests {
    use std::{
        io::{self, Write},
        sync::{Arc, Mutex},
    };

    use super::*;
    use axum::{Router, body::Body, http::Request, middleware, routing::get};
    use tower::ServiceExt as _;
    use tracing::Span;
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    struct BufferWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for BufferWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .expect("lock log buffer")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Buffer {
        type Writer = BufferWriter;

        fn make_writer(&'a self) -> Self::Writer {
            BufferWriter(self.0.clone())
        }
    }

    impl Buffer {
        fn output(&self) -> String {
            String::from_utf8(self.0.lock().expect("lock log buffer").clone())
                .expect("UTF-8 log output")
        }
    }

    #[test]
    fn parses_supported_log_configuration() {
        assert!(matches!(parse_format(None), Ok(LogFormat::Compact)));
        assert!(matches!(parse_format(Some("json")), Ok(LogFormat::Json)));
        assert!(parse_format(Some("xml")).is_err());
        assert!(parse_ansi(None).expect("default ANSI"));
        assert!(!parse_ansi(Some("false")).expect("disabled ANSI"));
        assert!(parse_ansi(Some("yes")).is_err());
        assert!(log_filter(DEFAULT_FILTER).is_ok());
        assert!(log_filter("engramind=nivel-invalido").is_err());
    }

    #[test]
    fn payload_logging_targets_stay_capped_even_when_rust_log_enables_them() {
        let buffer = Buffer::default();
        let filter = with_mandatory_redaction(EnvFilter::new("trace,rmcp=trace"));
        let subscriber = tracing_subscriber::registry().with(filter).with(
            tracing_subscriber::fmt::layer()
                .with_writer(buffer.clone())
                .with_ansi(false),
        );
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!(target: "rmcp::service", request = "tool-arguments", "received request");
            tracing::warn!(target: "rmcp::transport", "rejected host");
            tracing::debug!(target: "engramind::store", "own debug event");
        });
        let output = buffer.output();
        assert!(!output.contains("tool-arguments"));
        assert!(output.contains("rejected host"));
        assert!(output.contains("own debug event"));
    }

    #[test]
    fn classifies_noisy_operational_routes() {
        assert!(is_noisy(&Method::GET, "/health/live"));
        assert!(is_noisy(&Method::GET, "/"));
        assert!(is_noisy(&Method::GET, "/docs/"));
        assert!(is_noisy(&Method::GET, "/openapi.json"));
        assert!(is_noisy(&Method::GET, "/metrics"));
        assert!(is_noisy(&Method::POST, "/mcp"));
        assert!(is_noisy(&Method::OPTIONS, "/v1/memories"));
        assert!(!is_noisy(&Method::POST, "/v1/memories"));
    }

    #[test]
    fn compact_and_json_formats_are_single_line_and_structured() {
        let compact = Buffer::default();
        let compact_subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .compact()
                .with_timer(ChronoLocal::new(TIMESTAMP_FORMAT.to_owned()))
                .with_ansi(false)
                .with_writer(compact.clone()),
        );
        tracing::subscriber::with_default(compact_subscriber, || {
            tracing::info!(event = "test.completed", app = "safe-id", "concluido");
        });
        let compact_output = compact.output();
        assert_eq!(compact_output.lines().count(), 1);
        assert!(compact_output.contains("test.completed"));
        assert!(compact_output.contains("safe-id"));
        assert!(compact_output.contains(" - "));

        let json = Buffer::default();
        let json_subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .json()
                .without_time()
                .with_writer(json.clone()),
        );
        tracing::subscriber::with_default(json_subscriber, || {
            tracing::info!(event = "test.completed", app = "safe-id", "concluido");
        });
        let json_output = json.output();
        assert_eq!(json_output.lines().count(), 1);
        let parsed: serde_json::Value =
            serde_json::from_str(json_output.trim()).expect("valid JSON log");
        assert_eq!(parsed["fields"]["event"], "test.completed");
        assert_eq!(parsed["fields"]["app"], "safe-id");
    }

    #[test]
    fn compact_format_can_emit_ansi() {
        let buffer = Buffer::default();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .compact()
                .without_time()
                .with_ansi(true)
                .with_writer(buffer.clone()),
        );
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(event = "test.completed", "concluido");
        });

        assert!(buffer.output().contains("\u{1b}["));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn request_log_uses_route_and_redacts_request_content() {
        async fn handler() {
            let span = Span::current();
            span.record("app", field::display("safe-app"));
            span.record("agent", field::display("safe-agent"));
            span.record("token_id", field::display("safe-token-id"));
        }

        let buffer = Buffer::default();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .compact()
                .without_time()
                .with_ansi(false)
                .with_writer(buffer.clone()),
        );
        let app = Router::new()
            .route("/items/{item_id}", get(handler))
            .layer(middleware::from_fn(log_request))
            .layer(middleware::from_fn(assign_request_id));
        let request = Request::builder()
            .uri("/items/concrete-sensitive-value?token=query-secret")
            .header("authorization", "Bearer header-secret")
            .body(Body::from("body-secret"))
            .expect("request");

        let _guard = tracing::subscriber::set_default(subscriber);
        app.oneshot(request).await.expect("response");

        let output = buffer.output();
        assert_eq!(output.matches("http.request.completed").count(), 1);
        assert!(output.contains("/items/{item_id}"));
        assert!(output.contains("safe-app"));
        assert!(output.contains("safe-agent"));
        assert!(output.contains("safe-token-id"));
        for sensitive in [
            "concrete-sensitive-value",
            "query-secret",
            "header-secret",
            "body-secret",
        ] {
            assert!(!output.contains(sensitive));
        }
    }
}
