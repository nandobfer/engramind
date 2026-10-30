# Report - memórias

**Data**: 2026-10-30 · **Versão**: 0.4.0

## Entrega

FR-001 a FR-016 implementados (detalhes em `implement.md`). As perguntas Q-001 a Q-017 foram resolvidas pelas recomendações (`clarify.md`).

## Verificação executada

| Verificação | Resultado |
|---|---|
| `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings` | ok |
| `cargo nextest run --locked` | 167 testes, todos passam |
| Matriz tipo × escopo × operação | 8 tokens × 53 operações em REST, MCP e CLI |
| Invariantes de escrita | Memória, revisão, auditoria sem texto, versões da memória e do canal e jobs conferidos no banco; falha injetada (trigger em `jobs`) desfaz tudo |
| Duplicata, idempotência e conflito | `duplicate: true` com `200` (também com `Idempotency-Key`), `dedupe_exact` desligado, `MEMORY_VERSION_CONFLICT` com `current_version`, `MEMORY_DUPLICATE` na edição e no `move` |
| `revert`, `move`, `delete`, status | Revisão nova com `title_only` no `redact`; `REVISION_NOT_FOUND`; `write` nos dois canais e as duas versões; última revisão preservada; status repetido sem gravar; `CHANNEL_ARCHIVED` |
| Busca (critério de sucesso) | "configuracao" encontra "configuração"; `manage` só citado; categorias, `exclude_ids`, `include_archived`, `fields=title`; índice acompanha a edição; contadores sem mudar `version` |
| Listas e acesso | Filtros `status`, `channels`, `author`, `since`, `fields`, paginação; `MEMORY_NOT_FOUND` fora do alcance e fora do `Engramind-Channels` |
| Configurações | `text_max` por canal, `SETTING_UNKNOWN` para chave global, `similar_threshold` configurável, `CATEGORY_IN_USE`, `delete_channel` levando memórias e revisões, `memory_fts` íntegro no `doctor` |
| Três vias | CLI com `--text -`, `revert --version`, saída 6 no conflito, busca com `--fields`; MCP com busca, conflito e duplicata |
| Logs | O texto das memórias, das edições e das consultas não aparece nos logs (REST e MCP) |
| Contrato §15 | OpenAPI regenerado; catálogo × REST × MCP × CLI × guias × erros × configurações × skill |

## Pendências

- PR, CI, merge e deploy.
- Fluxo do curador do §5 seguindo só a skill: coberto pelos comandos testados (`channels list`, `memories list --fields title`, `memories create --origin-source`), sem um teste de ponta a ponta com um agente real.
- Workers de `redact`/`embed` (006/007), webhooks e `import_memories` (008).
