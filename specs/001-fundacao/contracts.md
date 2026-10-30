# Contracts - fundação operável

**Criado em**: 2026-10-30
**Status**: Draft
**Feature**: [`spec.md`](spec.md)

## Visão geral

Contratos de infraestrutura que todas as etapas seguintes usam: configuração, erro, `Store`, HTTP público e artefatos de deploy.

## Estrutura de módulos

```text
src/main.rs                    # dotenvy, init de logs, dispatch do clap, application.failed
src/lib.rs
src/cli/mod.rs                 # árvore clap (serve, migrate, doctor nesta etapa)
src/cli/output.rs              # único ponto de escrita no terminal (#[allow(clippy::print_stdout, clippy::print_stderr)] justificado)
src/config.rs                  # Settings::from_env, Secret
src/observability.rs           # init_logging, assign_request_id, log_request
src/error.rs                   # ApiError, ErrorBody, ErrorResponse
src/http.rs                    # ApiJson, ApiQuery, ApiPath, RequestId
src/app.rs                     # AppState, build_app
src/openapi.rs                 # ApiDoc, root_docs_redirect, contratos de /docs e /openapi.json
src/health.rs                  # live, ready
src/doctor.rs                  # diagnóstico local
src/store/mod.rs               # trait Store, StoreError
src/store/sqlite/mod.rs        # SqliteStore
src/store/sqlite/writer.rs     # thread escritora
src/store/sqlite/readers.rs    # pool de leitores
src/store/sqlite/migrations.rs # rusqlite_migration
src/store/sqlite/vec_extension.rs  # único bloco unsafe
migrations/0001_init.sql
```

## `Settings`

```rust
pub struct Settings {
    pub environment: Environment,        // APP_ENV: development | test | staging | production
    pub host: IpAddr,                    // APP_HOST (padrão 0.0.0.0)
    pub port: u16,                       // APP_PORT (obrigatória)
    pub database_path: PathBuf,          // DATABASE_PATH (padrão ./data/engramind.db fora do contêiner)
    pub models_path: PathBuf,            // MODELS_PATH (usado a partir da 007)
    pub bootstrap_admin_token: Option<Secret>, // usado a partir da 002
    pub token_pepper: Secret,            // TOKEN_PEPPER (obrigatória, >= 32)
    pub secrets_key: SecretKey,          // SECRETS_KEY (obrigatória, base64 de 32 bytes)
    pub request_timeout: Duration,       // REQUEST_TIMEOUT_SECS (30)
    pub max_body_bytes: usize,           // MAX_BODY_BYTES (1048576)
    pub read_pool_size: usize,           // READ_POOL_SIZE (4)
    pub mcp_allowed_hosts: AllowedHosts, // MCP_ALLOWED_HOSTS (usado a partir da 002)
    pub litestream_enabled: bool,        // LITESTREAM_ENABLED (informativo para get_status/doctor)
}
```

`Secret` e `SecretKey` implementam `Zeroize`/`ZeroizeOnDrop` e `Debug` mascarado. `APP_ENV=production|staging` exige `MCP_ALLOWED_HOSTS` (como o sap-middleman).

## Erro

```rust
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,           // pt-BR
    pub details: Option<Value>,
}

#[derive(Serialize, ToSchema)]
pub struct ErrorResponse { pub error: ErrorBody }

#[derive(Serialize, ToSchema)]
pub struct ErrorBody { pub code: String, pub message: String, pub details: Option<Value> }
```

Os códigos ficam num catálogo único (`pub const ERROR_CODES: &[ErrorCodeDoc]` com código, status e descrição) usado pelo teste de contrato do guia de erros.

## `Store`

```rust
pub trait Store: Clone + Send + Sync + 'static {
    // D-026: RPITIT com `+ Send` em vez de `async fn`.
    fn ready(&self) -> impl Future<Output = Result<ReadyReport, StoreError>> + Send;    // banco acessível e migrações em dia
    fn diagnose(&self) -> impl Future<Output = Result<DoctorReport, StoreError>> + Send; // integrity_check, migrações, extensões
    fn meta(&self, key: &str) -> impl Future<Output = Result<Option<Vec<u8>>, StoreError>> + Send;
    fn set_meta(&self, key: &str, value: &[u8]) -> impl Future<Output = Result<(), StoreError>> + Send;
}

pub enum StoreError {
    Unavailable,           // escritor parado, pool esgotado por timeout, I/O
    Conflict(&'static str),
    NotFound,
    Internal(String),      // nunca contém dados de memória
}
```

