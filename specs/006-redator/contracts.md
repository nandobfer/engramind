# Contracts - Redator

**Criado em**: 2026-10-30
**Status**: Implementado (0.6.0)
**Feature**: [`spec.md`](spec.md)

## Migração `0006_redator.sql`

```sql
secrets   (key TEXT PK, value_enc BLOB, updated_at)
llm_calls (id INTEGER PK, at, purpose, provider, model, input_tokens NULL, output_tokens NULL,
           cost_usd REAL NULL, duration_ms, outcome, error NULL)
CREATE INDEX llm_calls_at ON llm_calls(at);
ALTER TABLE jobs ADD COLUMN outcome TEXT;            -- applied, title_only, superseded, skipped
CREATE INDEX jobs_kind_ready ON jobs (kind, state, run_after);
```

## Core

```rust
// src/providers/llm
pub trait LlmClient: Send + Sync {
    fn complete_json(&self, request: LlmRequest) -> impl Future<Output = Result<LlmResponse, LlmError>> + Send;
}
pub enum LlmError { Timeout, RateLimited, Unavailable, Rejected(String), InvalidResponse }
// src/core/jobs/runner.rs (o handler recebe o `Core`; `Done` carrega o `outcome`)
pub trait JobHandler<S: Store> {
    const KIND: &'static str;
    fn lease(&self, core: &Core<S>) -> Duration;
    fn run(&self, core: &Core<S>, job: Job) -> impl Future<Output = JobOutcome> + Send;
}
pub enum JobOutcome { Done(&'static str), Superseded, Retry(String), Fail(String) }
// src/core/redator/guard.rs
pub trait MeaningGuard: Send + Sync { fn similarity<'a>(&'a self, original: &'a str, revised: &'a str) -> GuardFuture<'a>; }
```

`Store` ganha `claim_job`, `finish_job`, `newer_job_exists`, `list_jobs`, `retry_job`, `job_counts`, `get_secret`, `list_secrets`, `set_secret`, `append_llm_calls`, `list_llm_calls`, `llm_activity`, `apply_redaction`, `set_title_fallback` e `memory_content_hash`.

```text
```

## Operações

| Operação | Tipo·escopo | REST | CLI |
|---|---|---|---|
| `set_secret` | admin·write | `PUT /v1/secrets/{key}` `{ value }` | `engramind secrets set <chave>` (lê o valor da entrada padrão; `--delete`) |
| `test_redator` | admin·write | `POST /v1/settings/redator/test` `{ text, settings? }` | `engramind settings test-redator --text … [--settings <json>]` |
| `list_jobs` | admin·read | `GET /v1/jobs?state=&kind=` | `engramind jobs list` |
| `retry_job` | admin·write | `POST /v1/jobs/{job}/retry` | `engramind jobs retry <id>` |
| `list_llm_calls` | admin·read | `GET /v1/llm-calls?since=&purpose=` | `engramind llm-calls list` |

`get_settings` passa a listar segredos como `{ key, secret: true, configured, updated_at }`.

## Configurações desta etapa

| Chave | Tipo | Padrão | Por canal |
|---|---|---|---|
| `redator.enabled` | booleano | true | não |
| `redator.provider` | `openai_compatible`/`anthropic`/`gemini` | — | não |
| `redator.model` | texto | — | não |
| `redator.api_key` 🔒 | segredo | — | não |
| `redator.base_url` | URL | — | não |
| `redator.instructions` | texto | — | sim |
| `redator.rewrite_text` | booleano | true | sim |
| `redator.title_max` | inteiro | 100 | não |
| `redator.min_similarity` | número 0–1 | 0.8 | não |
| `redator.timeout_ms` | inteiro | 30000 | não |
| `redator.max_attempts` | inteiro | 3 | não |
| `redator.fallback_models` | lista de texto | [] | não |

## Códigos de erro novos

| Código | Status |
|---|---|
| `SETTING_IS_SECRET` | 400 |
| `SECRET_UNKNOWN` | 400 |
| `JOB_NOT_FOUND` | 404 |
| `JOB_NOT_RETRYABLE` | 409 |
| `PROVIDER_NOT_CONFIGURED` | 400 |
| `PROVIDER_FAILED` | 502 |
| `PROVIDER_TIMEOUT` | 504 |
