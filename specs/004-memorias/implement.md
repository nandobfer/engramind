# Implement - memórias

**Data**: 2026-10-30

## Ordem real

1. Perguntas Q-001 a Q-004 resolvidas pelas recomendações, mais Q-005 a Q-017, abertas no planejamento (`clarify.md`, D-006 a D-013). O limiar de `similar` ficou configurável (`memory.similar_threshold`, 0 a 1), como pedido.
2. Migração `0004_memories.sql`: `memories` com `seq` explícito (rowid do FTS), `memory_revisions`, `memory_fts` com triggers de insert/delete/update de `title`/`text`, e `jobs`.
3. Funções puras em `core::memory_text` (NFC, `content_hash`, título provisório, termos sem acento, Jaccard), com a dependência `unicode-normalization`.
4. Configurações: tipos `number`, `boolean` e `categories`, as chaves `categories.list` e `memory.*`, leitores tipados no `SettingsSnapshot`, `CATEGORY_IN_USE` conferido na transação do `update_settings` e as sobreposições por canal em `channels.settings_json` (no cache da rede).
5. `Store`: `src/store/sqlite/memories.rs` com a transação única de cada mutação (memória, revisão, auditoria sem texto, versões da memória e do canal, jobs e idempotência), leitura, histórico, busca BM25, estatísticas, contadores e a extensão de `delete_channel`. `journal::remember_as` grava a duplicata com status 200.
6. `core::memories` (acesso por `require_channel`, `404 MEMORY_NOT_FOUND` fora do alcance, validação, `similar`, conflitos com detalhes), `core::search`, `core::counters` (flush a cada 5 s e no shutdown), `channel_stats` e as configurações por canal.
7. Catálogo (16 operações; tag `Memories`; construtor `any_mutation`; `search_memories` como leitura por POST), códigos de erro, REST com `#[utoipa::path]`, ferramentas MCP e CLI (`memories`, `channels stats`, `channels settings get|set`, `--text -`).
8. `doctor` com `integrity-check` do `memory_fts`.
9. Testes: unitários (`memory_text`, `memories`, `settings`, `counters`), módulo de integração `tests/it/memories.rs`, log sem texto em `tests/it/logging.rs`, matriz com as 16 operações, contrato da skill e das configurações tipadas, snapshot do OpenAPI.
10. Documentação: guia `escrever-memorias`; guias `erros`, `cli`, `configuracoes`, `canais-e-assinaturas` e `overview`; skill; `aicontext/memories.md`, `settings.md`, `channels.md` e `README.md`; decisões `0011` e `0012`; desenho §6, §7 e §9; versão 0.4.0 e `CHANGELOG.md`.

## Mudanças além do contrato

- `memories.seq INTEGER PRIMARY KEY` com `id TEXT UNIQUE` (D-006).
- `MemoryTarget` protege a escrita contra a corrida com `move` (D-007).
- Toda mutação de memória invalida o cache da rede, porque muda `channels.version` (D-009).
- `revert` só troca o título quando o texto muda; conteúdo igual não grava nada.
- `restore_memory` só age sobre `archived` e `unpin_memory` só sobre `permanent`; nos outros status, nada muda.
- `SettingView.min`/`max` passam a ser números (`null` para `boolean`).
- `CLI_INPUT_INVALID` (saída 2) para falha ao ler `--text -`.

## Comandos de teste

```bash
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo nextest run -E 'test(/memories::|memory_text::|settings::|counters::|matrix::|contract::|logging::/)'
UPDATE_SNAPSHOTS=1 cargo nextest run -E 'test(/contract::/)'
cargo nextest run --locked   # suíte completa; ./scripts/test.sh usa llvm-cov, que não linka nesta máquina
```

## Rascunho de release notes

**0.4.0 — memórias.** Agentes e gestores gravam, editam, versionam, revertem, movem, arquivam, fixam, excluem e buscam memórias nas três vias. Toda mudança grava revisão, auditoria (sem o texto) e as versões da memória e do canal na mesma transação, com idempotência e `expected_version`. A escrita responde na hora com título provisório e `similar` (memórias parecidas do canal); o Redator e os embeddings ficam na fila para as próximas versões. A busca textual ignora acentos. Novas configurações `categories.list` e `memory.*`, com `memory.text_max` por canal; `channel_stats`; e a skill da CLI em `skills/engramind/SKILL.md`. Novos erros: `MEMORY_NOT_FOUND`, `REVISION_NOT_FOUND`, `CATEGORY_NOT_FOUND`, `CHANNEL_ARCHIVED`, `MEMORY_VERSION_CONFLICT`, `MEMORY_DUPLICATE` e `CATEGORY_IN_USE`.
