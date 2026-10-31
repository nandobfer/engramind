# Changelog

Todas as mudanças relevantes do Engramind. Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/); versões em [SemVer](https://semver.org/lang/pt-BR/).

## [0.6.0] - 2026-10-31

Etapa 006 — Redator.

### Adicionado

- Fila de jobs com runner do `redact` (2 em paralelo), acordado no commit e a cada segundo: captura por `UPDATE … RETURNING` com lease (`locked_until`), lease vencido volta à fila, backoff de 2 s × 2ⁿ (teto de 10 min) até `redator.max_attempts`, `failed` com um único `ERROR jobs.failed`, e jobs superados (memória excluída ou `redact` mais novo) terminando sem efeito. O backlog enfileirado desde a 0.4.0 é processado. Coluna `jobs.outcome` (`applied`, `title_only`, `superseded`, `skipped`).
- Redator: o texto vai ao LLM como dado, entre marcas aleatórias, com `redator.instructions` (global ou do canal) no prompt do sistema; resposta validada pelo serviço (`redator.title_max`, `memory.text_min`/`text_max`). O título aplicado vira `title_state = ready` e incrementa a `version` do canal (e a do contexto), sem revisão nem mudança na `version` da memória. O texto revisado (com `redator.rewrite_text`) só vale com a guarda de sentido, que até a 007 falha fechada; aplicado, gera uma revisão `system/redator` (`reason = redact`) e um job `embed`. Desligado ou sem configuração: `title_state = fallback`, sem chamada.
- Cliente LLM próprio (`reqwest`) para OpenAI-compatível (`json_schema`, com JSON livre quando o servidor não aceita), Anthropic (tool forçada) e Gemini (`responseSchema`), com tempo limite por chamada e `redator.fallback_models` em indisponibilidade, `429` ou tempo limite.
- `llm_calls` (finalidade, provedor, modelo, tokens, custo quando informado pelo provedor, duração, resultado e erro curto; nunca texto, prompt ou chave).
- Cofre de segredos AES-256-GCM com a `SECRETS_KEY` (nonce aleatório por gravação, o nome da chave como dado associado).
- Operações `set_secret` (`PUT /v1/secrets/{key}`, `engramind secrets set <chave>` lendo da entrada padrão, `--delete`), `test_redator` (`POST /v1/settings/redator/test`, `engramind settings test-redator --text … [--settings …]`), `list_jobs` (`GET /v1/jobs`, `engramind jobs list`), `retry_job` (`POST /v1/jobs/{job}/retry`, `engramind jobs retry <id>`) e `list_llm_calls` (`GET /v1/llm-calls`, `engramind llm-calls list`), com as ferramentas MCP de mesmo nome. Nova tag OpenAPI `Jobs` (admin).
- Configurações `redator.enabled`, `provider`, `model`, `api_key` (segredo), `base_url`, `instructions` e `rewrite_text` (por canal), `title_max`, `min_similarity`, `timeout_ms`, `max_attempts` e `fallback_models`; novos tipos `text`, `url`, `list` e `secret`.
- `get_status` com `queue` (pendentes, falhos, jobs por tipo e estado) e `redator` (ligado, configurado, `missing`, provedor, modelo e últimos sucesso e falha).
- Códigos de erro `SETTING_IS_SECRET`, `SECRET_UNKNOWN`, `PROVIDER_NOT_CONFIGURED` (400), `JOB_NOT_FOUND` (404), `JOB_NOT_RETRYABLE` (409), `PROVIDER_FAILED` (502) e `PROVIDER_TIMEOUT` (504).
- Migração `0006_redator.sql` (`secrets`, `llm_calls`, `jobs.outcome`). Decisões `docs/decisions/0015` a `0017`. Nova dependência `aes-gcm`.

### Alterado

- `get_settings` lista os segredos com `secret: true`, `configured` e `updated_at` (valor sempre `null`); `update_settings` recusa segredos com `SETTING_IS_SECRET`.
- `Core::new` recebe a `SECRETS_KEY`.
- Guias `configuracoes`, `operacao`, `erros` e `escrever-memorias` atualizados.

## [0.5.0] - 2026-10-30

Etapa 005 — contexto. **Marco: MVP do primeiro consumidor.**

### Adicionado

- `get_context` nas três vias (`GET /v1/context`, ferramenta MCP `get_context`, `engramind context`; tag OpenAPI `Context`, qualquer token·read): memórias `active` e `permanent` dos canais em modo `inject` (token `admin`: todos os canais ativos), restritas por `channels` e `Engramind-Channels`.
- Ordem determinística (prioridade da assinatura ou do canal, `permanent`, prioridade da categoria, `importance` classificada, `created_at`, `id`) e duas faixas: texto completo até `context.budget_tokens`, `#id: título` até `context.titles_budget_tokens`, o resto em `counts.overflow` e na linha `<overflow>`. A primeira memória que não cabe fecha a faixa; a sobreposição por canal é o teto da parte do canal.
- Formatos `tagged` (o exemplo do §8 do desenho), `markdown` (sem ids) e `json` (`content` objeto). A escolha das memórias é a mesma nos três. Texto de memória não forja as tags do bloco.
- `version` (`cv_` + 12 hex) sobre o formato, os canais alcançados (id, versão, modo, prioridade) e as configurações de renderização; `ETag`, `Cache-Control: private, no-cache`, `Vary` e `304` com `If-None-Match` sem ler o banco. Cache LRU de 1024 blocos. Contadores nunca mudam a `version`.
- MCP: `if_none_match` em `get_context` (`{ not_modified: true, version }`); resources `engramind://context`, `engramind://channels/{slug}` e `engramind://guides/{id}`; prompt `memory-writing` (`docs/prompts/memory-writing.md`).
- CLI: `engramind context [--format] [--if-none-match]` imprime o `content` cru; sem mudança, nada e saída 0. `memories search --exclude-context`.
- `search_memories` aceita `exclude_context` (tira a faixa de texto completo do contexto) e devolve `context_version`.
- `injected_count` em lote e a tabela `usage_log` (instante, app, agente, token, operação, canais, ids, `rendered_tokens`) para `get_context` entregue e `search_memories`, com índices por `at`, `(agent_id, at)` e `(op, at)` para consultas por período.
- Configurações `context.budget_tokens` e `context.titles_budget_tokens` (por canal), `context.default_format`, `context.token_estimator` (`o200k`/`cl100k`) e `context.overflow_line`; novo tipo de configuração `enum`, com `choices` em `get_settings`.
- Guias `contexto-e-cache` e `integracao-consumidor` (primeira versão); `integracao-mcp`, `configuracoes`, `cli`, `escrever-memorias`, `overview` e a skill atualizados.
- Novos códigos de erro `RESOURCE_NOT_FOUND` e `PROMPT_NOT_FOUND` (404, saída 5; só no MCP).
- Migração `0005_usage.sql`. Decisões `docs/decisions/0013` e `0014`. Nova dependência `tiktoken-rs`.

### Alterado

- `get_context` com `304` é logado em `DEBUG`.
- O MCP anuncia as capacidades `resources` e `prompts`.

## [0.4.1] - 2026-10-30

### Corrigido

- Teste `identities::lists_are_paginated_by_cursor` instável: apps criados no mesmo milissegundo desempatam pelo id aleatório; o teste passa a conferir só a partição em páginas. Sem mudança de comportamento.

## [0.4.0] - 2026-10-30

Etapa 004 — memórias.

### Adicionado

- Memórias nas três vias: `list_memories`, `get_memory`, `search_memories`, `memory_history`, `create_memory`, `update_memory`, `archive_memory`, `restore_memory`, `pin_memory`, `unpin_memory`, `move_memory`, `revert_memory` e `delete_memory` (tag OpenAPI `Memories`, qualquer token, com o acesso efetivo do canal).
- Toda mutação de memória grava, na mesma transação, a memória, a revisão (menos a exclusão), a auditoria (só metadados e o hash do texto), `memories.version`, `channels.version` (os dois canais no `move`), os jobs e a idempotência.
- Criação com título provisório (primeira frase, até 100 caracteres), `title_state`/`embedding_state` `pending` e os jobs `redact` e `embed` na fila `jobs` (os workers chegam na 006/007). A escrita nunca espera o LLM.
- Duplicata exata por `content_hash` normalizado (NFC, espaços colapsados, minúsculo): a criação devolve a existente com `duplicate: true` e `200`; edição, `revert` e `move` recebem `409 MEMORY_DUPLICATE`.
- `similar` na resposta de escrita: até três memórias do canal com Jaccard dos termos ≥ `memory.similar_threshold`.
- Edição com `expected_version` (`409 MEMORY_VERSION_CONFLICT` com `details.current_version`); `revert_memory` restaura uma versão numa revisão nova; `delete_memory` mantém a última revisão.
- Busca textual FTS5/BM25 sem acentos nos canais `inject` e `search` (mais os `manage` e arquivados citados), com filtros, `exclude_ids`, `include_archived`, `fields` e `rank`/`score`. `retrieval_count` e `last_retrieved_at` gravados em lote a cada 5 s.
- `list_memories` com `channels`, `categories`, `status`, `author`, `since`, `fields` e paginação por cursor. Memória num canal fora do alcance responde `404 MEMORY_NOT_FOUND`.
- `channel_stats` (tag `Channels`) e as configurações por canal `get_channel_settings`/`update_channel_settings` (tag `Settings`), gravadas em `channels.settings_json`.
- Configurações `categories.list`, `memory.text_min`, `memory.text_max` (por canal), `memory.dedupe_exact` e `memory.similar_threshold`. Retirar uma categoria em uso dá `409 CATEGORY_IN_USE`.
- CLI `engramind memories …`, `engramind channels stats` e `engramind channels settings get|set`; `--text -` lê o texto da entrada padrão (erro local `CLI_INPUT_INVALID`).
- `engramind doctor` confere o `memory_fts` contra `memories` (`integrity-check`).
- Skill `skills/engramind/SKILL.md`, conferida contra `engramind help --json`, e o guia `escrever-memorias`.
- Novos códigos de erro: `MEMORY_NOT_FOUND` e `REVISION_NOT_FOUND` (404, saída 5), `CATEGORY_NOT_FOUND` (400, saída 2), `CHANNEL_ARCHIVED`, `MEMORY_VERSION_CONFLICT`, `MEMORY_DUPLICATE` e `CATEGORY_IN_USE` (409, saída 6).
- Migração `0004_memories.sql` (`memories`, `memory_revisions`, `memory_fts` com triggers, `jobs`).
- Decisões `docs/decisions/0011` e `0012`. Nova dependência `unicode-normalization`.

### Alterado

- `delete_channel` também apaga as memórias do canal e as revisões delas; a auditoria registra `memories_removed`.
- Um canal arquivado não recebe escrita de memórias (`409 CHANNEL_ARCHIVED`).
- `SettingView.min`/`max` passam a ser números (`integer` e `number`) e `null` para `boolean`; novos tipos `number`, `boolean` e `categories`.

## [0.3.0] - 2026-10-30

Etapa 003 — canais e assinaturas.

### Adicionado

- Canais hierárquicos (`eng/global`), com slug único e imutável, prioridade e `version`, que cresce a cada mudança. Operações `list_channels`, `get_channel`, `create_channel`, `update_channel`, `archive_channel`, `restore_channel` e `delete_channel` nas três vias. Um canal arquivado sai das listagens sem `include_archived` e continua legível quando citado. `delete_channel` só exclui canal arquivado e leva junto as assinaturas exatas.
- Assinaturas por canal exato ou por prefixo terminado em `*` (`eng/*`, `eng/projeto-*`), com modo `inject`/`search`/`manage`, `access` restritivo e prioridade opcional. Operações `list_subscriptions`, `subscribe`, `update_subscription` e `unsubscribe`. Cada assinatura traz `reaches`, os canais que o alvo alcança agora.
- Resolvedor de acesso efetivo no `core` (`core::access`): escopos do token ∩ acesso da assinatura mais específica (a exata vence o prefixo; o prefixo mais longo vence o curto); token `admin` alcança todos os canais. Inclui o modo `manage` só por citação explícita e um cache em memória invalidado por geração.
- Seleção por requisição: cabeçalho `Engramind-Channels` no REST e no MCP (`--channels`/`ENGRAMIND_CHANNELS` na CLI) e parâmetro `channels`. Com os dois, vale a interseção.
- `list_channels` e `get_channel` trazem o alcance do token em `reach`. O token `agent` vê só os canais alcançados e as próprias assinaturas.
- Tags OpenAPI `Channels` (qualquer token) e `Network` (admin), e o guia `canais-e-assinaturas`.
- Novos códigos de erro: `CHANNEL_NOT_SUBSCRIBED` (403, saída 4), `CHANNEL_NOT_FOUND` e `SUBSCRIPTION_NOT_FOUND` (404, saída 5), `SUBSCRIPTION_EXISTS` e `CHANNEL_NOT_ARCHIVED` (409, saída 6). `SCOPE_REQUIRED` ganha `details.channel` quando a restrição vem da assinatura.
- Migração `0003_channels.sql` (`channels`, `subscriptions`).
- Decisão `docs/decisions/0010`.

### Alterado

- `delete_app` e `delete_agent` removem as assinaturas dos agentes, e a auditoria registra quantas saíram (`subscriptions_removed`).

## [0.2.0] - 2026-10-30

Etapa 002 — identidades e as três vias.

### Adicionado

- Tokens `eng.tk_<8 hex>.<segredo>` com HMAC-SHA256 (`TOKEN_PEPPER`), tipos `agent`/`admin`, escopos `read`/`write`/`delete`, expiração, revogação imediata e `last_used_at` gravado em lote.
- Bootstrap do primeiro token `admin` por `BOOTSTRAP_ADMIN_TOKEN`, aceito só com o banco sem tokens.
- Operações nas três vias (REST `/v1`, MCP e CLI): `whoami`; apps, agentes e tokens (`list_*`, `get_*`, `create_*`, `update_*`, `delete_*`, `issue_token`, `update_token`, `revoke_token`); `get_settings` e `update_settings`; `get_status` básico; `get_audit`; documentação (`get_overview`, `list_guides`, `get_guide`, `search_docs`, `list_operations`, `get_operation`, `get_schema`).
- Catálogo único de operações, autorização por tipo e escopo no `core` e proteção `LAST_ADMIN_TOKEN`.
- Auditoria de toda mutação na mesma transação, sem segredos; idempotência genérica (`Idempotency-Key`, `idempotency_key`, `--idempotency-key`) por 24 h.
- Rate limit por token (`limits.rate_per_minute`, `429 RATE_LIMITED` com `Retry-After`); configurações `limits.rate_per_minute` e `limits.max_batch`.
- MCP em `POST /mcp` (Streamable HTTP stateless): uma ferramenta por operação, `tools/list` filtrado pelo token, hints do catálogo, erros de domínio com o envelope em `structuredContent`, `MCP_ALLOWED_HOSTS` e erros de transporte no envelope padrão.
- CLI cliente: flags, variáveis `ENGRAMIND_*` e perfis em `~/.config/engramind/config.toml` (0600), `--json` em todo comando, códigos de saída estáveis e `engramind help --json`.
- Guias `overview`, `tokens-e-escopos`, `integracao-mcp`, `cli`, `erros`, `configuracoes` e `operacao`, compilados no binário.
- Novos códigos de erro: `AUTH_INVALID`, `SCOPE_REQUIRED`, `APP_NOT_FOUND`, `AGENT_NOT_FOUND`, `TOKEN_NOT_FOUND`, `GUIDE_NOT_FOUND`, `SECTION_NOT_FOUND`, `OPERATION_NOT_FOUND`, `SCHEMA_NOT_FOUND`, `TAG_NOT_FOUND`, `SLUG_ALREADY_EXISTS`, `IDEMPOTENCY_KEY_CONFLICT`, `LAST_ADMIN_TOKEN`, `SETTING_UNKNOWN`, `SETTING_INVALID`, `RATE_LIMITED` e `MCP_*`.
- Migração `0002_identities.sql` (`apps`, `agents`, `tokens`, `settings`, `idempotency`, `audit_log`).
- Testes de contrato do §15 (OpenAPI, catálogo, MCP, CLI, guias, erros e configurações) e a matriz tipo × escopo × operação nas três vias.
- Decisões `docs/decisions/0007` a `0009`.

## [0.1.0] - 2026-10-30

Etapa 001 — fundação operável. Nenhuma capacidade de domínio ainda.

### Adicionado

- Binário `engramind` (também `eng`) com `serve`, `migrate` e `doctor`, todos com `--json`.
- Configuração de infraestrutura validada no startup: `APP_PORT`, `TOKEN_PEPPER` (≥ 32 caracteres) e `SECRETS_KEY` (base64 de 32 bytes) obrigatórias; `MCP_ALLOWED_HOSTS` obrigatória em `staging`/`production`.
- Logs `tracing` em `compact` ou `json` (`LOG_FORMAT`, `LOG_ANSI`, `RUST_LOG`), `x-request-id` em toda resposta e um único evento `http.request.completed` por requisição, com a rota parametrizada.
- Envelope de erro `{ error: { code, message, details? } }` e os códigos `VALIDATION_ERROR`, `ROUTE_NOT_FOUND`, `METHOD_NOT_ALLOWED`, `PAYLOAD_TOO_LARGE`, `INTERNAL_ERROR`, `DATABASE_UNAVAILABLE`, `MIGRATIONS_PENDING` e `REQUEST_TIMEOUT`.
- `Store` com a implementação SQLite: thread escritora única, pool de leitores somente leitura, WAL, FTS5 e `sqlite-vec`; migração `0001_init.sql` (`schema_meta`).
- Verificador do `TOKEN_PEPPER`: o startup é recusado quando o pepper difere do usado no banco.
- Rotas públicas `GET /health/live`, `GET /health/ready`, `GET /openapi.json`, `GET /docs` e `GET /` (redirects relativos para `docs/`), e o snapshot `docs/openapi.json`.
- Imagem Docker de produção com Litestream (restore na primeira subida e réplica contínua para o MinIO), `docker-compose.yml` de um serviço e `scripts/check-compose.sh`.
- Testes com `cargo-nextest` (binário de integração único `tests/it/`) e `scripts/test.sh`.
- CI/CD: lint, compilação única dos testes, 4 shards, validação do compose, cobertura na `main`, build e smoke test da imagem e deploy por SCP.
- Decisões `docs/decisions/0001` a `0006`.
