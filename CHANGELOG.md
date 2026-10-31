# Changelog

Todas as mudanças relevantes do Engramind. Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/); versões em [SemVer](https://semver.org/lang/pt-BR/).

## [0.10.0] - 2026-10-31

Correção dos provedores: temperatura configurável, motivo real da recusa e carga do modelo local fora da requisição (decisão 0027).

### Adicionado

- Configuração `redator.temperature` (0 a 2, padrão vazio), usada pelo Redator e pelo consolidador. Vazio omite o campo `temperature` do corpo nos três adaptadores e o provedor usa o padrão do modelo — os modelos que recusam o campo (família 5.5 da Anthropic) voltam a funcionar.
- Código de erro `PROVIDER_REJECTED` (422) para recusas 4xx do LLM e dos embeddings remotos, com o status e a mensagem do provedor em `details.reason` e na `message`.
- Código de erro `EMBEDDING_MODEL_LOADING` (409), com `model`, `state`, `started_at` e `elapsed_ms`, quando o modelo local de embedding ainda está baixando ou carregando.
- `get_status.embedding.local`: modelo e fase do download/carregamento local (`idle`, `downloading`, `loading`, `ready`, `failed`), com `started_at`, `elapsed_ms` e `error`.

### Alterado

- **Quebra de contrato:** recusas do provedor (HTTP 4xx exceto 408/429) deixam de responder `502 PROVIDER_FAILED` e passam a `422 PROVIDER_REJECTED`, que proxies como o Cloudflare não substituem. `PROVIDER_FAILED` fica para indisponibilidade e resposta inválida.
- `llm_calls.error` e `details.reason` trazem a mensagem do provedor (uma linha, sem a chave, até 200 caracteres), não só `HTTP 400`. O motivo nunca vai para os logs.
- O modelo local de embedding é baixado e carregado numa thread própria, disparada na inicialização, depois de `update_settings` e no primeiro uso; nenhuma requisição espera o download (no máximo 5 s). O job `embed` espera o modelo voltando à fila em 15 s sem consumir tentativa; falha de carga é devolvida por 30 s e depois tentada de novo. Eventos `embeddings.loading`, `embeddings.ready` (com `downloaded`) e `embeddings.load_failed`.
- OpenAPI: `update_settings` documenta os erros da troca de embedding; `test_redator`, `test_embedding` e `list_embedding_models` documentam `422` e `409`.
- Guias `configuracoes`, `erros` e `operacao`, desenho e `aicontext` atualizados.

## [0.9.0] - 2026-10-31

Etapa 009 — consolidação.

### Adicionado

- Consolidador por canal (job `consolidate`, 1 por vez, lease de 30 min), agendado por `consolidation.schedule` (cron de 5 campos em UTC, verificado a cada minuto, um disparo por horário mesmo depois de vários perdidos) ou pedido com `run_consolidation`. Dependência `croner` 4.0.2.
- Tarefa `duplicates`: pares `active` com cosseno ≥ `consolidation.duplicate_threshold` (vetores do modelo vigente) vão ao Redator, que decide fundir; o texto passa pela guarda de sentido contra cada original; a mais antiga recebe o texto (revisão `system/consolidador`, `consolidation.merge`) e os contadores, e a outra vira `archived` com `merged_into`. Eventos `memory.updated` e `memory.archived`. Pares recusados não voltam ao Redator enquanto os textos forem os mesmos.
- Tarefa `importance`: nota de 0 a 1 em lotes de 20 às memórias não classificadas, gravada em `importance_pending` e promovida a `importance` só na próxima transação que incrementa `channels.version` do canal (a ordem do contexto nunca muda sem mudar a `version`).
- Tarefa `stale`: `stale_candidate` para memórias `active` sem edição, recuperação, injeção nem conferência há mais de `consolidation.stale_after_days`, limpa quando voltam a ser usadas; com `consolidation.auto_archive_stale`, arquivamento com revisão `consolidation.stale` e evento.
- `run_consolidation` (`POST /v1/consolidation/run`, `engramind consolidation run`, ferramenta MCP; tag nova `Consolidation`, admin·write): `dry_run` síncrono sem LLM com o plano por canal (`200`), ou um job por canal (`202`, `already_queued` para o pendente). Chamadas em `llm_calls` com `purpose = consolidation`.
- `mark_verified` (`POST /v1/memories/{memory}/verify`, `engramind memories verify`, ferramenta MCP; any·write): grava `verified_at` e a auditoria `memory.verify` e limpa `stale_candidate`, sem revisão, sem `version` e sem evento.
- `Memory` ganha `stale_candidate` e `merged_into`; `list_memories` aceita `stale` (`engramind memories list --stale`); `channel_stats` conta `stale`.
- `get_status.consolidation` (canais habilitados, `schedule`, próximo e último disparo, fila, ações e chamadas ao LLM em 24 h) e a métrica `engramind_consolidation_actions_total{task,action}`.
- Configurações `consolidation.enabled` (por canal), `consolidation.schedule`, `consolidation.tasks`, `consolidation.duplicate_threshold`, `consolidation.stale_after_days` e `consolidation.auto_archive_stale`; tipos de configuração `cron` e lista de opções.
- Código de erro `CONSOLIDATION_DISABLED` (409).
- Migração `0009_consolidation.sql` (`memories.importance_pending`, `memories.last_injected_at`, `consolidation_pairs`); guia `consolidacao`; decisões `docs/decisions/0025` e `0026`.

### Alterado

- O lote de injeções do contexto avança `last_injected_at`; edição de texto, `revert` e texto do Redator zeram também `importance_pending`, e a edição limpa `stale_candidate`; `restore_memory` limpa `merged_into` e `stale_candidate`.
- A chamada ao LLM do Redator virou `Core::complete_with` (prompt, esquema e validação por chamada), compartilhada com o consolidador.
- Guias `escrever-memorias`, `integracao-gestor`, `operacao`, `configuracoes`, `erros`, `cli` e `overview` e a skill atualizados.

## [0.8.0] - 2026-10-31

Etapa 008 — integração (marco: migração de consumidor e gestor).

### Adicionado

- Eventos `memory.created`, `memory.updated`, `memory.archived`, `memory.deleted`, `channel.updated`, `subscription.updated` e `settings.updated` em toda mutação de memória, canal, assinatura, configuração e segredo, inclusive o título e o texto aplicados pelo Redator; `move_memory` gera um evento por canal. Payload `{ id, type, channel, channel_version, memory_ids, at }` sem textos, com `subscription`/`agent`/`target` e `keys` como extras.
- Webhooks por app: `create_app`/`update_app` aceitam `webhook_url` (http/https, sem credenciais) e `webhook_secret` (32 a 256 caracteres, só gravado, cifrado com `SECRETS_KEY`); a leitura mostra `webhook: { url, secret: { configured, updated_at } }`. Destinatários: apps com agente cuja assinatura alcança o canal e apps com token `admin` ativo; um job `webhook` por destinatário na transação da mutação.
- Job `webhook` (4 em paralelo): `POST` com `Engramind-Event-Id`, `Engramind-Event-Type` e `Engramind-Signature: t=<unix>,v1=<hex(HMAC-SHA256(segredo, t.corpo))>`, sem seguir redirecionamentos; backoff 10 s × 4ⁿ (teto 1 h) até `webhooks.max_attempts`; `skipped` para app removido, desativado ou sem URL.
- `import_memories` (`POST /v1/memories/import`, `engramind memories import`, ferramenta MCP; tag nova `Integration`): até `limits.max_batch` memórias numa transação, com `created_at`, autor e origem preservados, duplicatas reportadas, revisão 1 (`reason = import`), um único `memory.created` e `Idempotency-Key` obrigatória (a CLI deriva uma do arquivo; aceita JSON e JSONL).
- `get_usage` (`GET /v1/usage`, `engramind usage`, ferramenta MCP): `usage_log` agregado por `day`, `agent`, `operation` ou `channel`, com total.
- `get_status` completo: `uptime_seconds`, `database.size_bytes`, `backup.replica_age_seconds`/`last_replica_at`/`checked_at` (amostrador de 60 s), `redator.last_24h` (chamadas, falhas, custo), `webhooks` e `retention`.
- `GET /metrics` (formato Prometheus, token `admin`·`read`): `http_requests_total` e `http_request_duration_seconds` com o rótulo `operation` (inclusive por ferramenta MCP), fila, falhas de jobs, chamadas, tokens e custo do LLM, embeddings, entregas de webhook, falhas de autenticação, tamanho do banco, idade da réplica, retenção, prontidão e versão. Dependências `metrics` 0.24 e `metrics-exporter-prometheus` 0.18.3.
- Retenção horária em lotes de `usage_log`, `llm_calls`, `audit_log`, jobs terminados e `idempotency`, com o relatório em `schema_meta`.
- Configurações `webhooks.enabled`, `webhooks.timeout_ms`, `webhooks.max_attempts`, `retention.usage_days`, `retention.llm_calls_days`, `retention.audit_days` e `retention.jobs_days`.
- Códigos de erro `IDEMPOTENCY_KEY_REQUIRED` (400) e `BATCH_TOO_LARGE` (400).
- Migração `0008_integration.sql`; guia `integracao-gestor`; decisões `docs/decisions/0022` a `0024`.

### Alterado

- `update_app` usa o corpo `UpdateAppBody` (com os campos do webhook); `App` ganha `webhook`.
- A limpeza horária da idempotência passou para a retenção; `doctor::backup_check` compartilha a medição com o amostrador.
- Mutações de apps, agentes e tokens invalidam a rede em cache (o diretório dos webhooks).
- Guias `integracao-consumidor`, `operacao`, `configuracoes`, `erros`, `tokens-e-escopos`, `cli` e `overview` e a skill atualizados.

## [0.7.0] - 2026-10-31

Etapa 007 — embeddings e busca híbrida.

### Adicionado

- Embeddings: provedores `local` (`fastembed` 7.5 + ONNX Runtime 1.31.1 carregado por `ORT_DYLIB_PATH`, padrão `multilingual-e5-small` baixado no primeiro uso para `MODELS_PATH`), `openai_compatible` (`POST /embeddings`), `ollama` (`/api/embed`), `gemini` (`batchEmbedContents`) e `off`, com prefixos automáticos da família e5 e conferência de quantidade, dimensão e valores.
- Runner do job `embed` (2 em paralelo): vetor de `título + "\n" + texto`, gravado só se o conteúdo, o título e o modelo vigente ainda forem os lidos; superado por um `embed` mais novo; até 5 tentativas, depois `embedding_state = failed`. O backlog enfileirado desde a 0.4.0 é processado. O título aplicado pelo Redator também reenfileira o `embed`.
- Tabela `memory_vectors` (um vetor por memória, cascata no `delete_memory`) e o ramo vetorial por `vec_distance_cosine` (varredura exata filtrada por modelo, dimensão, canal, categoria e status). Migração `0007_vectors.sql`.
- `search_memories` híbrida: fusão RRF `Σ peso / (search.rrf_k + posição)` com `search.vector_weight` e `search.text_weight`, campo `mode` (`hybrid`/`text`) e queda silenciosa para o FTS (desligado, sem configuração, falha ou consulta acima de 2 s).
- `get_hints` (`POST /v1/hints`, `engramind hints <texto>`, ferramenta MCP): dicas `{ id, title, channel }` acima de `search.hint_threshold` (cosseno), só em canais `inject`/`search`, sem o que já está no contexto; lista vazia sem embeddings.
- Guarda de sentido real no Redator (cosseno entre original e revisado); falha fechada sem embeddings.
- Troca de provedor, modelo, URL, dimensão ou prefixo em `update_settings`: valida o provedor antes da transação (no `local`, só confere o modelo) e reindexa na mesma transação (outro `model_id`: só o que falta; mesmo `model_id`: tudo; `off`: `embedding_state = off`). Ao terminar, apaga vetores de outros modelos e modelos locais fora de uso.
- Operações `test_embedding` (`POST /v1/settings/embedding/test`, `engramind settings test-embedding`), `list_embedding_models` (`POST /v1/settings/embedding/models`, `engramind settings embedding-models`) e `reembed_all` (`POST /v1/embeddings/reembed`, `202`, `engramind embeddings reembed`), com as ferramentas MCP de mesmo nome. Novas tags OpenAPI `Search` (qualquer token) e `Embeddings` (admin).
- Configurações `embedding.provider`, `model`, `base_url`, `api_key` (segredo), `dimensions`, `query_prefix`, `document_prefix`, `batch_size`, `timeout_ms` e `search.vector_weight`, `text_weight`, `rrf_k`, `default_limit`, `hint_threshold`, `hint_limit`.
- `get_status.embedding` (provedor, modelo, `model_id`, dimensão, `state` `off`/`ready`/`reindexing`/`failed`, `total`, `done`) e o item `onnx_runtime` no `doctor`.
- Códigos de erro `PROVIDER_LISTING_UNSUPPORTED` (400), `REEMBED_IN_PROGRESS` (409) e `EMBEDDING_DIMENSION_MISMATCH` (502).
- Guia `busca-e-dicas`; decisões `docs/decisions/0018` a `0021`; dependência `fastembed`; a imagem traz a biblioteca do ONNX Runtime (versão e SHA-256 fixos).

### Alterado

- `search_memories` passou da tag `Memories` para `Search`; `score` agora é a fusão dos dois ramos e a resposta ganha `mode`.
- Com `embedding.provider = off`, a escrita não enfileira `embed` e grava `embedding_state = off`.
- `Core::new` recebe `ModelsSettings` (`MODELS_PATH` e `ORT_DYLIB_PATH`, validado no startup quando definido); `serve` sincroniza o `model_id` vigente na inicialização.
- `PROVIDER_NOT_CONFIGURED`, `PROVIDER_FAILED` e `PROVIDER_TIMEOUT` valem também para os embeddings.
- Guias `configuracoes`, `operacao`, `erros`, `escrever-memorias`, `contexto-e-cache`, `integracao-consumidor`, `integracao-mcp`, `overview` e `cli` e a skill atualizados.

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
