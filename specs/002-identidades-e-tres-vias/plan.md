# Plan - identidades e as três vias

**Data**: 2026-10-30
**Referência de spec**: [`spec.md`](spec.md)

## Resumo

Criar o `core` com principal, autorização e catálogo de operações; implementar identidades, configurações, auditoria e idempotência; portar o MCP do sap-middleman como servidor de operações e documentação; criar a CLI cliente e os testes de contrato que valem para todas as etapas seguintes.

## Contexto técnico

| Item | Valor |
|------|-------|
| Novas dependências | `rand` 0.8, `rmcp` 3.4.0 (`server`, `macros`, `transport-streamable-http-server`), `schemars` 1.2, `reqwest` 0.12 (rustls, json), `toml`, `dirs` (`hmac` e `sha2` já vieram na 001) |
| Testes | HTTP em processo, MCP via `oneshot` com JSON-RPC, CLI contra um servidor de teste em porta efêmera |

## Áreas afetadas

**Core**: `src/core/` (novo).
**Adaptadores**: `src/rest/`, `src/mcp/`, `src/cli/`, `src/auth/`.
**Dados**: `migrations/0002_identities.sql`, `src/store/sqlite/identities.rs`, `settings.rs`, `audit.rs`, `idempotency.rs`.
**Docs**: `docs/guides/` (7 guias), `docs/openapi.json`, `aicontext/identities.md`, `aicontext/settings.md`, `docs/decisions/0007-0009`.

## Estratégia de implementação

1. Resolver Q-001 a Q-003.
2. `TokenService` portado; `Principal`, `ScopeSet` e testes unitários.
3. Migração `0002` e métodos do `Store`.
4. `OPERATIONS`, `authz::require` e catálogo `ERROR_CODES` com os códigos de saída.
5. Auditoria e idempotência como utilitários do `core`.
6. Serviços de apps, agentes, tokens, configurações, auditoria e status.
7. Bootstrap no `serve`.
8. Middleware `require_auth`, rate limit e flush de `last_used_at`.
9. Handlers REST com `#[utoipa::path]`; snapshot OpenAPI.
10. Documentação: catálogo de guias, índice do OpenAPI e busca portados; os 7 guias.
11. MCP: servidor, ferramentas geradas, filtro de `tools/list`, erros de domínio, transporte.
12. CLI: cliente, perfis, comandos por grupo, `help --json`, códigos de saída.
13. Testes de contrato e matriz.
14. Versão 0.2.0, CHANGELOG, aicontext, decisões.

## Estratégia de testes

**Unit**: parser de token, `ScopeSet`, `authz::require`, validação de configurações, hash de idempotência, cursor.
**Integração**: bootstrap (vazio e populado), ciclo de vida de app/agente/token, expiração, revogação, `LAST_ADMIN_TOKEN`, escalada de escopo negada, auditoria sem segredos, idempotência (replay e conflito), rate limit, `last_used_at` em lote.
**Matriz**: para cada operação, tokens `agent`/`admin` × `read`/`read+write`/`read+delete`/`read+write+delete` → status esperado, nas três vias (REST, MCP e CLI).
**MCP**: `initialize`, `tools/list` filtrado por token, hints, erro de domínio com envelope, hosts, normalização de erros de transporte.
**CLI**: `help --json`, perfis, `--json`, códigos de saída.
**Logs**: captura de eventos garantindo ausência do token e do segredo (como `tests/mcp_logging.rs`).

## Riscos

| Risco | Mitigação |
|-------|-----------|
| `rmcp` 3.4 não expõe forma simples de filtrar `list_tools` | Sobrescrever `ServerHandler::list_tools` e `call_tool`, delegando ao `ToolRouter`; validar no começo do passo 11. |
| Três vias divergirem com o tempo | Catálogo único (D-002) e matriz executada nas três vias. |
| Ferramentas MCP demais para o contexto de um agente | Filtro por token; o consumidor ainda pode filtrar mais. |
