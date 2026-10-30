# Engramind — visão geral

O Engramind é uma rede de memórias para agentes de IA: guarda fatos duráveis e curtos em **canais** e os entrega por **REST, MCP e CLI**. Cada agente se identifica por um **token** com escopos. Uma memória pertence ao canal, não ao agente que a escreveu; o autor é proveniência.

Nesta versão existem as identidades (apps, agentes e tokens), os canais e as assinaturas, as configurações, a auditoria, o status básico e esta documentação. As memórias chegam na próxima versão.

## Três vias, um contrato

Toda operação existe nas três vias com o mesmo nome base, os mesmos parâmetros, o mesmo escopo e os mesmos códigos de erro:

| Via | Como chamar |
|---|---|
| REST | `GET /v1/whoami`, `POST /v1/tokens`… (contrato em `/openapi.json`, Swagger em `/docs`) |
| MCP | `POST /mcp`, ferramenta com o nome da operação (`whoami`, `issue_token`…) |
| CLI | `engramind whoami`, `engramind tokens issue`… |

O `operationId` do OpenAPI é o próprio nome base. `list_operations` e `get_operation` mostram o contrato exato de cada uma.

## Autenticação

Toda rota em `/v1` e o `/mcp` exigem `Authorization: Bearer eng.<token_id>.<segredo>`. Token ausente, inválido, expirado, revogado ou de app/agente desativado recebe `401 AUTH_INVALID`. Tipo ou escopo insuficiente recebe `403 SCOPE_REQUIRED`. Os detalhes estão no guia `tokens-e-escopos`.

## Canais

Um token `agent` só alcança os canais que o agente assina (`403 CHANNEL_NOT_SUBSCRIBED` nos demais), com o acesso efetivo = escopos do token ∩ acesso da assinatura. O cabeçalho `Engramind-Channels` e o parâmetro `channels` restringem os canais de cada requisição. Os detalhes estão no guia `canais-e-assinaturas`.

Rotas públicas, sem token: `GET /health/live`, `GET /health/ready`, `GET /openapi.json` e `GET /docs`.

## Envelope de erro

```json
{ "error": { "code": "SCOPE_REQUIRED", "message": "…", "details": { "required_kind": "admin", "required_scope": "write" } } }
```

`code` é estável, em inglês, e faz parte do contrato; `message` é texto em pt-BR para quem lê. A lista completa está no guia `erros`.

## Mutações

- **Idempotência**: toda mutação aceita `Idempotency-Key` (REST), `idempotency_key` (MCP) ou `--idempotency-key` (CLI). A mesma chave com a mesma operação e o mesmo corpo devolve a resposta gravada por 24 h; com outro corpo, `409 IDEMPOTENCY_KEY_CONFLICT`. A chave vale por token.
- **Auditoria**: toda mutação grava um evento com autor, ação e os estados antes e depois, sem segredos.

## Listas

Listas são paginadas por `cursor` e `limit` (padrão 50, máximo 200). A resposta traz `next_cursor`, que vale `null` na última página.

## Limite de requisições

Cada token pode fazer até `limits.rate_per_minute` requisições por minuto (padrão 600). Acima disso, a resposta é `429 RATE_LIMITED` com o cabeçalho `Retry-After`.

## Documentação viva

As operações do grupo Documentação servem os guias embutidos no binário e o OpenAPI da versão em execução. Comece por `get_overview`, use `list_guides` para ver os guias, `get_guide` para ler um deles, `search_docs` para localizar um assunto e `get_operation` ou `get_schema` para o contrato exato. Guias e operações administrativas não aparecem para tokens `agent`.