Cada etapa estende a trait com os métodos do próprio domínio. Nenhum tipo de `rusqlite` aparece na assinatura.

`SqliteStore::open(path, read_pool_size)` registra o `sqlite-vec` (uma vez por processo), abre o escritor, aplica os PRAGMAs em todas as conexões e expõe:

```rust
impl SqliteStore {
    async fn write<R: Send + 'static>(&self, f: impl FnOnce(&Transaction) -> rusqlite::Result<R> + Send + 'static) -> Result<R, StoreError>;
    async fn read<R: Send + 'static>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<R> + Send + 'static) -> Result<R, StoreError>;
}
```

`write` e `read` são privados ao módulo `store::sqlite`.

## Migração `0001_init.sql`

```sql
CREATE TABLE schema_meta (
    key   TEXT PRIMARY KEY,
    value BLOB NOT NULL
) STRICT;
```

## HTTP

| Método | Rota | `operationId` | Tag | Segurança | Respostas |
|---|---|---|---|---|---|
| GET | `/health/live` | `health_live` | Health | nenhuma | 200 `{ status: "ok" }` |
| GET | `/health/ready` | `health_ready` | Health | nenhuma | 200 `{ status: "ready", database: "ok", migrations: { applied, expected } }`; 503 `DATABASE_UNAVAILABLE` / `MIGRATIONS_PENDING` (com `details.applied`/`expected`) |
| GET | `/openapi.json` | `get_openapi_document` | Operations | nenhuma | 200 |
| GET | `/` | `open_swagger_ui` | Operations | nenhuma | 303 `Location: docs/` |
| GET | `/docs` | `get_swagger_ui` | Operations | nenhuma | 303 `Location: docs/` |

## Códigos de erro desta etapa

| Código | Status | Quando |
|---|---|---|
| `VALIDATION_ERROR` | 400 | path, query ou body inválido |
| `ROUTE_NOT_FOUND` | 404 | rota inexistente |
| `METHOD_NOT_ALLOWED` | 405 | método não suportado |
| `PAYLOAD_TOO_LARGE` | 413 | corpo acima de `MAX_BODY_BYTES` |
| `INTERNAL_ERROR` | 500 | falha inesperada |
| `DATABASE_UNAVAILABLE` | 503 | escritor ou leitores indisponíveis |
| `MIGRATIONS_PENDING` | 503 | `/health/ready` com migração pendente |
| `REQUEST_TIMEOUT` | 504 | tempo limite da requisição |

## CLI

| Comando | Abre o banco | Saída |
|---|---|---|
| `engramind serve` | sim | logs |
| `engramind migrate` | sim | migrações aplicadas (`--json`) |
| `engramind doctor` | sim | relatório; código de saída ≠ 0 quando algum item falha (`--json`) |

## Deploy

`docker-compose.yml`:

```yaml
name: engramind-${APP_PORT:?APP_PORT is required}

services:
  api:
    build:
      context: .
      network: host
    image: ${API_IMAGE:-engramind-api:latest}
    container_name: ${CONTAINER_NAME:?CONTAINER_NAME is required}
    init: true
    restart: unless-stopped
    environment: { ... variáveis do §16, obrigatórias com ${VAR:?...} ... }
    ports: ["${APP_PORT}:${APP_PORT}"]
    volumes: ["engramind-data:/data"]
    networks: [engramind, s3]
    deploy: { resources: { limits: { cpus: '2.0', memory: 2G } } }

networks:
  engramind: { external: true, name: "${ENGRAMIND_NETWORK:-engramind-network}" }
  s3:          { external: true, name: "${S3_NETWORK:-minio-network}" }

volumes:
  engramind-data:
```

`litestream.yml`:

```yaml
dbs:
  - path: ${DATABASE_PATH}
    replica:            # Litestream 0.5: `replicas:` (lista) está obsoleto
      type: s3
      bucket: ${ENGRAMIND_S3_BUCKET}
      path: ${ENGRAMIND_S3_PATH}
      endpoint: ${ENGRAMIND_S3_ENDPOINT}
      region: us-east-1
      force-path-style: true
```

`.env.example` = lista do §16 acrescida de `READ_POOL_SIZE`, `ENGRAMIND_NETWORK`, `S3_NETWORK` e `ENGRAMIND_S3_ENDPOINT=http://minio:9000`.

