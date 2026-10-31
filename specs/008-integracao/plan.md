# Plan - integração

**Data**: 2026-10-31
**Referência de spec**: [`spec.md`](spec.md)

## Resumo

Eventos e webhooks em todas as mutações de memória, canal, assinatura e configuração; `import_memories`; `/metrics`; `get_status` completo; `get_usage`; retenção — nas três vias, com os guias de integração finais.

## Contexto técnico

- Toda mutação passa por `journal::audit`/`journal::remember` dentro da transação do `Store` (65 chamadas em `src/store/sqlite/{memories,channels,identities,settings,jobs,vectors}.rs`): é ali que `journal::emit` entra, sem SQL de destinatário.
- O acesso efetivo (`core::access`, `subscription_pattern`) e a rede em cache (`Network`, geração com `network_changed()`) já existem; falta o diretório agente → app, URLs e apps com token `admin`.
- `Vault` (AES-256-GCM com AAD) cifra o segredo do webhook; `hmac` e `sha2` já são dependências; o cliente HTTP do `Core` (`providers::llm::http_client`) já não segue redirecionamentos.
- O runner de jobs usa o backoff do Redator para todos os tipos; precisa de backoff por handler.
- `doctor::backup_check` roda `litestream ltx`; vira a fonte do amostrador.
- Novas dependências: `metrics` 0.24 e `metrics-exporter-prometheus` 0.18.3 (as do sap-middleman), decisão 0022.

## Áreas afetadas

**Core**: `src/core/events.rs` (novo: tipos, `EventDraft`, destinatários, diretório), `src/core/webhooks.rs` (job, assinatura, backoff), `src/core/import.rs`, `src/core/usage_report.rs` (`get_usage`; `usage.rs` é o rate limiter), `src/core/retention.rs`, `src/core/status.rs`, `src/core/background.rs` (retenção e amostrador do backup), `src/core/jobs/runner.rs` (backoff por handler, runner `webhook`, contador de falhas), `src/core/{memories,channels,subscriptions,settings,secrets,identities}.rs` (eventos e `network_changed`), `src/core/redator/mod.rs`, `src/core/settings.rs` (catálogo), `src/core/operations.rs`, `src/core/model.rs`, `src/core/mod.rs` (`started_at`, slot de operação).
**Adaptadores**: `src/rest/{memories,identities,status}.rs`, `src/app.rs` (`/metrics`), `src/metrics.rs` (novo, portado), `src/observability.rs` (métricas HTTP), `src/auth/middleware.rs` (falhas), `src/openapi.rs`; `src/mcp/tools.rs`; `src/cli/{mod,remote,help}.rs`.
**Dados**: `migrations/0008_integration.sql`; `src/store/mod.rs`; `src/store/sqlite/{journal,identities,memories,channels,settings,jobs,mod}.rs`; `src/store/sqlite/usage.rs` e `retention.rs` (novos).
**Docs**: `docs/guides/integracao-gestor.md` (novo), `integracao-consumidor.md`, `operacao.md`, `configuracoes.md`, `erros.md`, `tokens-e-escopos.md` (webhook do app), `cli.md`, `overview.md`; `skills/engramind/SKILL.md`; `aicontext/webhooks.md` (novo), `aicontext/README.md`, `aicontext/jobs.md`, `aicontext/identities.md`; `docs/design.md` §6/§12/§17/§20; `docs/decisions/0022`–`0024`; `CHANGELOG.md`.

## Estratégia de implementação

1. **T001** resolver Q-001 a Q-015 e ajustar spec/contracts.
2. **Spike T003 (portão)**: `metrics` + exporter instalados uma vez; confirmar que o slot de operação preenchido em `Core::authorize` chega ao middleware tanto no REST quanto nas ferramentas MCP (rmcp stateless pode executar a ferramenta em outra task; se perder, o slot vai numa extensão da requisição clonada no `Principal`).
3. Migração `0008`; `Store::webhook_directory`, colunas do app, `update_app`/`create_app` com URL e segredo cifrado.
4. Eventos: `EventDraft`, `Core::recipients`, `journal::emit`; ligar em memórias (inclusive `move` com dois eventos), canais, assinaturas, configurações, segredos e Redator. Teste de cobertura por `OPERATIONS` antes de ligar as últimas mutações, para ele guiar o trabalho.
5. Job `webhook`: entrega assinada, `skipped`, backoff próprio, `max_attempts` da configuração, contador de entregas.
6. `import_memories` (validação total, autores, duplicatas, revisão 1, jobs, um evento).
7. Métricas: HTTP com operação, auth, jobs, LLM, embeddings, webhooks, banco, backup; `/metrics` com `authorize_metrics`.
8. Amostrador do backup; `get_status` completo; `get_usage`.
9. Retenção periódica em lotes, `schema_meta['retention.last_run']`, relatório em `get_status`.
10. Três vias (REST/OpenAPI, MCP, CLI), tags (`Integration`), guias, skill, contrato e matriz.
11. `aicontext`, desenho, decisões, versão 0.8.0.

