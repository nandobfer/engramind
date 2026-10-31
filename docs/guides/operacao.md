# Operação

Ferramentas para quem opera o Engramind. Todas exigem um token `admin` com `read`.

## Status

`get_status` resume tudo o que a tela do gestor precisa:

| Bloco | Conteúdo |
|---|---|
| `version`, `uptime_seconds` | Versão em execução e segundos desde a subida. |
| `database` | `status` (`ok` ou `migrations_pending`), migrações aplicadas e esperadas e `size_bytes`. |
| `backup` | `enabled`, `status` (`ok`, `fail` ou `skip`), `detail`, `replica_age_seconds`, `last_replica_at` e `checked_at`. |
| `counts` | Apps, agentes e tokens. |
| `queue` | Jobs pendentes, falhos e por tipo e estado. |
| `redator` | Ligado, configurado, chaves que faltam, provedor, modelo, último sucesso e última falha e `last_24h` (`calls`, `failures`, `cost_usd`) do job `redact`. |
| `embedding` | Provedor, modelo, `model_id`, dimensão e estado da reindexação. |
| `webhooks` | `enabled`, `apps` com webhook, `pending`, `delivered_24h` e `failed_24h`. |
| `retention` | Última execução da retenção (`last_run_at`) e as linhas removidas por tabela; `null` antes da primeira. |

```bash
engramind status --json
```

A réplica do Litestream é medida a cada 60 s por uma tarefa do servidor (`litestream ltx`), e `get_status` e `/metrics` leem essa medição; `engramind doctor` mede na hora. `backup.status = skip` quando o backup está desligado ou o binário `litestream` não existe.

## Uso

`get_usage` agrega `usage_log` (cada `get_context` entregue, `search_memories` e `get_hints`) em `[since, until)`: requisições, memórias devolvidas ou injetadas e tokens renderizados do contexto. `group_by` é `day` (padrão, UTC), `agent` (`key` é o id do agente, ou do app para tokens sem agente, e `label` o `app/agente`), `operation` ou `channel`. Sem `since`, os últimos 7 dias; no máximo 366 dias por consulta. No agrupamento por canal, uma requisição conta em cada canal que alcançou e as memórias no canal atual delas; o `total` conta cada requisição uma vez. Os `304` do contexto não entram (veja `/metrics`).

```bash
engramind usage --group-by agent --since 2026-10-01T00:00:00Z --json
```

## Métricas

`GET /metrics` expõe o formato de texto do Prometheus e exige um token `admin` com `read` (sem token, `401`; token `agent`, `403 SCOPE_REQUIRED`). No Prometheus:

```yaml
scrape_configs:
  - job_name: engramind
    metrics_path: /metrics
    authorization:
      credentials_file: /etc/prometheus/engramind.token
    static_configs:
      - targets: ["engramind-api:8000"]
```

| Métrica | Tipo | Rótulos |
|---|---|---|
| `http_requests_total` | counter | `method`, `route`, `status`, `operation` |
| `http_request_duration_seconds` | histogram | `method`, `route`, `operation` |
| `service_ready` | gauge | — |
| `engramind_build_info` | gauge | `version` |
| `engramind_jobs` | gauge | `kind`, `state` |
| `engramind_jobs_failed_total` | counter | `kind` |
| `engramind_llm_calls_total` | counter | `purpose`, `provider`, `outcome` |
| `engramind_llm_tokens_total` | counter | `purpose`, `direction` |
| `engramind_llm_cost_microusd_total` | counter | `purpose`, `provider` |
| `engramind_embedding_duration_seconds` | histogram | `provider`, `kind` |
| `engramind_embedding_failures_total` | counter | `provider`, `kind` |
| `engramind_webhook_deliveries_total` | counter | `outcome` |
| `engramind_auth_failures_total` | counter | `reason` |
| `engramind_backup_replica_age_seconds` | gauge | — |
| `engramind_database_size_bytes` | gauge | — |
| `engramind_retention_removed_total` | counter | `table` |

`operation` é o nome da operação do catálogo, inclusive em cada ferramenta MCP (todas passam pela rota `/mcp`); fica vazio nas rotas técnicas. Os rótulos nunca têm slugs, ids, URLs nem textos. A taxa de `304` do contexto sai de `http_requests_total{operation="get_context",status="304"}` sobre o total de `get_context`; as falhas de autenticação, de `engramind_auth_failures_total` (`missing`, `invalid`, `inactive`). O custo do LLM é contado em micro-dólares (`/ 1e6` para dólares). As métricas são do processo: reiniciar zera os contadores.

## Retenção

A cada hora uma tarefa do servidor remove, em lotes de 5.000 linhas por transação, o que passou do prazo: `usage_log` (`retention.usage_days`, 90), `llm_calls` (`retention.llm_calls_days`, 90), `audit_log` (`retention.audit_days`; vazio, nunca), jobs `done` e `failed` terminados há mais de `retention.jobs_days` (30; `queued` e `running` nunca saem) e chaves de idempotência com mais de 24 h. O último resultado fica em `get_status.retention` e sobrevive a reinícios; removendo algo, o log registra `INFO retention.purged` com as contagens.

