# 0005 — Testes e CI rápidos

**Data**: 2026-10-30 · **Etapa**: 001 (0.1.0) · **Status**: aceita

## Decisão

- `cargo-nextest`: um processo por teste (isolamento) e `test-threads = "num-cpus"` (`.config/nextest.toml`).
- Um único binário de integração, `tests/it/main.rs`, com um módulo amplo por domínio (`health`, `store`, `contract`, `cli`…) e helpers em `tests/it/support/`. Criar arquivo direto em `tests/` é proibido.
- `[lib] doctest = false`; `[[bin]] test = false`.
- Perfil de dev: `debug = "line-tables-only"`, dependências sem debug e `libsqlite3-sys` com `opt-level = 3`.
- Inicialização pesada é preguiçosa: o OpenAPI é um `LazyLock`.
- CI: `Build tests` compila uma vez (`cargo nextest archive`) e os 4 shards (`Test (i/4)`, `--partition hash:i/4`) só executam o arquivo, sem toolchain de compilação, com `--workspace-remap .`. Lint e Compose rodam em paralelo.
- Os testes leem, em tempo de execução, `CARGO_MANIFEST_DIR` (o snapshot do OpenAPI) e `NEXTEST_BIN_EXE_engramind` (o binário), porque os caminhos de compilação não existem no shard.
- Cache: `Swatinem/rust-cache` com `shared-key` por job e `save-if` só na `main`.
- Cobertura (`cargo llvm-cov nextest`) só em push na `main`, em paralelo ao build e sem bloquear o deploy (Q-005).
- Localmente, só os testes afetados (`cargo nextest run -E 'test(/<módulo>::/)'`); a suíte completa (`./scripts/test.sh`) só a pedido.
