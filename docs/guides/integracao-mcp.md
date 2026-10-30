# Integração MCP

O Engramind expõe um servidor MCP em `POST /mcp` (Streamable HTTP, **stateless**). Cada POST leva uma mensagem JSON-RPC e recebe `application/json`; não há sessões nem SSE.

## Conexão

- `Authorization: Bearer eng.<token_id>.<segredo>`;
- `Content-Type: application/json`;
- `Accept: application/json, text/event-stream`;
- o `Host` precisa estar em `MCP_ALLOWED_HOSTS` (proteção contra DNS rebinding); fora dela, `403 MCP_HOST_FORBIDDEN`.

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

## Resultados e erros

- Sucesso: `structuredContent` com o mesmo JSON do REST. `get_overview` e `get_guide` devolvem o Markdown como texto.
- Erro de domínio: `isError: true` e o envelope `{ "error": { "code", "message", "details" } }` em `structuredContent`, com o mesmo `code` do REST.
- Argumentos que não seguem o esquema da ferramenta: `VALIDATION_ERROR`, também com `isError: true`.
- Erros de protocolo (método inexistente, ferramenta desconhecida) continuam como erros JSON-RPC.
- Falhas HTTP (token, `Host`, `Accept`, `Content-Type`, corpo grande) usam o envelope padrão: `401 AUTH_INVALID`, `403 MCP_HOST_FORBIDDEN`, `406 MCP_NOT_ACCEPTABLE`, `413 PAYLOAD_TOO_LARGE`, `415 MCP_UNSUPPORTED_MEDIA_TYPE`, `400 MCP_BAD_REQUEST` e `422 MCP_INVALID_MESSAGE`.

## Documentação pelo MCP

As ferramentas `get_overview`, `list_guides`, `get_guide`, `search_docs`, `list_operations`, `get_operation` e `get_schema` servem a documentação da versão em execução. Guias e operações administrativas não aparecem para tokens `agent`.
