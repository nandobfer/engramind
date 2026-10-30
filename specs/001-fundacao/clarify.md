# Clarify - fundação operável

**Atualizado em**: 2026-10-30

## Perguntas

| ID | Tópico | Pergunta | Status | Resolução |
|----|--------|----------|--------|-----------|
| Q-001 | Envelope | Seguir o envelope do desenho (`{ error: { code, message, details? } }`, sucesso sem `{ success, data }`) ou o do sap-middleman (`{ success, error, code, details }` e `{ success, data }`)? | aberta | Recomendação: desenho. É contrato documentado do Engramind e o sap-middleman é referência só onde o desenho não decide. |
| Q-002 | Rede Docker | Quais redes externas o serviço usa? | resolvida | D-006. |
| Q-003 | Porta | Qual `APP_PORT` padrão? | aberta | Recomendação: `4300` (desenho §16); está livre em `/home/used_ports`. Registrar a porta lá ao implantar. |
| Q-005 | Cobertura no CI | O `cargo llvm-cov` do sap-middleman recompila tudo instrumentado e roda a suíte inteira em série de shards; no PR isso anula o ganho dos shards. | aberta | Recomendação: cobertura só em push na `main`, em job paralelo ao build e que não bloqueia o deploy (relatório `lcov` como artefato); no PR, só lint, shards e compose. `scripts/test.sh` continua gerando o resumo de cobertura localmente. |
| Q-006 | Workflow de prévia | O `opex` tem `preview.yml` (`workflow_dispatch` para as prévias do desenvolvimento agêntico). O Engramind terá prévias pelo Opex? | aberta | Recomendação: não nesta etapa; quando o Engramind entrar no fluxo de prévias do Opex, aplicar a skill `setup-preview-workflow`. |
| Q-007 | Servidor de produção | Em qual servidor, diretório e usuário o deploy acontece? | aberta | Recomendação: o mesmo host dos serviços em `/home` (`DEPLOY_REMOTE_DIR=/home/engramind`, projeto `engramind-4300`); valores reais só como Secrets/Variables do Environment `production`. |
| Q-004 | Destino do backup | Litestream replica para qual S3? | aberta | Recomendação: MinIO do host (`http://minio:9000`, rede `minio-network`), bucket próprio do Engramind criado manualmente; credenciais só no `.env`. |

## Decisões

| ID | Decisão | Fonte | Impacto |
|----|---------|-------|---------|
| D-001 | Toolchain 1.98.0, edition 2024, `rust-version = "1.98"`, componentes `clippy`, `llvm-tools-preview`, `rustfmt`. | sap-middleman | Mesma base do serviço em produção. |
| D-002 | Lints iguais ao sap-middleman, com `unsafe_code = "deny"` e um único `#[allow(unsafe_code)]` em `src/store/sqlite/vec_extension.rs`. | desenho §3 | Exceção documentada em `docs/decisions/0004`. |
| D-003 | Um binário `engramind`; a imagem instala o link `engramind -> engramind`. O `clap` aceita os dois nomes e a mesma árvore de comandos (`serve`, `migrate`, `doctor` e, a partir da 002, os comandos de cliente). | desenho §3 e §13 | Um artefato só; `engramind serve` e `engramind serve` são equivalentes. |
| D-004 | `Store` é uma trait com `async fn` (sem `dyn`); o `core` é genérico em `S: Store` e o `AppState` usa o alias `type AppStore = SqliteStore`. Testes usam o próprio `SqliteStore` em `tempdir`, nunca um falso. | desenho §3, §21 | Troca futura para Postgres altera só o alias e a implementação. |
| D-005 | Escritor: uma `std::thread` dona da `rusqlite::Connection`, alimentada por `tokio::sync::mpsc` com closures `FnOnce(&Transaction) -> Result<R>` e resposta por `oneshot`. Leitores: pool próprio de `READ_POOL_SIZE` conexões somente leitura (padrão 4) com `tokio::sync::Semaphore` e `spawn_blocking`. Sem `r2d2`. | desenho §16 | Uma dependência a menos; contrato de escrita serializada explícito. |
| D-006 | Redes do compose: `engramind-network` (externa, nome por `ENGRAMIND_NETWORK`) para os consumidores alcançarem `http://${CONTAINER_NAME}:${APP_PORT}`, e `minio-network` (externa, nome por `S3_NETWORK`) para o Litestream. Build com `network: host` (Docker rootless). | padrão do host: cada serviço compartilhado tem `<serviço>-network` externa (`mongodb-network`, `soketi-network`, `minio-network`, `postgresql-network`, `mysql-network`) e os consumidores (`opex`, `dndicas`) se juntam a ela; o build usa `network: host` | Antes do primeiro `up`: `docker network create engramind-network`. Consumidores adicionam a rede ao próprio compose. |
| D-007 | `restart: unless-stopped`, limites `cpus: '2.0'` e `memory: 2G` (como o sap-middleman), `init: true`, `image: ${API_IMAGE:-engramind-api:latest}`. | desenho §16, sap-middleman | — |
| D-008 | Migrações com `rusqlite_migration`, SQL em `migrations/NNNN_nome.sql` via `include_str!`. `schema_meta(key, value)` guarda o pepper check (HMAC de um contexto fixo, como `0002_token_pepper_check.sql` do sap-middleman). | desenho §6, sap-middleman | Pepper trocado é detectado no startup. |
| D-009 | `doctor` lê a idade da réplica com `litestream snapshots`/`ltx` do binário instalado; sem Litestream, informa "sem backup". | desenho §16 | Exige o binário no contêiner; fora dele o item é pulado. |
| D-010 | Erros de startup são logados com `event = "application.failed"` e o processo sai com código ≠ 0, como `main.rs` do sap-middleman. | sap-middleman | — |