## Estratégia de testes

**Unitários**
- Assinatura HMAC com vetor conhecido; formato `t=…,v1=…`.
- Backoff do webhook (10 s, 40 s, …, teto 1 h).
- Destinatários: agente assinante (exato e prefixo), agente desativado, app desativado, app sem URL, admin ativo/revogado/expirado, `subscription.updated` após `unsubscribe`, `settings.updated` global só para admin.
- `get_usage`: limites de intervalo, `group_by` inválido.
- Retenção: corte por dia, `audit_days = null`, jobs `queued` preservados.

**Integração (`tests/it/webhooks.rs`, `tests/it/integration.rs` + `support/webhook.rs`)**
- Servidor de webhook de teste (no molde de `support/llm.rs`): grava corpo e cabeçalhos, responde roteirizado.
- Assinatura verificada com o segredo; `500` → retentativa com o mesmo `id`/corpo; esgotar → `failed` e `retry_job`; `skipped` após remover a URL; redirecionamento não seguido; timeout.
- Destinatários de ponta a ponta (cenário do §19): `app-a` assinante, `app-b` sem assinatura, gestor admin.
- `move_memory` com dois eventos e versões de cada canal; Redator gerando `memory.updated`; idempotência repetida sem jobs novos; `webhooks.enabled = false`.
- Cobertura: percorre as operações de escrita de `OPERATIONS` (fora as exceções do contrato) e confere o job `webhook` enfileirado para um app admin com URL.
- Importação: 100 memórias, datas e autoria preservadas, duplicatas no canal e no lote, tudo ou nada com item inválido, `BATCH_TOO_LARGE`, `IDEMPOTENCY_KEY_REQUIRED` nas três vias, repetição idempotente, um único `memory.created`, jobs `redact`/`embed`.
- `/metrics`: `401` sem token, `403` com `agent`, `200` com `admin`·`read`; presença de `http_requests_total{operation="get_context",status="304"}` e de `engramind_jobs`.
- `get_status` com webhooks, retenção, banco e backup desligado; `get_usage` nos quatro agrupamentos.
- Retenção com datas antigas inseridas por `Store` de teste.

**Contrato/matriz**
- `import_memories` e `get_usage` em `OPERATIONS`, OpenAPI, MCP, CLI e guia; `get_metrics` como rota técnica.
- Todo tipo de evento citado em `integracao-gestor`; configurações e códigos de erro novos documentados.
- `tests/it/matrix.rs`: casos para as duas operações novas.

## Observabilidade

- Eventos de log: `webhooks.delivered` (`DEBUG`), `webhooks.retry` (`WARN`, host e status), `jobs.failed` (já existe), `import.completed` (`INFO`, canal e contagens), `retention.purged` (`INFO` só com remoções), `backup.sample_failed` (`WARN`).
- `/metrics` e `get_status` com `304`/sucesso em `DEBUG` (rota ruidosa, como no sap-middleman).

## Riscos

| Risco | Mitigação |
|-------|-----------|
| Esquecer o evento numa mutação | Teste de cobertura por `OPERATIONS` com lista explícita de exceções (contracts.md). |
| Destinatário lento travar a fila | Runner próprio do `webhook` (concorrência 4), timeout curto, lease = timeout + 30 s; não compartilha permissão com `redact`/`embed`. |
| Diretório de webhooks desatualizado | Mesma geração da rede; `network_changed()` nas mutações de token e app; expiração de token avaliada no enfileiramento. |
| `jobs` crescendo com um job por destinatário | `retention.jobs_days` (Q-008) e índice `jobs_finished`. |
| SSRF por `webhook_url` | Só admin define; http/https sem credenciais; sem redirecionamento; host nos logs; documentado que a URL é de confiança do operador. |
| Cardinalidade das métricas | Rótulos fechados (rota parametrizada, operação do catálogo, provedor, finalidade); nunca slug, id ou URL. |
| Recorder global entre testes | nextest usa um processo por teste; `init()` idempotente com `OnceLock`. |
| Retenção segurando o escritor | Lotes de 5.000 linhas por transação, uma tabela por vez. |
| Importação grande no escritor único | Limite `limits.max_batch` (100) e uma transação; Redator/embeddings assíncronos. |

## Versão e entrega

- `0.8.0` em `Cargo.toml`/`Cargo.lock`; `CHANGELOG.md` com a etapa 008.
- `docs/openapi.json` regenerado com `UPDATE_SNAPSHOTS=1`.
- `implement.md` e `report.md` ao final.
