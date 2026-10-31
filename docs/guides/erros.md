# Erros

Todo erro usa o envelope único:

```json
{ "error": { "code": "APP_NOT_FOUND", "message": "App nao encontrado", "details": null } }
```

- `code` é estável, em inglês, e faz parte do contrato. Decida pelo `code`, nunca pela `message`.
- `message` é texto em pt-BR para quem lê e pode mudar.
- `details` traz dados de diagnóstico da operação, nunca segredos.
- No MCP, erros de domínio voltam com `isError: true` e este envelope em `structuredContent`.
- Na CLI, cada código vira um código de saída estável.

## Códigos

| Código | HTTP | Saída da CLI | Quando |
|---|---|---|---|
| `VALIDATION_ERROR` | 400 | 2 | Path, query, header ou body inválido. |
| `IDEMPOTENCY_KEY_REQUIRED` | 400 | 2 | A operação (`import_memories`) só roda com `Idempotency-Key` (`idempotency_key` no MCP). |
| `BATCH_TOO_LARGE` | 400 | 2 | O lote passa de `limits.max_batch` itens; `details` traz `max` e `received`. |
| `AUTH_INVALID` | 401 | 3 | Token ausente, malformado, desconhecido, expirado, revogado ou de app/agente desativado. |
| `SCOPE_REQUIRED` | 403 | 4 | O tipo ou os escopos do token (ou o acesso efetivo no canal) não permitem a operação; `details` traz `required_kind`, `required_scope` e, no canal, `channel`. |
| `CHANNEL_NOT_SUBSCRIBED` | 403 | 4 | O token não alcança o canal (sem assinatura ou fora da seleção `channels`/`Engramind-Channels`); `details.channel` traz o slug. |
| `ROUTE_NOT_FOUND` | 404 | 5 | Rota inexistente. |
| `APP_NOT_FOUND` | 404 | 5 | App inexistente. |
| `AGENT_NOT_FOUND` | 404 | 5 | Agente inexistente. |
| `TOKEN_NOT_FOUND` | 404 | 5 | Token inexistente. |
| `CHANNEL_NOT_FOUND` | 404 | 5 | Canal inexistente; canais nunca nascem implicitamente. |
| `SUBSCRIPTION_NOT_FOUND` | 404 | 5 | Assinatura inexistente. |
| `MEMORY_NOT_FOUND` | 404 | 5 | Memória inexistente ou num canal fora do alcance do token (ou do `Engramind-Channels`). |
| `REVISION_NOT_FOUND` | 404 | 5 | A memória não tem a versão pedida em `revert_memory`. |
| `CATEGORY_NOT_FOUND` | 400 | 2 | Categoria fora de `categories.list`; `details.valid` lista as aceitas. |
| `GUIDE_NOT_FOUND` | 404 | 5 | Guia inexistente ou oculto para o token; `details.valid` lista os visíveis. |
| `SECTION_NOT_FOUND` | 404 | 5 | Seção inexistente no guia; `details.valid` lista as seções. |
| `OPERATION_NOT_FOUND` | 404 | 5 | Operação inexistente ou oculta para o token; `details.valid` lista as visíveis. |
| `SCHEMA_NOT_FOUND` | 404 | 5 | Esquema inexistente ou oculto para o token; `details.valid` lista os visíveis. |
| `TAG_NOT_FOUND` | 404 | 5 | Tag OpenAPI inexistente ou oculta para o token; `details.valid` lista as visíveis. |
| `RESOURCE_NOT_FOUND` | 404 | 5 | URI de resource MCP desconhecida (só no MCP, como erro `-32002`). |
| `PROMPT_NOT_FOUND` | 404 | 5 | Prompt MCP inexistente (só no MCP, como `-32002`). |
| `METHOD_NOT_ALLOWED` | 405 | 1 | Método não suportado pela rota. |
| `SLUG_ALREADY_EXISTS` | 409 | 6 | Já existe um app, um agente no mesmo app ou um canal com esse slug. |
| `SUBSCRIPTION_EXISTS` | 409 | 6 | O agente já assina esse alvo (mesmo canal exato ou mesmo prefixo). |
| `CHANNEL_NOT_ARCHIVED` | 409 | 6 | Só um canal arquivado pode ser excluído; arquive antes. |
| `CHANNEL_ARCHIVED` | 409 | 6 | O canal está arquivado e não recebe escrita; restaure-o antes. |
| `MEMORY_VERSION_CONFLICT` | 409 | 6 | A memória mudou desde `expected_version`; `details.current_version` traz a atual. |
| `MEMORY_DUPLICATE` | 409 | 6 | Já existe uma memória com o mesmo texto no canal; `details.existing_id` aponta para ela. |
| `CATEGORY_IN_USE` | 409 | 6 | A mudança de `categories.list` retira categorias usadas; `details.in_use` traz a contagem. |
| `IDEMPOTENCY_KEY_CONFLICT` | 409 | 6 | A mesma `Idempotency-Key` já foi usada com outra operação ou outro corpo. |
| `LAST_ADMIN_TOKEN` | 409 | 6 | A mudança deixaria a rede sem nenhum token `admin` ativo com `write`. |
| `SETTING_UNKNOWN` | 400 | 2 | Chave de configuração desconhecida; `details.valid` lista as chaves. |
| `SETTING_INVALID` | 400 | 2 | Valor fora do tipo ou do intervalo da configuração; `details` traz a regra. |
| `SETTING_IS_SECRET` | 400 | 2 | A chave é um segredo e não passa por `update_settings`; grave com `set_secret`. |
| `SECRET_UNKNOWN` | 400 | 2 | Chave de segredo fora do catálogo; `details.valid` traz as aceitas. |
| `JOB_NOT_FOUND` | 404 | 5 | Job inexistente. |
| `JOB_NOT_RETRYABLE` | 409 | 6 | Só jobs com `state = failed` voltam para a fila. |
| `PROVIDER_NOT_CONFIGURED` | 400 | 2 | Redator ou embeddings desligados ou sem provedor, modelo, URL ou chave; `details.missing` traz as chaves que faltam. |
| `PROVIDER_FAILED` | 502 | 8 | O provedor do LLM ou de embedding falhou ou devolveu uma resposta inválida; `details.outcome` e `details.reason` trazem o motivo curto. |
| `PROVIDER_TIMEOUT` | 504 | 8 | O provedor do LLM ou de embedding não respondeu dentro de `redator.timeout_ms` ou `embedding.timeout_ms`. |
| `PROVIDER_LISTING_UNSUPPORTED` | 400 | 2 | O provedor (`gemini` ou `off`) não oferece listagem de modelos de embedding. |
| `EMBEDDING_DIMENSION_MISMATCH` | 502 | 8 | O provedor devolveu vetores de outra dimensão (a configurada em `embedding.dimensions` ou a do modelo); `details.expected` e `details.got`. |
| `REEMBED_IN_PROGRESS` | 409 | 6 | Já há uma reindexação com jobs na fila; acompanhe em `get_status.embedding`. |
| `CONSOLIDATION_DISABLED` | 409 | 6 | O canal está com `consolidation.enabled = false`; ligue a configuração no canal ou use `dry_run` para só pré-visualizar. |
| `PAYLOAD_TOO_LARGE` | 413 | 1 | Corpo acima de MAX_BODY_BYTES. |
| `MCP_BAD_REQUEST` | 400 | 1 | Cabeçalhos ou mensagem MCP inválidos. |
| `MCP_HOST_FORBIDDEN` | 403 | 1 | Cabeçalho `Host` fora de MCP_ALLOWED_HOSTS. |
| `MCP_NOT_ACCEPTABLE` | 406 | 1 | `Accept` precisa incluir application/json e text/event-stream. |
| `MCP_UNSUPPORTED_MEDIA_TYPE` | 415 | 1 | `Content-Type` do MCP precisa ser application/json. |
| `MCP_INVALID_MESSAGE` | 422 | 1 | Mensagem JSON-RPC inesperada. |
| `RATE_LIMITED` | 429 | 7 | Token acima de `limits.rate_per_minute`; espere `Retry-After` segundos. |
| `INTERNAL_ERROR` | 500 | 1 | Falha inesperada. |
| `DATABASE_UNAVAILABLE` | 503 | 8 | Escritor ou leitores do banco indisponíveis. |
| `MIGRATIONS_PENDING` | 503 | 8 | Há migração pendente; o serviço ainda não recebe tráfego. |
| `REQUEST_TIMEOUT` | 504 | 8 | Tempo limite da requisição (REQUEST_TIMEOUT_SECS) excedido. |

