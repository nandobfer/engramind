# Report - canais e assinaturas

**Data**: 2026-10-30 · **Versão**: 0.3.0 · **Branch**: `feat/003-canais`

## Entrega

FR-001 a FR-014 implementados (detalhes em `implement.md`). As perguntas Q-001 a Q-012 foram resolvidas pelas recomendações (`clarify.md`).

## Verificação executada

| Verificação | Resultado |
|---|---|
| `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings` | ok |
| `cargo nextest run --locked` | 142 testes, todos passam |
| Matriz tipo × escopo × operação | 8 tokens × 37 operações em REST, MCP e CLI |
| Cenário do §19 (critério de sucesso) | `channels::the_design_example_is_built_by_the_cli_and_seen_alike_in_the_three_ways`: rede montada pela CLI; `list_channels` de cada um dos 5 agentes igual (canal, modo, acesso) em REST, MCP e CLI; seleção pelo cabeçalho, interseção com `channels` e `CHANNEL_NOT_SUBSCRIBED` nas três vias |
| Resolvedor | Unitários de exato × prefixo, especificidade, `access` restritivo, `manage` explícito, admin, arquivados, seleção e cache por geração |
| Integração | CRUD, `version`, auditoria, arquivamento, exclusão só de arquivado, idempotência, paginação, validação de alvos, `SUBSCRIPTION_EXISTS`, cascatas (`delete_agent`, `delete_app`, `delete_channel`) e revogação valendo na requisição seguinte |
| Contrato §15 | OpenAPI regenerado; catálogo × REST × MCP × CLI × guias × erros |

## Pendências

- PR #4 com o CI verde (lint, build, 4 shards, compose); merge e deploy pendentes.
- `SCOPE_REQUIRED` com `details.channel` é coberto pelos testes unitários do resolvedor. Nas três vias, ele só aparece quando houver operação que exija `write`/`delete` num canal (memórias, 004).
- `CHANNEL_ARCHIVED`, a exclusão das memórias junto com o canal (Q-002) e os webhooks `channel.updated`/`subscription.updated` ficam para a 004 e a 008.
