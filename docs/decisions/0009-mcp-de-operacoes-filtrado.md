# 0009 — MCP de operações filtrado pelo token

**Data**: 2026-10-30 · **Etapa**: 002 (0.2.0) · **Status**: aceita

## Contexto

O sap-middleman tem um MCP só de documentação. O Engramind expõe também as operações, e o desenho (§4) pede que `tools/list` mostre só o que o token pode usar.

## Decisão

- Um `EngramindServer` (`rmcp` 3.4, `ServerHandler`) com um `ToolRouter` gerado por `#[tool_router]`: uma ferramenta por operação do catálogo. Descrição, título e hints vêm de `OPERATIONS` na construção, sobrescrevendo o roteador.
- `list_tools` e `call_tool` são implementados à mão: o `Principal` vem de `RequestContext.extensions` (as `Parts` da requisição HTTP, como o `caller()` do sap-middleman). `tools/list` filtra por `authz::allows`; chamar uma ferramenta oculta devolve o mesmo `SCOPE_REQUIRED` do REST.
- Argumentos planos: caminho, query e corpo do REST viram propriedades do mesmo objeto, mais `idempotency_key` nas mutações. Exceção: `update_settings` recebe o mapa em `settings`.
- Erros de domínio voltam como `CallToolResult` com `isError: true` e o envelope `{ error: { code, message, details } }` em `structuredContent`. Argumentos fora do esquema (que o `rmcp` devolve como texto) são convertidos em `VALIDATION_ERROR` no mesmo formato. Erros de protocolo continuam JSON-RPC.
- Transporte portado: `StreamableHttpService` stateless com `NeverSessionManager`, respostas `application/json`, `MCP_ALLOWED_HOSTS` e `normalize_transport_errors` (envelope padrão para as falhas HTTP do transporte, com os códigos `MCP_*`).
- Log: `mcp.tool.completed` em `DEBUG`, só com o nome da ferramenta do catálogo, o resultado e a duração; nunca os argumentos. `rmcp` fica limitado a `WARN`.
- `get_overview` e `get_guide` devolvem o Markdown como texto (como no sap-middleman); as demais ferramentas, o mesmo JSON do REST em `structuredContent`.