## Como reagir

- `401 AUTH_INVALID`: confira o token; não repita sem trocá-lo.
- `403 SCOPE_REQUIRED`: o token não tem o tipo ou o escopo; `details.required_kind` e `details.required_scope` dizem o que falta. Com `details.channel`, quem restringe é a assinatura daquele canal.
- `403 CHANNEL_NOT_SUBSCRIBED`: peça ao gestor uma assinatura no canal, ou confira se ele está no `Engramind-Channels` da conexão.
- `409 CHANNEL_NOT_ARCHIVED`: arquive o canal (`archive_channel`) antes de excluí-lo.
- `409 CHANNEL_ARCHIVED`: o canal foi arquivado; peça ao gestor para restaurá-lo ou escreva em outro canal.
- `409 MEMORY_VERSION_CONFLICT`: outro agente mudou a memória; releia (`get_memory`), refaça a edição sobre a versão atual e mande o novo `expected_version`.
- `409 MEMORY_DUPLICATE`: o texto já existe em `details.existing_id`; edite ou use aquela memória em vez de duplicar.
- `409 CATEGORY_IN_USE`: mova ou recategorize as memórias de `details.in_use` antes de retirar a categoria.
- `409 IDEMPOTENCY_KEY_CONFLICT`: a chave já foi usada com outro corpo; gere uma chave nova para a nova intenção.
- `409 LAST_ADMIN_TOKEN`: emita outro token `admin` com `write` antes de revogar, desativar ou excluir o último.
- `400 SETTING_IS_SECRET`: grave a chave com `set_secret`, nunca por `update_settings`.
- `400 PROVIDER_NOT_CONFIGURED`: defina as chaves de `details.missing` (`update_settings` e `set_secret`) ou proponha-as em `settings` no `test_redator` e no `test_embedding`.
- `409 JOB_NOT_RETRYABLE`: só jobs `failed` voltam para a fila; os outros já estão nela ou terminaram.
- `502 PROVIDER_FAILED` e `504 PROVIDER_TIMEOUT`: o provedor do LLM ou de embedding falhou; confira modelo, URL e chave, ou acrescente `redator.fallback_models`. Numa troca de embedding, nada foi gravado.
- `502 EMBEDDING_DIMENSION_MISMATCH`: ajuste ou apague `embedding.dimensions` (ou troque de modelo) e teste com `test_embedding`.
- `409 REEMBED_IN_PROGRESS`: espere a reindexação atual terminar (`get_status.embedding.state`).
- `409 CONSOLIDATION_DISABLED`: ligue `consolidation.enabled` no canal (`update_channel_settings`) ou rode com `dry_run` para só ver o plano.
- `400 IDEMPOTENCY_KEY_REQUIRED`: mande `Idempotency-Key` (a CLI deriva uma do arquivo); repetir a mesma chave com o mesmo corpo devolve a resposta gravada.
- `400 BATCH_TOO_LARGE`: divida o lote em partes de até `details.max` itens, cada uma com a sua chave.
- `400 PROVIDER_LISTING_UNSUPPORTED`: escolha o modelo pela documentação do provedor e valide com `test_embedding`.
- `429 RATE_LIMITED`: espere os segundos de `Retry-After`.
- `503` e `504`: falhas transitórias; repita com backoff, usando `Idempotency-Key` nas mutações.
