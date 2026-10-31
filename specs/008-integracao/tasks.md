# Tasks - integração

**Input**: `spec.md`, `clarify.md`, `contracts.md`, `plan.md`

## Planejamento

- [x] T001 - Resolver Q-001 a Q-015 com a usuária; ajustar spec, contracts e desenho conforme as respostas.

## Portão técnico

- [x] T003 - Spike: `metrics` 0.24 + `metrics-exporter-prometheus` 0.18.3 com `OnceLock`; slot de operação preenchido por `Core::authorize` visível no middleware para REST e MCP. Registrar a decisão 0022.

## Dados

- [x] T002 - Migração `0008_integration.sql` (colunas do webhook, `audit_log_at`, `jobs_finished`); teste de migração.
- [x] T004 - `Store`: `webhook_directory`, app com webhook, `import_memories`, `usage_report`, `purge_before`, contagens de webhooks e do LLM em 24 h, tamanho do banco.

## Eventos e webhooks

- [x] T010 - `create_app`/`update_app` com `webhook_url` e `webhook_secret` (cifrado, só de escrita, auditoria sem segredo); `network_changed()` em app e tokens.
- [x] T011 - `EventDraft`, `WebhookDirectory`, `Core::recipients`, `journal::emit`.
- [x] T012 - Eventos em memórias (`move` com dois), canais, assinaturas, configurações, segredos e Redator.
- [x] T013 - Job `webhook` (assinatura, `skipped`, backoff próprio, `max_attempts`, lease, runner com concorrência 4); backoff por handler no runner.

## Importação, uso e status

- [x] T014 - `import_memories` (validação total, autores, duplicatas, revisão 1, jobs, um evento, `IDEMPOTENCY_KEY_REQUIRED`, `BATCH_TOO_LARGE`).
- [x] T015 - `get_usage` (intervalo, agrupamentos, total).
- [x] T016 - Amostrador do backup e `get_status` completo (uptime, tamanho, réplica, Redator 24 h, webhooks, retenção).
- [x] T017 - Retenção periódica em lotes com `schema_meta['retention.last_run']`.

## Métricas

- [x] T018 - `src/metrics.rs`, `/metrics` com `authorize_metrics`, métricas HTTP com operação, auth, jobs, LLM, embeddings, webhooks, banco, backup e retenção.

## Três vias e contrato

- [x] T020 - `OPERATIONS` (tag `Integration`, `get_usage` em `Status`), REST com `#[utoipa::path]`, `get_metrics`, MCP (`idempotency_key` obrigatório no import), CLI (`memories import`, `usage`, `apps … --webhook-*`).
- [x] T021 - `docs/openapi.json` (`UPDATE_SNAPSHOTS=1`), `ADMIN_TAGS`, `catalog.rs` (guia `integracao-gestor`).

## Testes

- [x] T030 - Unitários: HMAC, backoff, destinatários, `get_usage`, retenção.
- [x] T031 - Integração com servidor de webhook local (assinatura, retentativa, `failed`/`retry_job`, `skipped`, redirecionamento, timeout, destinatários do §19, `move`, Redator, idempotência, desligado).
- [x] T032 - Cobertura de eventos por operação de escrita de `OPERATIONS`.
- [x] T033 - Importação (100 itens, datas, autoria, duplicatas, tudo ou nada, limites, idempotência).
- [x] T034 - `/metrics` (401/403/200, séries esperadas), `get_status`, `get_usage`, retenção.
- [x] T035 - Matriz e contrato (operações novas, tipos de evento no guia, erros e configurações documentados).

## Operação

- [x] T040 - Guias (`integracao-gestor`, `integracao-consumidor`, `operacao`, `configuracoes`, `erros`, `tokens-e-escopos`, `cli`, `overview`) e skill.
- [x] T041 - `aicontext/webhooks.md`, `aicontext/{README,jobs,identities}.md`; `docs/design.md` §6/§12/§17/§20; decisões 0022–0024 (métricas, eventos e destinatários, retenção).
- [x] T042 - Versão `0.8.0`, `Cargo.lock`, `CHANGELOG.md`.
- [x] T043 - `implement.md` e `report.md`; verificação final (`fmt`, `clippy`, testes afetados, snapshot); scrape real no deploy.