| D-011 | **Shards como no `opex`/`dndicas`**: matriz `shard: [1, 2, 3, 4]`, check `Test (i/4)`, `concurrency` com o shard no grupo e `cancel-in-progress` só em PR, `fail-fast: false`. Para mudar a quantidade, edita-se só a lista. | `opex` e `dndicas` `ci-cd.yml` | A proteção da `main` exige os 4 checks. |
| D-012 | **Compilar uma vez, rodar em shards.** Em Rust o custo dominante é compilar, não executar; recompilar em cada shard anularia o paralelismo. Um job `Build tests` roda `cargo nextest archive` com cache e publica o arquivo; cada shard baixa e roda `cargo nextest run --archive-file … --partition hash:i/4` (hash: o shard de cada teste não muda quando outros testes entram). | equivalente Rust do `--shard` do Vitest | Shards levam segundos de execução, sem toolchain de compilação. |
| D-013 | **Workers**: `.config/nextest.toml` fixa `test-threads = "num-cpus"` com comentário, como o `maxWorkers: "100%"` do `opex`/`dndicas` (explícito para não depender de padrão). | `opex`/`dndicas` `vitest.config.ts` | — |
| D-014 | **Isolamento mantido**: nextest roda cada teste em processo próprio — o equivalente do `isolate: true` que `opex`/`dndicas` mediram e mantiveram. Nenhum teste depende de estado global; cada um abre o próprio SQLite em `tempdir`. | `opex`/`dndicas` (recusa de `isolate: false`) | Estado global (`OnceLock`) só para dados imutáveis. |
| D-015 | **Poucos arquivos amplos em vez de micro arquivos**: em Rust cada arquivo em `tests/` é um crate e um binário linkado à parte. Há um único `tests/it/main.rs` com um módulo amplo por domínio (`identities.rs`, `channels.rs`, `memories.rs`, `context.rs`, `contract.rs`…) e helpers em `tests/it/support/`. Criar arquivo novo direto em `tests/` é proibido. | equivalente Rust dos arquivos amplos do `opex`/`dndicas` | Um link só para toda a integração. |
| D-016 | **Import sob demanda** = inicialização preguiçosa: OpenAPI (`LazyLock`), BPE do `tiktoken`, modelo local e clientes de provedor só são criados no primeiro uso; o `AppState` de teste não cria provedores que o teste não usa. | `opex` "Imports pesados entram sob demanda" | FR-023. |
| D-017 | **Ferramenta mais rápida** (o equivalente de Vitest 5 + happy-dom): `cargo-nextest` no lugar do `cargo test`; linker `rust-lld` (padrão do Rust ≥ 1.90 em `x86_64-unknown-linux-gnu`); `[profile.dev] debug = "line-tables-only"` e `[profile.dev.package."*"] debug = false`; `opt-level = 3` em `libsqlite3-sys` no perfil de dev/teste (SQLite compilado otimizado deixa os testes de banco mais rápidos); `[lib] doctest = false`; `[[bin]] test = false`. | medição na 001 | Ajustes registrados em `docs/decisions/0005-testes-e-ci-rapidos.md`. |
| D-018 | **Cache**: `Swatinem/rust-cache` com `shared-key` por job e `save-if` só na `main` (PRs leem, não sobrescrevem). Imagem com `cargo-chef` (camada de dependências) e cache do Buildx em `type=gha`; o `RUN --mount=type=cache` do sap-middleman não persiste entre runners. | sap-middleman + `opex` | Diverge do `Dockerfile` do sap-middleman só no estágio de dependências. |
| D-019 | **CD = skill `setup-ci-cd` literal** (padrão `opex`): build no runner, `docker save \| gzip -9`, `sha256sum`, artefato por 3 dias, deploy com `scripts/deploy-production.ts` (`npx --yes tsx`), `docker-compose.deploy.yml` com `pull_policy: never`. Única adaptação: a variável da imagem é `API_IMAGE` (a mesma do `docker-compose.yml` do desenho) em vez de `OPEX_IMAGE`. | skill `setup-ci-cd`, `opex` | Node só no job de deploy (`package-manager-cache: false`). |
| D-020 | **Variáveis do deploy**: `DEPLOY_SERVICE=api`, `DEPLOY_IMAGE_REPOSITORY=engramind-api`, `DEPLOY_REQUIRED_NETWORKS=engramind-network,minio-network`, `DEPLOY_HEALTHCHECK_URL=http://127.0.0.1:<APP_PORT>/health/ready` (o `curl` roda no servidor). | skill `setup-ci-cd` | — |
| D-021 | **Uma instância por banco no deploy**: o Compose recria o contêiner (para o antigo, sobe o novo), então nunca há dois processos no mesmo volume. Rollback de imagem só é seguro quando nenhuma migração nova foi aplicada; com migração nova, o rollback é restaurar do Litestream. Migrações são sempre aditivas. | desenho §16 | Registrado no guia `operacao`. |
| D-022 | **Rodando os testes localmente** (regra do `AGENTS.md` mantida): só os afetados — `cargo nextest run -E 'test(/<módulo>::/)'` ou `cargo nextest run <filtro>`; suíte completa (`./scripts/test.sh`) só a pedido. | `opex` §6 "Rodando os testes" | Documentado em `aicontext/` quando os comandos existirem. |

## Bloqueios

Nenhum bloqueio de código. A validação do backup depende de um bucket no MinIO e de credenciais com acesso a ele.

## Notas

O sap-middleman usa `chrono` e `uuid`; o Engramind adota os dois (timestamps RFC 3339 em UTC no banco; UUID v4 no `x-request-id`).
