# 0003 — Binário único `engramind` (também `eng`)

**Data**: 2026-10-30 · **Etapa**: 001 (0.1.0) · **Status**: aceita

## Decisão

- Um binário, `engramind`, com a árvore `clap`: `serve`, `migrate` e `doctor` nesta etapa; os comandos de cliente HTTP chegam a partir da 002.
- A imagem instala o link `/usr/local/bin/eng -> engramind`. Os dois nomes aceitam a mesma árvore; `eng serve` e `engramind serve` são equivalentes.
- Só `serve`, `migrate` e `doctor` abrem o banco. `migrate` e `doctor` leem apenas `DATABASE_PATH`, `READ_POOL_SIZE` e `LITESTREAM_ENABLED`; `serve` exige a configuração completa.
- `--json` é global. Toda escrita no terminal passa por `src/cli/output.rs`, o único módulo com `print_stdout`/`print_stderr` liberados. `migrate` e `doctor` registram logs no stderr para não misturar com a saída; `serve` registra no stdout (logs do contêiner).
- Falha de startup: `event = "application.failed"` e código de saída ≠ 0. Se a observabilidade ainda não subiu (ex.: `LOG_FORMAT` inválido), o erro vai para o stderr pela saída da CLI.
- `doctor` sai com código ≠ 0 quando algum item falha.
