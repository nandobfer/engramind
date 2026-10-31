# Fila de jobs

## Onde vive

- `src/core/jobs/mod.rs`: `JobOutcome`, `list_jobs`, `retry_job`, `list_llm_calls`.
- `src/core/jobs/runner.rs`: trait `JobHandler` (`KIND`, `lease`, `run`), `spawn` (runner em segundo plano), `drain` e `Core::spawn_workers` / `Core::run_redact_jobs(now)`.
- `src/store/sqlite/jobs.rs`: captura, fim, superados, lista, retry, contagens, `secrets` e `llm_calls`.
- Adaptadores: `src/rest/jobs.rs` (tag `Jobs`, admin), ferramentas MCP de mesmo nome, `engramind jobs list|retry` e `engramind llm-calls list`.

## Regras

- Jobs nascem na mesma transação da mutação (`Store`), com payload só de ids e versões (`memory_id`, `version`, `title_only`). Depois do commit, chame `Core::jobs_enqueued()` para acordar os runners.
- Captura só por `Store::claim_job(kind, now, lease_until)`: `queued` pronto ou `running` com lease vencido, mais antigo primeiro, soma a tentativa. Nunca capture com `SELECT` + `UPDATE` separados.
- `finish_job` só vale para `running`; `false` significa que outro executor retomou o job (log `jobs.lease_lost`).
- Handler devolve `Done(outcome)`, `Superseded`, `Retry(erro)` ou `Fail(erro)`. Quem decide `Fail` é o handler (sabe `max_attempts`); o `ERROR jobs.failed` é do runner, uma vez. `Retry` usa `redator::backoff(attempts)`.
- Erros gravados em `last_error` são curtos e nunca têm texto de memória, prompt ou segredo.
- Tipo novo de job: um `JobHandler`, um runner em `spawn_workers` e a concorrência no código. Testes chamam `drain` via um método `Core::run_*_jobs(now)`.
- `retry_job` só para `failed` (`409 JOB_NOT_RETRYABLE`), com auditoria `job.retry`.

Desenho §6; decisão 0016; guia `operacao`.