## Testes

```text
tests/it/main.rs          # único binário de integração: `mod support; mod health; mod store; mod contract; …`
tests/it/support/mod.rs   # TestContext (SqliteStore em tempdir, app, tokens), helpers HTTP
tests/it/<domínio>.rs     # um módulo amplo por domínio, criado pela etapa que introduz o domínio
```

`Cargo.toml`:

```toml
[lib]
doctest = false

[[bin]]
name = "engramind"
path = "src/main.rs"
test = false

[profile.dev]
debug = "line-tables-only"

[profile.dev.package."*"]
debug = false

[profile.dev.package.libsqlite3-sys]
opt-level = 3
```

`.config/nextest.toml`:

```toml
[profile.default]
# Explícito de propósito, como o `maxWorkers: "100%"` do opex/dndicas: usa todos os núcleos
# (2 ou 4 no runner do GitHub, todos na máquina do dev).
test-threads = "num-cpus"
fail-fast = false
slow-timeout = { period = "30s", terminate-after = 4 }

[profile.ci]
failure-output = "immediate-final"
junit = { path = "junit.xml" }
```

## CI/CD (`.github/workflows/ci-cd.yml`)

Actions fixadas por SHA; `permissions: contents: read`; gatilhos `push` na `main`, `pull_request` e `workflow_dispatch`.

| Job | Nome do check | Quando | Depende de | O que faz |
|---|---|---|---|---|
| `lint` | `Lint` | sempre | — | `cargo fmt --check`; `cargo clippy --locked --all-targets --all-features -- -D warnings` (cache próprio) |
| `build-tests` | `Build tests` | sempre | — | `cargo nextest archive --locked --workspace --all-targets --all-features --archive-file nextest.tar.zst`; publica o artefato |
| `test` | `Test (i/4)` | sempre | `build-tests` | matriz `shard: [1, 2, 3, 4]`; baixa o arquivo; `cargo nextest run --profile ci --archive-file nextest.tar.zst --workspace-remap . --partition hash:${{ matrix.shard }}/${{ strategy.job-total }}`; publica `junit.xml` |
| `compose` | `Compose` | sempre | — | `./scripts/check-compose.sh` (inclui o `docker-compose.deploy.yml`) |
| `coverage` | `Coverage` | push na `main` (Q-005) | — | `cargo llvm-cov nextest --locked --all-features --lcov`; artefato `lcov.info`; não bloqueia o deploy |
| `build` | `Build image` | push na `main` | `lint`, `test`, `compose` | Buildx com cache `gha`; `docker run` + `curl /health/ready` (smoke); `docker save \| gzip -9 > engramind-image.tar.gz`; `sha256sum`; artefato (3 dias, `compression-level: 0`) |
| `deploy` | `Deploy production` | push na `main` | `build` | Environment `production`; `concurrency: deploy-production` sem cancelamento; SSH com `known_hosts` fixo; `sha256sum -c`; `npx --yes tsx scripts/deploy-production.ts` |

Checks obrigatórios na proteção da `main`: `Lint`, `Test (1/4)`, `Test (2/4)`, `Test (3/4)`, `Test (4/4)`, `Compose`.

Secrets do Environment `production`: `DEPLOY_SSH_KEY`, `DEPLOY_KNOWN_HOSTS`, `DEPLOY_HOST`, `DEPLOY_SSH_USER`.
Variables: `DEPLOY_REMOTE_DIR`, `DEPLOY_PROJECT_NAME`, `DEPLOY_HEALTHCHECK_URL`, `DEPLOY_SSH_PORT` (22), `DEPLOY_SERVICE` (`api`), `DEPLOY_IMAGE_REPOSITORY` (`engramind-api`), `DEPLOY_PLATFORM` (`linux/amd64`), `DEPLOY_REQUIRED_NETWORKS` (`engramind-network,minio-network`).

`docker-compose.deploy.yml`:

```yaml
services:
  api:
    image: ${API_IMAGE:?defina API_IMAGE (repositório:tag)}
    pull_policy: never
```

`Dockerfile` (estágios): `chef` (`rust:1.98.0-bookworm` + `cargo-chef` de versão fixa) → `planner` (`cargo chef prepare`) → `builder` (`cargo chef cook --release --locked`, depois `cargo build --release --locked`) → `runtime` (`debian:bookworm-slim`, Litestream, link `engramind`, usuário sem privilégios, `HEALTHCHECK`).