## Auditoria

Toda mutação de identidade, token e configuração grava um evento na mesma transação, com autor (`app_id`, `agent_id`, `token_id`), ação, entidade e os estados antes e depois. Segredos e digests nunca entram na auditoria. Excluir um app, agente ou token não apaga os eventos dele.

`get_audit` lista do mais recente ao mais antigo, filtra por `entity`, `entity_id` e `action` e pagina por `cursor`.

```bash
engramind audit list --entity token --action token.revoke
```

| Ação | Entidade |
|---|---|
| `app.create`, `app.update`, `app.delete` | `app` |
| `agent.create`, `agent.update`, `agent.delete` | `agent` |
| `token.issue`, `token.update`, `token.revoke`, `token.bootstrap` | `token` |
| `settings.update` | `settings` (com a chave em `entity_id`) |
| `secret.set`, `secret.delete` | `secret` (com a chave em `entity_id`; só `configured` e `updated_at`) |
| `job.retry` | `job` |
| `memory.redact` | `memory` (autor `system/redator`) |
| `memory.import` | `channel` (com os ids criados e o número de duplicatas, sem textos) |

A auditoria de `app.create` e `app.update` mostra o webhook só como `webhook: { url, secret: { configured, updated_at } }`; o segredo nunca entra.

## Fila de jobs

O trabalho assíncrono roda numa fila em tabela (`jobs`), sem broker. A escrita de uma memória grava os jobs na mesma transação e responde na hora; um runner por tipo os executa depois do commit. Há três runners: `redact` (Redator) e `embed` (vetores da busca por sentido), com 2 jobs em paralelo cada, e `webhook` (entrega dos eventos aos apps), com 4.

- **Captura**: o runner pega o job pronto mais antigo do tipo com um único `UPDATE … RETURNING`, marca `running`, conta a tentativa e reserva um lease (`locked_until`). Ele acorda a cada commit que enfileira e, sem aviso, a cada segundo.
- **Lease vencido**: um job `running` cujo lease venceu (o processo caiu no meio) volta a ser capturável.
- **Falha temporária**: o job volta para `queued` com backoff e `last_error`: 2 s × 2ⁿ (teto de 10 min) até `redator.max_attempts` no `redact` e 5 tentativas no `embed`; 10 s × 4ⁿ (teto de 1 h) até `webhooks.max_attempts` no `webhook`. Esgotadas as tentativas, fica `failed`, com um único `ERROR` `jobs.failed`.
- **Superado**: um job de uma memória excluída, ou com um job do mesmo tipo mais novo para a mesma memória (o texto mudou de novo), termina `done` com `outcome = superseded`, sem efeito.

`list_jobs` lista do mais recente ao mais antigo, filtra por `state` e `kind` e pagina por `cursor`; o `payload` só tem ids e versões. `retry_job` recoloca um job `failed` na fila, com as tentativas zeradas; outro estado dá `409 JOB_NOT_RETRYABLE` e um id inexistente, `404 JOB_NOT_FOUND`. Exigem `read` e `write`.

```bash
engramind jobs list --state failed
engramind jobs retry 42
```

| `outcome` | Quando |
|---|---|
| `applied` | Título e texto revisado aplicados. |
| `title_only` | Só o título (sem revisão de texto, texto igual, duplicata no canal ou guarda de sentido). |
| `superseded` | Memória excluída ou conteúdo mudou depois do job. |
| `skipped` | Redator desligado ou sem configuração: `title_state = fallback`. |

## Webhooks

Cada mutação de memória, canal, assinatura ou configuração cria, na mesma transação, um job `webhook` por app destinatário (guia `integracao-gestor`). O `payload` do job é `{ app_id, event }`; a URL e o segredo são lidos na hora de cada tentativa. `last_error` traz só o status HTTP (`http 500`) ou a classe da falha (`timeout`, `connect`, `redirect`, `request`), nunca a URL; os logs `webhooks.retry` (`WARN`) e `webhooks.delivered` (`DEBUG`) trazem o host. Lease de `webhooks.timeout_ms` + 30 s.

| `outcome` do `webhook` | Quando |
|---|---|
| `delivered` | O app respondeu `2xx`. |
| `skipped` | App removido, desativado, sem URL ou segredo, ou `webhooks.enabled = false` na hora da entrega. |

```bash
engramind jobs list --kind webhook --state failed
```

## Embeddings

O job `embed` gera o vetor de `título + "\n" + texto` de cada memória depois da escrita, da edição, do título do Redator ou de uma reindexação (lotes `{ memory_ids, reindex }` de `embedding.batch_size`). O vetor só é gravado se a memória ainda tiver o conteúdo e o título lidos e o modelo ainda for o vigente; senão o job termina `superseded`. Lease de `embedding.timeout_ms` + 60 s e até 5 tentativas com o backoff da fila; esgotadas (ou sem provedor configurado, ou com dimensão divergente), a memória fica `embedding_state = failed`. Com `embedding.provider = off`, o job só marca `off`.

