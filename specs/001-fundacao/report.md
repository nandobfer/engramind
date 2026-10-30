# Report - fundação operável

**Data**: 2026-10-30 · **Versão**: 0.1.0 · **PR**: #1

## Entrega

Todos os requisitos FR-001 a FR-023 estão implementados (ver `implement.md`). Faltam o primeiro deploy na `main` e a cobertura, que só rodam depois do merge.

## Verificação executada

| Verificação | Resultado |
|---|---|
| `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings` | ok |
| `cargo nextest run` (local) | 43 testes, todos passam |
| Simulação dos shards (`nextest archive` + `--workspace-remap` + `--partition hash:i/4`, sem o `target/` original) | 43 passam |
| `./scripts/check-compose.sh` | ok |
| `docker build` local e compose com `LITESTREAM_ENABLED=false` | `/health/ready` 200; `/` → `303 docs/`; `/docs/` 200; `eng doctor` ok; usuário `engramind` |
| Compose com `LITESTREAM_ENABLED=true` (MinIO do host) | réplica criada; `doctor` mostra a idade da última réplica |
| Restore depois de apagar o volume | banco restaurado da réplica (sem nova migração nem novo registro do pepper) |
| Pepper divergente sobre o banco restaurado | startup recusado com `application.failed` |
| CI no PR #1 | `Lint`, `Build tests`, `Test (1/4)`…`Test (4/4)` e `Compose` verdes |

Os dados do teste de restore (`engramind/verify-local`) foram apagados do bucket.

## Medições do primeiro PR (T040)

| Job | Duração |
|---|---|
| `Build tests` (compilação única, cache ainda vazio: `save-if` só na `main`) | 1m49s |
| `Test (i/4)` (só execução) | 8–11 s cada |
| `Lint` | 1m41s |
| `Compose` | 5 s |

Com 43 testes, cada shard gasta mais tempo subindo o job do que executando. A lista da matriz pode cair para 2 sem perda de tempo, mas fica em 4, como decidido, até a suíte crescer.

## Problema encontrado e corrigido

- No primeiro CI, `/docs/` respondia 404 nos shards: em build de dev, o `rust-embed` da Swagger UI lê os arquivos do `target/` da compilação, que não existe no shard. A correção foi a feature `debug-embed` (decisão 0001).

## Pendências

- `cargo llvm-cov` não linka nesta máquina, porque o `cc` local é um wrapper do `zig cc`. A cobertura roda só no job `Coverage` da `main`.
- `/home/engramind` (com o `.env` de produção) precisa existir antes do primeiro deploy.
- `actions/upload-artifact` e `actions/download-artifact` v5 (os mesmos SHAs do `opex`) rodam em Node 20, que está obsoleto; o GitHub força Node 24 e emite um aviso.
