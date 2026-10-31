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
- `429 RATE_LIMITED`: espere os segundos de `Retry-After`.
- `503` e `504`: falhas transitórias; repita com backoff, usando `Idempotency-Key` nas mutações.
