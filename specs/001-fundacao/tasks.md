# Tasks - fundação operável

**Input**: `spec.md`, `clarify.md`, `contracts.md`, `plan.md`

## Setup

- [x] T001 - Resolver Q-001, Q-003 a Q-007 do `clarify.md`.
- [x] T002 - Spike `rusqlite` + `sqlite-vec` + FTS5 e travar versões.
- [x] T003 - `Cargo.toml`, `rust-toolchain.toml`, lints, `.gitignore`, `.dockerignore`.

## Implementação

- [x] T010 - `config.rs` com `Secret`/`SecretKey`.
- [x] T011 - `observability.rs` (formatos, request id, evento de conclusão).
- [x] T012 - `error.rs` com envelope e catálogo de códigos; `http.rs`.
- [x] T013 - `store/` (trait, escritor, leitores, PRAGMAs, `vec_extension.rs`).
- [x] T014 - `migrations/0001_init.sql`, migrações embutidas e pepper check.
- [x] T015 - `app.rs`, `health.rs`, `openapi.rs`, Swagger e redirects.
- [x] T016 - `cli/` com `serve`, `migrate`, `doctor` e `output.rs`.

## Deploy

- [x] T020 - `Dockerfile` multi-stage (builder, runtime com Litestream, link `engramind`).
- [x] T021 - `docker/entrypoint.sh` e `litestream.yml`.
- [x] T022 - `docker-compose.yml`, `.env.example`, `scripts/check-compose.sh`.

## Testes

- [x] T030 - Unitários de `Settings`, observabilidade e erro.
- [x] T031 - Integração do `SqliteStore` em `tempdir`.
- [x] T032 - Integração HTTP (health, redirects, envelope, limites, CORS).
- [x] T033 - Contrato: snapshot `docs/openapi.json` e rotas anotadas.

## Testes e CI/CD

- [x] T034 - `tests/it/` (binário único), perfis do `Cargo.toml`, `.config/nextest.toml`.
- [x] T035 - `scripts/test.sh` com `cargo llvm-cov nextest`.
- [x] T036 - `.github/workflows/ci-cd.yml` (lint, build-tests, 4 shards, compose, coverage, build, deploy).
- [x] T037 - `Dockerfile` com `cargo-chef`; smoke test da imagem no CI.
- [x] T038 - `scripts/deploy-production.ts` e `docker-compose.deploy.yml` da skill `setup-ci-cd`.
- [ ] T039 - Entregar ao usuário a lista de Secrets/Variables do Environment `production` e a proteção da `main` com os checks obrigatórios.

## Qualidade e docs

- [ ] T040 - Medir compilação e shards no primeiro PR; registrar no `report.md`.
- [x] T041 - `docs/decisions/0001-dependencias-base.md`, `0002-store-e-escritor-unico.md`, `0003-binario-unico-engramind.md`, `0004-unsafe-sqlite-vec.md`, `0005-testes-e-ci-rapidos.md`, `0006-cd-por-artefato.md`.
- [x] T042 - `CHANGELOG.md` 0.1.0, `aicontext/README.md`, `aicontext/use-quando-configurar-deploy.md` (redes e CD), `aicontext/use-sempre-que-desenvolver.md` e `AGENTS.md` (como rodar só os testes afetados, layout `tests/it/`, nada de arquivo novo em `tests/`).
- [x] T043 - Verificação manual: compose com e sem Litestream e restore.
- [ ] T044 - `implement.md` e `report.md`.
