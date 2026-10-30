# Tasks - identidades e as três vias

**Input**: `spec.md`, `clarify.md`, `contracts.md`, `plan.md`

## Setup

- [ ] T001 - Resolver Q-001, Q-002 e Q-003.
- [ ] T002 - Migração `0002_identities.sql` e `Store` de identidades, configurações, auditoria e idempotência.

## Core

- [ ] T010 - `TokenService`, `Principal`, `ScopeSet`.
- [ ] T011 - `OPERATIONS`, `authz`, `ERROR_CODES` com códigos de saída.
- [ ] T012 - Auditoria e idempotência.
- [ ] T013 - Serviços de apps, agentes e tokens (escalada de escopo, `LAST_ADMIN_TOKEN`).
- [ ] T014 - Catálogo `SETTINGS`, `get_settings`, `update_settings`.
- [ ] T015 - `get_status` básico e `get_audit`.
- [ ] T016 - Bootstrap.

## Adaptadores

- [ ] T020 - `require_auth`, rate limit, flush de `last_used_at`.
- [ ] T021 - REST + `#[utoipa::path]` + snapshot.
- [ ] T022 - Documentação viva (catálogo, índice OpenAPI, busca) e operações `docs`.
- [ ] T023 - MCP (servidor, filtro, hints, erros, transporte).
- [ ] T024 - CLI (cliente, perfis, comandos, `help --json`, códigos de saída).

## Testes

- [ ] T030 - Unitários do core.
- [ ] T031 - Integração de identidades, configurações, idempotência e rate limit.
- [ ] T032 - Matriz tipo × escopo × operação nas três vias.
- [ ] T033 - MCP e CLI.
- [ ] T034 - Testes de contrato 1 a 6.
- [ ] T035 - Logs sem segredos.

## Docs

- [ ] T040 - Guias `overview`, `tokens-e-escopos`, `configuracoes`, `cli`, `integracao-mcp`, `erros`, `operacao`.
- [ ] T041 - `docs/decisions/0007-tokens-hmac.md`, `0008-catalogo-de-operacoes.md`, `0009-mcp-de-operacoes-filtrado.md`; ajustes em `docs/design.md` §4 e §16.
- [ ] T042 - `aicontext/identities.md`, `aicontext/settings.md`, `aicontext/README.md`; CHANGELOG 0.2.0.
- [ ] T043 - `implement.md` e `report.md`.
