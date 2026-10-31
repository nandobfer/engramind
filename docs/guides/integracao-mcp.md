# Integração MCP

O Engramind expõe um servidor MCP em `POST /mcp` (Streamable HTTP, **stateless**). Cada POST leva uma mensagem JSON-RPC e recebe `application/json`; não há sessões nem SSE.

## Conexão

- `Authorization: Bearer eng.<token_id>.<segredo>`;
- `Content-Type: application/json`;
- `Accept: application/json, text/event-stream`;
- o `Host` precisa estar em `MCP_ALLOWED_HOSTS` (proteção contra DNS rebinding); fora dela, `403 MCP_HOST_FORBIDDEN`;
- opcional: `Engramind-Channels: eng/global,eng/projeto-atlas`, o filtro padrão de canais da conexão. Registre o MCP por sessão com esse cabeçalho, e o agente não precisa escolher canais. O parâmetro `channels` de uma ferramenta só restringe dentro dele (guia `canais-e-assinaturas`).

```json
{ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
  "params": { "name": "whoami", "arguments": {} } }
```

## Ferramentas

Cada operação do Engramind é uma ferramenta com o mesmo nome base, os mesmos parâmetros, o mesmo escopo e os mesmos códigos de erro do REST. Os parâmetros de caminho, de query e do corpo ficam juntos em `arguments`; por exemplo, `update_app` recebe `{ "app": "app-a", "name": "Novo nome" }`. A única diferença é `update_settings`, que recebe o mapa em `settings`.

- `tools/list` mostra **só as ferramentas que o token pode usar**: um token `agent` não vê as administrativas, e um token só `read` não vê as mutações.
- Chamar uma ferramenta oculta devolve o mesmo `SCOPE_REQUIRED` do REST.
- Toda ferramenta declara `readOnlyHint`, `destructiveHint` e `idempotentHint`, derivados do catálogo de operações.
- Mutações aceitam `idempotency_key`, com o mesmo efeito do cabeçalho `Idempotency-Key`.
- `search_memories` e `get_hints` são leituras (`readOnlyHint`), mesmo sendo `POST` no REST: não aceitam `idempotency_key`. Para o agente lembrar de algo sem gastar contexto, `get_hints { text }` devolve só `{ id, title, channel }` (guia `busca-e-dicas`).

## Resultados e erros

- Sucesso: `structuredContent` com o mesmo JSON do REST. `get_overview` e `get_guide` devolvem o Markdown como texto.
- Erro de domínio: `isError: true` e o envelope `{ "error": { "code", "message", "details" } }` em `structuredContent`, com o mesmo `code` do REST.
- Argumentos que não seguem o esquema da ferramenta: `VALIDATION_ERROR`, também com `isError: true`.
- Erros de protocolo (método inexistente, ferramenta desconhecida) continuam como erros JSON-RPC.
- Falhas HTTP (token, `Host`, `Accept`, `Content-Type`, corpo grande) usam o envelope padrão: `401 AUTH_INVALID`, `403 MCP_HOST_FORBIDDEN`, `406 MCP_NOT_ACCEPTABLE`, `413 PAYLOAD_TOO_LARGE`, `415 MCP_UNSUPPORTED_MEDIA_TYPE`, `400 MCP_BAD_REQUEST` e `422 MCP_INVALID_MESSAGE`.

## Contexto

A ferramenta `get_context { channels?, format?, if_none_match? }` é a mesma operação do REST. `if_none_match` faz o papel do cabeçalho `If-None-Match`: com a `version` atual, o resultado é `{ "not_modified": true, "version" }`, sem ler o banco. Detalhes no guia `contexto-e-cache`.

## Resources

`resources/list` mostra o que o token alcança; `resources/templates/list`, os modelos.

| URI | Conteúdo | Exige |
|---|---|---|
| `engramind://context` | O bloco de `get_context` com o `Engramind-Channels` da conexão, no `context.default_format` (`text/plain`, ou `application/json` no formato `json`) | `read` |
| `engramind://channels/{slug}` | `#id: título` das memórias `active` e `permanent` do canal, na ordem do contexto, até 500 linhas e uma linha final com o restante (`text/plain`). O slug vai cru, com as barras: `engramind://channels/eng/global` | `read` efetivo no canal |
| `engramind://guides/{id}` | O guia em Markdown (`text/markdown`); guias `admin` ocultos para tokens `agent` | qualquer token |

Erros de resource são erros JSON-RPC: `-32002` (não encontrado) para URI desconhecida (`RESOURCE_NOT_FOUND`), guia inexistente, canal inexistente ou fora das assinaturas; os demais usam `-32600` ou `-32603`. O envelope do domínio vai em `data`.

## Prompt

`prompts/list` mostra `memory-writing`, um texto curto de como escrever uma boa memória, para o consumidor anexar ao agente que grava. `prompts/get` com outro nome devolve `-32002` (`PROMPT_NOT_FOUND`).

## Documentação pelo MCP

As ferramentas `get_overview`, `list_guides`, `get_guide`, `search_docs`, `list_operations`, `get_operation` e `get_schema` servem a documentação da versão em execução. Guias e operações administrativas não aparecem para tokens `agent`.
