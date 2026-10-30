# Implement - fundação operável

**Data**: 2026-10-30

## Ordem real

1. Decisões pendentes do `clarify.md` (Q-001, Q-003 a Q-007) e correção das divergências das specs: segundo nome `eng` (D-003), `litestream ltx` no lugar de `snapshots` (D-009), decisões `0001`–`0006` no `plan.md`, `Store` com RPITIT (D-026). Porta `8000` registrada em `/home/used_ports`.
2. Ferramentas locais: toolchain 1.98.0, `cargo-nextest` 0.9.150, `cargo-llvm-cov` 0.9.1.
3. `Cargo.toml`, `rust-toolchain.toml`, lints, perfis e ignores. `rusqlite` ficou em 0.40, a versão exigida pelo `rusqlite_migration` 2.6.
4. `config.rs`, `observability.rs`, `error.rs` e `http.rs`, portados do sap-middleman com o envelope do desenho. `Settings::from_lookup` permite testar a configuração sem `env::set_var`, que é `unsafe` na edition 2024.
5. `store/` (trait, escritor, leitores, PRAGMAs, migrações, `vec_extension.rs`) e `migrations/0001_init.sql`. O spike (T002) virou o teste `store::sqlite::tests::sqlite_vec_and_fts5_are_available` e passou de primeira (`vec_version` v0.1.9, FTS5 com `remove_diacritics 2`, `vec0`).
6. `core/pepper.rs`, `health.rs`, `openapi.rs`, `app.rs`, `doctor.rs`, `cli/` e `main.rs`.
7. `tests/it/` (`health`, `store`, `contract`, `cli`) e o snapshot `docs/openapi.json`.
8. `Dockerfile` (cargo-chef), `docker/entrypoint.sh`, `litestream.yml`, compose, override de deploy, `.env.example` e `scripts/`.
9. `.github/workflows/ci-cd.yml` e `scripts/deploy-production.ts`.
10. Verificação manual com a imagem local e o MinIO do host; documentação; Environment `production` e proteção da `main`.

## Mudanças de código além do contrato

- `src/core/pepper.rs`: o verificador do pepper é regra de segurança e vive no `core`, genérico em `S: Store`. O módulo `core` passa a existir já na 001.
- `config::DatabaseSettings`: `migrate` e `doctor` só exigem `DATABASE_PATH`, `READ_POOL_SIZE` e `LITESTREAM_ENABLED`; `serve` exige a configuração completa.
- `observability::LogTarget`: `serve` registra logs no stdout; `migrate` e `doctor`, no stderr, para não misturar com a saída `--json`.
- `app::route_paths()`: ponto único de registro de rotas, comparado com o OpenAPI no teste de contrato.
- Body grande: um middleware recusa pelo `Content-Length` (`413` no envelope), e o `DefaultBodyLimit` cobre os extractors.
- `/health/ready` devolve `migrations: { applied, expected }`; o `503 MIGRATIONS_PENDING` leva os mesmos números em `details`.
- `litestream.yml` usa `replica:` no singular (a chave `replicas:` está obsoleta na v0.5) e `region: us-east-1` para o MinIO.
- `docker/entrypoint.sh` executa o comando recebido como argumento (`docker compose exec api eng doctor`) e exige as variáveis do S3 quando `LITESTREAM_ENABLED=true`.
- Os testes leem `NEXTEST_BIN_EXE_engramind` e `CARGO_MANIFEST_DIR` em tempo de execução, para funcionar nos shards (`--archive-file`).
- `scripts/deploy-production.ts`: cópia do `opex` com `OPEX_IMAGE` → `API_IMAGE`, padrões `api`/`engramind-api`/`engramind-network,minio-network` e as referências de documentação trocadas.

## Comandos de verificação

```bash
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo nextest run --locked                       # 43 testes
./scripts/check-compose.sh
UPDATE_SNAPSHOTS=1 cargo nextest run -E 'test(/contract::openapi_snapshot/)'
cargo nextest archive … && cargo-nextest nextest run --archive-file … --workspace-remap . --partition hash:i/4   # simulação dos shards
docker build --network host -t engramind-api:local .
```

## Rascunho das release notes (0.1.0)

Primeira versão executável do Engramind: binário `engramind`/`eng` com `serve`, `migrate` e `doctor`; health checks; OpenAPI e Swagger; SQLite com WAL, FTS5 e `sqlite-vec`; imagem Docker com backup contínuo por Litestream; CI/CD com testes em shards e deploy por artefato. Ainda sem capacidades de domínio.
