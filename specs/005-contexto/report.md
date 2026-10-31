# Report - contexto

**Data**: 2026-10-30 · **Versão**: 0.5.0

## Entrega

FR-001 a FR-012 implementados (detalhes em `implement.md`). As perguntas Q-001 a Q-023 foram resolvidas (`clarify.md`).

## Verificação executada

| Verificação | Resultado |
|---|---|
| `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings` | ok |
| `cargo nextest run --locked` | 207 testes, todos passam |
| Snapshot do §8 (critério de sucesso) | O bloco `tagged` do desenho reproduzido literalmente a partir de fixtures, com `counts` 4/2/12; `markdown` e `json` do mesmo exemplo |
| Cortes (critério de sucesso) | Primeira que não cabe fecha a faixa, cortes exatos, orçamento zero, teto por canal, global com teto, `overflow_line` desligado mantendo a contagem |
| Estabilidade (critério de sucesso) | `version` e corpo idênticos depois de buscas e do flush dos contadores (`injected_count` > 0) |
| Invalidação (critério de sucesso) | `version` muda após criar, editar, fixar e arquivar memória, prioridade e modo da assinatura, prioridade do canal, orçamento por canal, configuração global e categorias; `default_format` não muda a de quem passa `format` |
| `304` nas três vias (critério de sucesso) | REST com aspas, sem aspas, `W/` e lista (`*` e outra versão dão 200); MCP `if_none_match` → `{ not_modified, version }`; CLI `--if-none-match` sem saída e 0, `--json` com `not_modified: true` |
| Alcance | Só `inject`; `search`/`manage` fora; `Engramind-Channels` e `channels`; admin sem filtro vê todos os ativos, arquivado fora |
| Escape | Texto com `</memories>`, `</channel>`, chaves de categoria em qualquer caixa vira `&lt;`; `<div>`, `Vec<String>`, `<channels>` intactos |
| `usage_log` | Linha com `at` RFC 3339 UTC, agente, canais, ids e tokens para o contexto entregue; busca registrada; `304` não grava; consulta por período |
| Busca | `context_version` igual à de `get_context`; `exclude_context` tira só a faixa 1 |
| MCP | `resources/list` (contexto, canais com `read`, guias visíveis, guia admin oculto), templates, `read` do contexto com `Engramind-Channels`, do canal e do guia, `-32002` para URI, canal e guia inexistentes e `CHANNEL_NOT_SUBSCRIBED` em `data`; prompt `memory-writing`; capacidades no `initialize` |
| Logs | `get_context` 200 em `INFO`, `304` em `DEBUG`, `mcp.resource.completed`, texto das memórias fora dos logs |
| Matriz tipo × escopo × operação | 8 tokens × 54 operações em REST, MCP e CLI |
| Contrato §15 | OpenAPI regenerado; argumentos MCP com `if_none_match`; configurações `enum` no guia; resources e prompt no guia `integracao-mcp`; skill e comandos dos guias em `help --json` |

## Pendências

- PR #7 com o CI verde (lint, build, 4 shards, compose), merge e deploy: produção na 0.5.0 com `/health/ready` 200 (5 de 5 migrações), contêiner `healthy` com a imagem do commit `5a7a750` e `eng doctor` todo ok; Coverage da `main` verde.
- O Redator (006) precisa incrementar `channels.version` ao aplicar só o título (FR-007 da 006 ajustado; teste de regressão na 006).
- `get_usage` e a retenção de `usage_log` chegam na 008; a tabela e os índices já existem.
- Custo do `tiktoken-rs` medido só em testes (cerca de 1,7 s para carregar a tabela em debug); sem medição em produção com milhares de memórias.
