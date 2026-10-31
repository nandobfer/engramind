//! Entrega dos eventos (desenho §17, 008 FR-005/FR-006): job `webhook`, um por destinatário,
//! com `POST` assinado por HMAC-SHA256, retentativa com backoff próprio e `skipped` quando o
//! app deixou de receber.

use std::time::Duration;

use hmac::{Hmac, Mac};
use sha2::Sha256;

use super::{
    Core,
    identities::webhook_secret_name,
    jobs::JobOutcome,
    model::Job,
    settings::{WEBHOOKS_ENABLED, WEBHOOKS_MAX_ATTEMPTS, WEBHOOKS_TIMEOUT_MS},
};
use crate::{metrics, store::Store};

pub const JOB_KIND: &str = "webhook";
/// Entregas em paralelo (Q-006): um destinatário lento não segura os outros.
pub const WEBHOOK_CONCURRENCY: usize = 4;
/// Folga do lease além do prazo da tentativa.
const LEASE_MARGIN: Duration = Duration::from_secs(30);
const BACKOFF_BASE: Duration = Duration::from_secs(10);
const BACKOFF_MAX: Duration = Duration::from_secs(3600);

pub const SIGNATURE_HEADER: &str = "Engramind-Signature";
pub const EVENT_ID_HEADER: &str = "Engramind-Event-Id";
pub const EVENT_TYPE_HEADER: &str = "Engramind-Event-Type";

/// `10 s × 4^(n-1)`, até 1 h (Q-006): 10 s, 40 s, 160 s, 640 s, 2560 s, 3600 s…
pub fn backoff(attempts: i64) -> Duration {
    let exponent = u32::try_from(attempts.saturating_sub(1).clamp(0, 10)).unwrap_or(10);
    BACKOFF_BASE
        .saturating_mul(4u32.saturating_pow(exponent))
        .min(BACKOFF_MAX)
}

/// `t=<unix>,v1=<hex(HMAC-SHA256(segredo, t + "." + corpo))>`.
pub fn signature(secret: &[u8], timestamp: i64, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC aceita qualquer chave");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    let digest = mac.finalize().into_bytes();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("t={timestamp},v1={hex}")
}

/// Classe curta de uma falha de rede, sem a URL (D-004).
fn failure_class(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_redirect() {
        "redirect"
    } else {
        "request"
    }
}

fn host(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_default()
}

impl<S: Store> Core<S> {
    pub(crate) fn webhook_lease(&self) -> Duration {
        let timeout =
            u64::try_from(self.settings_snapshot().whole(WEBHOOKS_TIMEOUT_MS)).unwrap_or(5000);
        Duration::from_millis(timeout) + LEASE_MARGIN
    }

    /// Uma tentativa de entrega do job.
    pub(crate) async fn run_webhook(&self, job: &Job) -> JobOutcome {
        let outcome = self.deliver(job).await;
        metrics::webhook_delivery(match &outcome {
            JobOutcome::Done("delivered") => "delivered",
            JobOutcome::Done(_) | JobOutcome::Superseded => "skipped",
            JobOutcome::Retry(_) => "retry",
            JobOutcome::Fail(_) => "failed",
        });
        outcome
    }

    async fn deliver(&self, job: &Job) -> JobOutcome {
        let (Some(app_id), Some(event)) = (
            job.payload["app_id"].as_str(),
            job.payload.get("event").filter(|event| event.is_object()),
        ) else {
            return JobOutcome::Fail("payload invalido".to_owned());
        };
        let snapshot = self.settings_snapshot();
        if !snapshot.boolean(WEBHOOKS_ENABLED) {
            return JobOutcome::Done("skipped");
        }
        let target = match self.store().webhook_target(app_id.to_owned()).await {
            Ok(Some(target)) => target,
            Ok(None) => return JobOutcome::Done("skipped"),
            Err(error) => return JobOutcome::Retry(error.to_string()),
        };
        let (false, Some(url), Some(sealed)) = (target.disabled, target.url, target.secret_enc)
        else {
            return JobOutcome::Done("skipped");
        };
        let secret = match self
            .vault()
            .open(&webhook_secret_name(&target.slug), &sealed)
        {
            Ok(secret) => secret,
            Err(_) => return JobOutcome::Fail("segredo do webhook nao decifra".to_owned()),
        };
        let max_attempts = snapshot.whole(WEBHOOKS_MAX_ATTEMPTS).max(1);
        let timeout = Duration::from_millis(
            u64::try_from(snapshot.whole(WEBHOOKS_TIMEOUT_MS)).unwrap_or(5000),
        );
        let body = event.to_string().into_bytes();
        let timestamp = chrono::Utc::now().timestamp();
        let event_id = event["id"].as_str().unwrap_or_default();
        let event_type = event["type"].as_str().unwrap_or_default();
        let response = self
            .http()
            .post(&url)
            .timeout(timeout)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::USER_AGENT,
                concat!("engramind/", env!("CARGO_PKG_VERSION")),
            )
            .header(EVENT_ID_HEADER, event_id)
            .header(EVENT_TYPE_HEADER, event_type)
            .header(
                SIGNATURE_HEADER,
                signature(secret.as_bytes(), timestamp, &body),
            )
            .body(body)
            .send()
            .await;
        let failure = match response {
            Ok(response) if response.status().is_success() => {
                tracing::debug!(
                    event = "webhooks.delivered",
                    job_id = job.id,
                    event_type,
                    host = %host(&url),
                    attempt = job.attempts,
                    "evento entregue"
                );
                return JobOutcome::Done("delivered");
            }
            Ok(response) => format!("http {}", response.status().as_u16()),
            Err(error) => failure_class(&error).to_owned(),
        };
        if job.attempts >= max_attempts {
            return JobOutcome::Fail(failure);
        }
        tracing::warn!(
            event = "webhooks.retry",
            job_id = job.id,
            event_type,
            host = %host(&url),
            attempt = job.attempts,
            max_attempts,
            reason = %failure,
            "entrega do evento falhou; nova tentativa com backoff"
        );
        JobOutcome::Retry(failure)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_by_four_up_to_an_hour() {
        let seconds: Vec<u64> = (1..=7).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(seconds, [10, 40, 160, 640, 2560, 3600, 3600]);
        assert_eq!(backoff(0).as_secs(), 10);
        assert_eq!(backoff(i64::MAX).as_secs(), 3600);
    }

    #[test]
    fn signature_is_the_hmac_of_timestamp_dot_body() {
        // HMAC-SHA256("segredo", "1700000000.{}"), conferido com openssl.
        assert_eq!(
            signature(b"segredo", 1_700_000_000, b"{}"),
            "t=1700000000,v1=28d73844c84580182772a1e98a60aeb8f04184b1bef0d4e147cfa59ebf0bfedc"
        );
    }
}
