# Spec - fundação operável

**Criado em**: 2026-10-30
**Status**: Draft
**Versão alvo**: 0.1.0
**Input**: Desenho §3, §15, §16, §18, §20, §21, o padrão operacional do sap-middleman e o CI/CD do `opex` e do `dndicas` (skill `setup-ci-cd`).

## Objetivo

Entregar o esqueleto executável do Engramind: binário único, configuração de infraestrutura validada, observabilidade, envelope de erro, banco SQLite atrás da interface `Store` com escritor dedicado, migrações embutidas, healthchecks, OpenAPI/Swagger vazios de domínio, imagem Docker de produção com Litestream replicando para o MinIO do host, a estrutura de testes rápida e o CI/CD completo (testes em shards bloqueando o PR; build da imagem no runner e deploy por SCP na `main`). Nenhuma capacidade de domínio.

## Escopo

- `Cargo.toml` (edition 2024, `rust-version = "1.98"`), `rust-toolchain.toml` 1.98.0, lints do crate.
- Binário único `engramind`, também invocável como `engramind`, com os subcomandos `serve`, `migrate` e `doctor`.
- `config.rs`: variáveis de infraestrutura do §16, `Secret` com `zeroize`, falha no startup quando obrigatórias faltam ou são inválidas.
- `observability.rs`: `LOG_FORMAT`, `LOG_ANSI`, `RUST_LOG`, request id, evento único `http.request.completed`.
- `error.rs`: envelope `{ error: { code, message, details? } }`.
- `http.rs`: extractors `ApiJson`, `ApiQuery`, `ApiPath`, `RequestId`.
- `app.rs`: router, timeout, limite de corpo, CORS fechado, redirects relativos, `no-store` no Swagger, `fallback` 404.
- `store/`: trait `Store`, implementação SQLite (PRAGMAs, thread escritora, pool de leitores), registro do `sqlite-vec`, `rusqlite_migration`.
- Migração `0001_init.sql` com `schema_meta` (pepper check).
- `GET /health/live`, `GET /health/ready`, `GET /openapi.json`, `GET /docs`, `GET /`.
- `Dockerfile`, `docker-compose.yml`, `docker/entrypoint.sh`, `litestream.yml`, `.env.example`, `.dockerignore`, `.gitignore`.
- Estrutura de testes rápida: `cargo-nextest`, um único binário de integração (`tests/it/`), perfis de compilação de teste e cache (seção "Testes e CI rápidos").
- `scripts/test.sh`, `scripts/check-compose.sh`, `.config/nextest.toml`.
- CI/CD em `.github/workflows/ci-cd.yml` (lint, arquivo de testes, 4 shards, compose, cobertura, build da imagem, deploy), `scripts/deploy-production.ts` e `docker-compose.deploy.yml` (skill `setup-ci-cd`, como no `opex`).
- `CHANGELOG.md`, `docs/decisions/0001` a `0006`.

## Requisitos

