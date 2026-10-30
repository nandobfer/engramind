# Changelog

Todas as mudanças relevantes do Engramind. Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/); versões em [SemVer](https://semver.org/lang/pt-BR/).

## [0.2.0] - 2026-10-30

Etapa 002 — identidades e as três vias.

### Adicionado

- Tokens `eng.tk_<8 hex>.<segredo>` com HMAC-SHA256 (`TOKEN_PEPPER`), tipos `agent`/`admin`, escopos `read`/`write`/`delete`, expiração, revogação imediata e `last_used_at` gravado em lote.
- Bootstrap do primeiro token `admin` por `BOOTSTRAP_ADMIN_TOKEN`, aceito só com o banco sem tokens.
- Operações nas três vias (REST `/v1`, MCP e CLI): `whoami`; apps, agentes e tokens (`list_*`, `get_*`, `create_*`, `update_*`, `delete_*`, `issue_token`, `update_token`, `revoke_token`); `get_settings` e `update_settings`; `get_status` básico; `get_audit`; documentação (`get_overview`, `list_guides`, `get_guide`, `search_docs`, `list_operations`, `get_operation`, `get_schema`).
- Catálogo único de operações, autorização por tipo e escopo no `core` e proteção `LAST_ADMIN_TOKEN`.
- Auditoria de toda mutação na mesma transação, sem segredos; idempotência genérica (`Idempotency-Key`, `idempotency_key`, `--idempotency-key`) por 24 h.
- Rate limit por token (`limits.rate_per_minute`, `429 RATE_LIMITED` com `Retry-After`); configurações `limits.rate_per_minute` e `limits.max_batch`.
- MCP em `POST /mcp` (Streamable HTTP stateless): uma ferramenta por operação, `tools/list` filtrado pelo token, hints do catálogo, erros de domínio com o envelope em `structuredContent`, `MCP_ALLOWED_HOSTS` e erros de transporte no envelope padrão.
- CLI cliente: flags, variáveis `ENGRAMIND_*` e perfis em `~/.config/engramind/config.toml` (0600), `--json` em todo comando, códigos de saída estáveis e `engramind help --json`.
- Guias `overview`, `tokens-e-escopos`, `integracao-mcp`, `cli`, `erros`, `configuracoes` e `operacao`, compilados no binário.
- Novos códigos de erro: `AUTH_INVALID`, `SCOPE_REQUIRED`, `APP_NOT_FOUND`, `AGENT_NOT_FOUND`, `TOKEN_NOT_FOUND`, `GUIDE_NOT_FOUND`, `SECTION_NOT_FOUND`, `OPERATION_NOT_FOUND`, `SCHEMA_NOT_FOUND`, `TAG_NOT_FOUND`, `SLUG_ALREADY_EXISTS`, `IDEMPOTENCY_KEY_CONFLICT`, `LAST_ADMIN_TOKEN`, `SETTING_UNKNOWN`, `SETTING_INVALID`, `RATE_LIMITED` e `MCP_*`.
- Migração `0002_identities.sql` (`apps`, `agents`, `tokens`, `settings`, `idempotency`, `audit_log`).
- Testes de contrato do §15 (OpenAPI, catálogo, MCP, CLI, guias, erros e configurações) e a matriz tipo × escopo × operação nas três vias.
- Decisões `docs/decisions/0007` a `0009`.

## [0.1.0] - 2026-10-30

Etapa 001 — fundação operável. Nenhuma capacidade de domínio ainda.

### Adicionado

- Binário `engramind` (também `eng`) com `serve`, `migrate` e `doctor`, todos com `--json`.
- Configuração de infraestrutura validada no startup: `APP_PORT`, `TOKEN_PEPPER` (≥ 32 caracteres) e `SECRETS_KEY` (base64 de 32 bytes) obrigatórias; `MCP_ALLOWED_HOSTS` obrigatória em `staging`/`production`.
- Logs `tracing` em `compact` ou `json` (`LOG_FORMAT`, `LOG_ANSI`, `RUST_LOG`), `x-request-id` em toda resposta e um único evento `http.request.completed` por requisição, com a rota parametrizada.
- Envelope de erro `{ error: { code, message, details? } }` e os códigos `VALIDATION_ERROR`, `ROUTE_NOT_FOUND`, `METHOD_NOT_ALLOWED`, `PAYLOAD_TOO_LARGE`, `INTERNAL_ERROR`, `DATABASE_UNAVAILABLE`, `MIGRATIONS_PENDING` e `REQUEST_TIMEOUT`.
- `Store` com a implementação SQLite: thread escritora única, pool de leitores somente leitura, WAL, FTS5 e `sqlite-vec`; migração `0001_init.sql` (`schema_meta`).
- Verificador do `TOKEN_PEPPER`: o startup é recusado quando o pepper difere do usado no banco.
- Rotas públicas `GET /health/live`, `GET /health/ready`, `GET /openapi.json`, `GET /docs` e `GET /` (redirects relativos para `docs/`), e o snapshot `docs/openapi.json`.
- Imagem Docker de produção com Litestream (restore na primeira subida e réplica contínua para o MinIO), `docker-compose.yml` de um serviço e `scripts/check-compose.sh`.
- Testes com `cargo-nextest` (binário de integração único `tests/it/`) e `scripts/test.sh`.
- CI/CD: lint, compilação única dos testes, 4 shards, validação do compose, cobertura na `main`, build e smoke test da imagem e deploy por SCP.
- Decisões `docs/decisions/0001` a `0006`.
