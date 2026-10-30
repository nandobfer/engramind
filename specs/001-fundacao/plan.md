# Plan - fundação operável

**Data**: 2026-10-30
**Referência de spec**: [`spec.md`](spec.md)

## Resumo

Portar o esqueleto do sap-middleman (config, observabilidade, erro, extractors, router, OpenAPI, Swagger, scripts e CI) trocando PostgreSQL/sqlx por SQLite atrás de `Store`, e adicionar Litestream, compose sem perfis e o binário com CLI.

## Contexto técnico

| Item | Valor |
|------|-------|
| Linguagem | Rust 1.98.0, edition 2024 |
| HTTP | axum 0.8, tokio, tower-http |
| Dados | rusqlite (`bundled`), sqlite-vec, FTS5, rusqlite_migration |
| Docs | utoipa 5, utoipa-swagger-ui (`vendored`) |
| CLI | clap (derive) |
| Testes | unitários e integração HTTP com `tower::ServiceExt::oneshot` e SQLite em `tempfile::TempDir` |
| Deploy | Docker rootless, compose, Litestream → MinIO |
| Testes/CI | `cargo-nextest` (archive + partition), um binário de integração, `Swatinem/rust-cache`, `cargo-chef` + cache `gha` do Buildx |
| CD | skill `setup-ci-cd` (build no runner, SCP, `docker load`, Compose sem build/pull) |

Dependências: `anyhow`, `axum`, `base64`, `chrono`, `clap`, `dotenvy`, `hmac`, `sha2`, `rusqlite`, `rusqlite_migration`, `serde`, `serde_json`, `sqlite-vec`, `thiserror`, `tokio`, `tower-http`, `tracing`, `tracing-subscriber`, `utoipa`, `utoipa-swagger-ui`, `uuid`, `zeroize`; dev: `http`, `http-body-util`, `tempfile`, `tower`. Registradas em `docs/decisions/0001-dependencias-base.md`.

## Checklist do AGENTS.md

- [x] Regra no `core` (não há domínio nesta etapa)
- [x] Fora do escopo do desenho respeitado
- [x] Segredos nunca em log, imagem ou exemplo
- [x] Testes com SQLite temporário
- [x] Versão e CHANGELOG

## Áreas afetadas

**Entry points**: `src/main.rs`, `src/app.rs`, `src/openapi.rs`.
**Infra**: `src/config.rs`, `src/observability.rs`, `src/error.rs`, `src/http.rs`, `src/store/`.
**Deploy**: `Dockerfile`, `docker-compose.yml`, `docker/entrypoint.sh`, `litestream.yml`, `.env.example`.
**Qualidade**: `scripts/`, `.config/nextest.toml`, `.github/workflows/ci-cd.yml`, `tests/it/`.
**CD**: `scripts/deploy-production.ts`, `docker-compose.deploy.yml`.
**Docs**: `CHANGELOG.md`, `docs/decisions/0001`–`0006`, `docs/openapi.json`, `aicontext/README.md`, `aicontext/use-quando-configurar-deploy.md` (redes).

## Estratégia de implementação

1. Spike: `rusqlite` bundled + `sqlite-vec` + `sqlite3_auto_extension` + FTS5 em um teste isolado; travar versões no `Cargo.lock`.
2. Cargo, toolchain, lints, `.gitignore`, `.dockerignore`.
3. `config.rs` e `observability.rs` portados do sap-middleman (nomes `engramind`).
4. `error.rs` com o envelope do desenho e catálogo de códigos; `http.rs` portado.
5. `store/`: escritor, leitores, PRAGMAs, migrações, pepper check, `ready`, `diagnose`.
6. `app.rs`, `health.rs`, `openapi.rs` (servers `.`, `bearerAuth`), snapshot `docs/openapi.json`.
7. `cli/` com `serve`, `migrate`, `doctor` e `output.rs`.
8. `Dockerfile`, entrypoint, Litestream, compose, `.env.example`, `check-compose.sh`.
9. Estrutura de testes: `tests/it/`, perfis do `Cargo.toml`, `.config/nextest.toml`; `scripts/test.sh` (check-compose, fmt, clippy, `cargo llvm-cov nextest`).
9a. `ci-cd.yml` com `Lint`, `Build tests`, `Test (i/4)`, `Compose`, `Coverage`, `Build image`, `Deploy production`; `deploy-production.ts` e `docker-compose.deploy.yml` copiados da skill `setup-ci-cd` (só `OPEX_IMAGE` → `API_IMAGE`).
9b. Medição: tempo de compilação dos testes e de cada shard registrado no `report.md`, como base para mudar a quantidade de shards.
10. Decisões, CHANGELOG, `aicontext/README.md`.

## Estratégia de testes

**Unit**: parsing de `Settings` (obrigatórias, formatos, produção exige `MCP_ALLOWED_HOSTS`), `observability` (formatos inválidos), envelope de erro, pepper check.
**Integração**: `SqliteStore` em `tempdir` (PRAGMAs efetivos, `vec_version()` responde, FTS5 com `remove_diacritics 2`, migração do zero idempotente, escrita concorrente serializada), HTTP (health, redirects, 404/405/413/504 no envelope, `x-request-id`, ausência de CORS), contrato (snapshot OpenAPI; toda rota anotada).
**Deploy**: `check-compose.sh` valida `name`, `container_name`, redes externas, volume e ausência de perfis; verificação manual do restore.
**CI**: o primeiro PR confirma os 4 checks de shard, a compilação única e o cache; o primeiro push na `main` confirma build, smoke test e deploy.
**Lacunas conhecidas**: restore real do MinIO não roda no CI; o `deploy-production.ts` não ganha testes aqui (no `opex` eles são Vitest; o script é usado sem alteração de lógica).

## Riscos

| Risco | Mitigação |
|-------|-----------|
| `sqlite-vec` incompatível com a versão do SQLite do `rusqlite` bundled | Spike no passo 1 antes de qualquer outro código. |
| Litestream 0.5 mudou flags (`-if-replica-exists`; `snapshots` virou `ltx`) | Fixar versão e validar os comandos no build da imagem. |
| Docker rootless sem acesso à rede no build | `build.network: host`, como `opex` e `dndicas`. |
| Shards com pouco teste no início custarem mais em subida de job do que economizam | A quantidade é uma lista na matriz; `report.md` registra os tempos e a lista pode cair para 2 até a suíte crescer. |
| `cargo nextest archive` não levar algo que os testes leem em runtime (guias, migrações, snapshots) | Guias e migrações entram por `include_str!`; snapshots são lidos de `CARGO_MANIFEST_DIR` remapeado com `--workspace-remap .` (checkout no shard). |
| Volume nomeado com dono root e usuário sem privilégio | `entrypoint` não faz `chown`; o `Dockerfile` cria `/data` com o dono do usuário de runtime antes do `VOLUME`. |
