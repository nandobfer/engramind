# Implement - identidades e as três vias

**Data**: 2026-10-30

## Ordem real

1. Perguntas do `clarify.md` resolvidas pelas recomendações (Q-001 a Q-003), mais Q-005 (alcance do `LAST_ADMIN_TOKEN`) e Q-006 (PR único). Divergências corrigidas nas specs: o D-001 citava o prefixo `hk` (vale `eng` com id `tk_`); o desenho e o `AGENTS.md` diziam SHA-256 (vale HMAC-SHA256); o `plan.md` listava `hmac`/`sha2` como novas.
2. Dependências: `rand` 0.8, `rmcp` 3.4.0 (fixado no `Cargo.lock`, igual ao sap-middleman), `schemars` 1.2, `reqwest` 0.12 (rustls), `toml`, `dirs`.
3. `core::principal`, `core::operations` (`OPERATIONS`), `core::authz`, catálogo `ERROR_CODES` com o código de saída, `core::ids`, `core::pagination`, `auth::token` (portado).
4. Migração `0002_identities.sql`, `Store` estendido e a implementação SQLite (`identities.rs`, `settings.rs`, `journal.rs`), com auditoria, idempotência e a guarda `LAST_ADMIN_TOKEN` dentro da transação.
5. `Core<S: Store>` com `authenticate`, `mutate` (idempotência), identidades, configurações, auditoria, status, bootstrap, `usage` (rate limit e `last_used_at`) e as tarefas periódicas.
6. REST (`src/rest/`) com `#[utoipa::path]` e o snapshot; middleware `require_auth`.
7. Documentação viva: `catalog`, `openapi_index` e `search` portados do sap-middleman para `core::docs`, e os 7 guias.
8. MCP: `EngramindServer` com `#[tool_router]`, hints sobrescritos pelo catálogo, `list_tools`/`call_tool` filtrados e o transporte portado.
9. CLI: comandos por operação, cliente HTTP, perfis, `help --json` e saída legível.
10. Testes (integração, matriz nas três vias, MCP, CLI, logs, contrato) e documentação.

## Mudanças além do contrato

- `IssueTokenBody.app` é opcional quando `agent` é informado (D-018), como no exemplo `tokens issue --agent app-a/curador` do desenho.
- Repetição idempotente de `issue_token` sem `secret` (D-014) e repetição conferida antes de resolver os alvos (D-016).
- `TAG_NOT_FOUND` para `list_operations?tag=` desconhecida (o sap-middleman usava um erro genérico de parâmetro).
- `get_settings` aceita `?key=` para a CLI `settings get <chave>` ter a mesma regra nas três vias.
- Argumentos inválidos de ferramenta MCP viram `VALIDATION_ERROR` com `isError: true` (o `rmcp` 3.4 devolve texto solto).
- Rate limit em `core::usage`, não em `src/auth/rate_limit.rs` (D-020).
- `Core::new` é síncrono e `reload_settings` carrega as configurações depois das migrações, para o `TestContext` sem migração continuar funcionando.
- `profiles add` grava as flags globais `--url`/`--token`/`--channels`: flags com o mesmo nome no subcomando colidiriam com as globais no `clap` (D-019). Pelo mesmo motivo, o id posicional de `tokens update|revoke` se chama `TOKEN_ID`.
- `StoreError::Internal` passa a gerar `store.failed` em `ERROR` antes de virar `500`.
- Saída legível genérica da CLI (`output::human`): Markdown de `content`, listas em linhas com os campos principais e objetos em `chave: valor`.

## Comandos de verificação

```bash
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo nextest run --locked                           # 116 testes
UPDATE_SNAPSHOTS=1 cargo nextest run -E 'test(/contract::openapi_snapshot/)'
docker build --network host -t engramind-api:002 .
```

## Rascunho das release notes (0.2.0)

Identidades e as três vias: apps, agentes e tokens com escopos, bootstrap do primeiro admin, auditoria, idempotência, rate limit e configurações, expostos por REST (`/v1`), MCP (`POST /mcp`, ferramentas filtradas pelo token) e CLI (`engramind <grupo> <ação>`, perfis e `--json`), com a documentação viva servida pelo próprio binário e testes de contrato que travam as três vias daqui em diante.