| ID | Requisito |
|----|-----------|
| FR-001 | `APP_PORT`, `TOKEN_PEPPER` (≥ 32 caracteres) e `SECRETS_KEY` (base64 de 32 bytes) são obrigatórias; ausência ou valor inválido encerra o processo com erro claro, sem exibir o valor. |
| FR-002 | `LOG_FORMAT` aceita `compact`/`json`; `LOG_ANSI` aceita `true`/`false`; `RUST_LOG` inválido impede o startup. |
| FR-003 | Toda resposta carrega `x-request-id` (preservado quando é UUID válido, gerado caso contrário). |
| FR-004 | Cada request gera um único evento `http.request.completed` com `request_id`, método, rota parametrizada, status e latência; healthchecks, `/docs` e `/openapi.json` ficam em `DEBUG` quando não falham. |
| FR-005 | Erros HTTP usam o envelope `{ error: { code, message, details? } }`; `401` inclui `WWW-Authenticate: Bearer`. |
| FR-006 | O banco abre com `journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout=5000` e `foreign_keys=ON`, com `sqlite-vec` e FTS5 disponíveis. |
| FR-007 | Toda escrita passa pela thread escritora; leituras usam o pool de leitores. |
| FR-008 | `serve` aplica migrações pendentes; `migrate` só aplica; `doctor` confere `PRAGMA integrity_check`, migrações, extensões e, com Litestream ligado, a idade da última réplica. |
| FR-009 | O pepper check gravado em `schema_meta` impede o startup com um `TOKEN_PEPPER` diferente do usado no banco. |
| FR-010 | `/health/live` responde sem tocar o banco; `/health/ready` confere banco e migrações e responde `503` com o envelope quando falha. |
| FR-011 | `GET /` e `GET /docs` redirecionam para `docs/` (relativo); o Swagger carrega `../openapi.json`; o OpenAPI declara `servers: [{ url: "." }]` e o esquema `bearerAuth`. |
| FR-012 | CORS fechado: nenhuma resposta emite cabeçalhos `Access-Control-Allow-*`. |
| FR-013 | Corpo acima de `MAX_BODY_BYTES` → `413 PAYLOAD_TOO_LARGE`; requisição acima de `REQUEST_TIMEOUT_SECS` → `504 REQUEST_TIMEOUT`. |
| FR-014 | A imagem roda com usuário sem privilégios, `HEALTHCHECK` em `/health/live`, Litestream de versão fixa e entrypoint com `restore -if-db-not-exists -if-replica-exists` e `replicate -exec`. |
| FR-015 | `LITESTREAM_ENABLED=false` executa `engramind serve` direto. |
| FR-016 | O compose tem um único serviço `api`, sem perfis, `name: engramind-${APP_PORT}`, volume nomeado `engramind-data:/data` e as redes externas definidas em `clarify.md` (D-006). |
| FR-017 | Nunca há `println!`, `eprintln!` ou `dbg!` no servidor; `unsafe` só no módulo de registro do `sqlite-vec`. |
| FR-018 | Os testes rodam com `cargo-nextest` (um processo por teste, sem estado vazando entre testes) e `test-threads` igual ao número de CPUs. |
| FR-019 | Existe um único binário de testes de integração (`tests/it/main.rs` com um módulo amplo por domínio); a biblioteca não compila doctests; o binário `main.rs` não tem testes próprios. |
| FR-020 | O CI compila os testes uma única vez (`cargo nextest archive`) e executa o arquivo em 4 shards paralelos (`--partition hash:i/4`), sem recompilar em cada shard. |
| FR-021 | Lint (`fmt` + `clippy`) e validação do compose rodam em jobs paralelos aos testes; todos são checks obrigatórios do PR. |
| FR-022 | Na `main`, depois de todos os checks: build da imagem no runner (camada de dependências cacheada), smoke test do contêiner em `/health/ready`, `docker save \| gzip`, checksum e artefato; deploy por SCP com `docker load` e `docker compose up -d --no-build --pull never api`, healthcheck e indicação da imagem anterior para rollback. |
| FR-023 | Inicializações pesadas (OpenAPI, BPE do `tiktoken`, modelo local de embeddings, clientes de provedores) são preguiçosas (`LazyLock`/`OnceLock`): só o teste que usa paga o custo. |

## Fora do escopo

- Qualquer tabela ou rota de domínio (tokens, canais, memórias).
- Autenticação (nasce na 002; nesta etapa não há rota protegida).
- `/metrics` (008) e `/mcp` (002).
- Embeddings e ONNX Runtime na imagem (007).
- Estágio de desenvolvimento no `Dockerfile` e perfis no compose (o desenho proíbe).
- Workflow de prévia (`preview.yml`) do `opex` — ver Q-006 do `clarify.md`.

## Critérios de sucesso

- `cargo run -- serve` sobe localmente com `.env` e responde `/health/live` e `/health/ready`.
- `docker compose up -d --build` sobe o contêiner saudável com `LITESTREAM_ENABLED=false` e com `true` apontando para o MinIO do host; após apagar o volume, o banco é restaurado da réplica.
- Startup recusado com variável obrigatória ausente, `LOG_FORMAT` inválido e pepper divergente.
- `GET /` e `GET /docs` sem seguir redirect retornam `Location: docs/`.
- `./scripts/test.sh` passa localmente.
- Num PR, os checks `Lint`, `Test (1/4)`…`Test (4/4)` e `Compose` aparecem e bloqueiam o merge; a compilação dos testes acontece uma vez só.
- Um push na `main` executa build, smoke test e deploy no servidor, e o `/health/ready` responde pela URL configurada.