| `outcome` do `embed` | Quando |
|---|---|
| `embedded` | Vetores gravados. |
| `superseded` | Memória excluída, conteúdo, título ou modelo mudou, ou há `embed` mais novo para ela. |
| `off` | Embeddings desligados: `embedding_state = off`. |

`get_status.embedding` resume o provedor, o modelo, o `model_id`, a dimensão e o estado: `off`, `ready`, `reindexing` (com `total`, `done` e `reindex_started_at`) ou `failed` (um lote da reindexação esgotou as tentativas; `reembed_all` recomeça). Ao terminar uma reindexação, saem `INFO embeddings.reindex_finished` e, para cada modelo local removido de `MODELS_PATH`, `INFO embeddings.model_removed`; a primeira carga do modelo local gera `INFO embeddings.ready` com a duração. `engramind doctor` confere a biblioteca do ONNX Runtime no item `onnx_runtime` (pulado sem `ORT_DYLIB_PATH`).

Com o provedor `local`, o modelo é baixado no primeiro job para `MODELS_PATH` (o `multilingual-e5-small` tem uns 470 MB) e fica carregado no processo (cerca de 1 GB de RAM); até lá, a busca segue só com o FTS. Download e carga levam uns 20 s: os primeiros jobs podem estourar `embedding.timeout_ms` (`WARN embeddings.retry`, `outcome = timeout`) e voltam à fila com backoff enquanto a carga termina. Sem rede para o Hugging Face, os jobs falham e voltam à fila.

```bash
engramind status --json
engramind jobs list --kind embed --state failed
engramind embeddings reembed
```

## Redator

O job `redact` envia o texto ao LLM como **dado**, entre marcas com um código aleatório e com a instrução de não seguir nada do conteúdo; `redator.instructions` (global ou do canal) vai no fim do prompt do sistema. A resposta `{ title, text }` é validada pelo serviço (esquema, `redator.title_max`, `memory.text_min`/`text_max`), independente do modo de saída estruturada do provedor; resposta inválida conta como falha da tentativa.

- **Título**: aplicado com `title_state = ready`; não cria revisão nem muda a `version` da memória, mas incrementa a `version` do canal e muda a `version` do contexto (o título aparece na faixa de títulos).
- **Texto revisado** (com `redator.rewrite_text`): só vale se mudou, não repete outra memória do canal e passa na **guarda de sentido** (similaridade de embedding acima de `redator.min_similarity`). Aplicado, nasce uma revisão com autor `system/redator` (`reason = redact`), a `version` da memória sobe e o job `embed` é enfileirado; o original continua no histórico e `revert_memory` o restaura. A guarda compara os embeddings do original e do revisado (cosseno); com `embedding.provider = off` ou o provedor indisponível, ela falha fechada: só o título é aplicado.
- **Desligado ou sem configuração** (`redator.enabled = false`, ou sem provedor, modelo, chave ou URL): `title_state = fallback`, sem chamar o LLM e sem `ERROR`.
- **Modelos**: o principal e, em ordem, os de `redator.fallback_models` quando o anterior falha por indisponibilidade, `429` ou tempo limite (`WARN redator.fallback`).
- **Conflito**: se a memória mudou enquanto o LLM respondia, o resultado é descartado (`superseded`); a edição nova já enfileirou outro job.

Os logs do Redator (`redator.completed` em `INFO`, `redator.retry` e `redator.fallback` em `WARN`) trazem provedor, modelo, resultado, tentativa e duração, nunca o texto, o prompt ou a chave.

## Chamadas ao LLM

Toda chamada ao LLM (job `redact` ou `test_redator`) grava uma linha em `llm_calls`: finalidade (`purpose`), provedor, modelo, tokens de entrada e saída, custo (`cost_usd`, só quando o provedor informa, como o `usage.cost` do OpenRouter), duração, resultado (`success`, `timeout`, `rate_limited`, `unavailable`, `rejected` ou `invalid_response`) e um erro curto. O texto, o prompt e a chave nunca são gravados. `list_llm_calls` lista da mais recente à mais antiga e filtra por `since` (RFC 3339) e `purpose`.

```bash
engramind llm-calls list --purpose redact --since 2026-10-31T00:00:00Z
```

## Primeiro acesso

Com o banco vazio, defina `BOOTSTRAP_ADMIN_TOKEN` no `.env` e suba o serviço: o log registra `security.bootstrap_created`. Depois, emita os tokens dos sistemas gestores e guarde o do bootstrap num cofre. Com tokens existentes, o valor é ignorado (`security.bootstrap_ignored`); sem nenhum token e sem o valor, o log avisa `security.bootstrap_missing`.

## Logs

Cada requisição gera um único evento `http.request.completed` com `request_id`, rota parametrizada, `app`, `agent`, `token_id`, `operation`, status e duração. Tokens, segredos e o cabeçalho `Authorization` nunca aparecem. As chamadas MCP geram `mcp.tool.completed` em `DEBUG`, com o nome da ferramenta e o resultado, sem os argumentos.
