# Tasks - identidades e as três vias

**Input**: `spec.md`, `clarify.md`, `contracts.md`, `plan.md`

## Setup

- [x] T001 - Resolver Q-001, Q-002 e Q-003.
- [x] T002 - Migração `0002_identities.sql` e `Store` de identidades, configurações, auditoria e idempotência.

## Core

- [x] T010 - `TokenService`, `Principal`, `ScopeSet`.
- [x] T011 - `OPERATIONS`, `authz`, `ERROR_CODES` com códigos de saída.
- [x] T012 - Auditoria e idempotência.
- [x] T013 - Serviços de apps, agentes e tokens (escalada de escopo, `LAST_ADMIN_TOKEN`).
- [x] T014 - Catálogo `SETTINGS`, `get_settings`, `update_settings`.
- [x] T015 - `get_status` básico e `get_audit`.
- [x] T016 - Bootstrap.

## Adaptadores

- [x] T020 - `require_auth`, rate limit, flush de `last_used_at`.
- [x] T021 - REST + `#[utoipa::path]` + snapshot.
- [x] T022 - Documentação viva (catálogo, índice OpenAPI, busca) e operações `docs`.
- [x] T023 - MCP (servidor, filtro, hints, erros, transporte).
- [x] T024 - CLI (cliente, perfis, comandos, `help --json`, códigos de saída).

## Testes

- [x] T030 - Unitários do core.
- [x] T031 - Integração de identidades, configurações, idempotência e rate limit.
- [x] T032 - Matriz tipo × escopo × operação nas três vias.
- [x] T033 - MCP e CLI.
- [x] T034 - Testes de contrato 1 a 6.
- [x] T035 - Logs sem segredos.

## Docs

- [x] T040 - Guias `overview`, `tokens-e-escopos`, `configuracoes`, `cli`, `integracao-mcp`, `erros`, `operacao`.
- [x] T041 - `docs/decisions/0007-tokens-hmac.md`, `0008-catalogo-de-operacoes.md`, `0009-mcp-de-operacoes-filtrado.md`; ajustes em `docs/design.md` §4 e §16.
- [x] T042 - `aicontext/identities.md`, `aicontext/settings.md`, `aicontext/README.md`; CHANGELOG 0.2.0.
- [x] T043 - `implement.md` e `report.md`.
