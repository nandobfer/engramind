# Changelog

Todas as mudanças relevantes do Engramind. Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/); versões em [SemVer](https://semver.org/lang/pt-BR/).

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
