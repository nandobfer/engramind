# Report - identidades e as três vias

**Data**: 2026-10-30 · **Versão**: 0.2.0 · **Branch**: `feat/002-identidades`

## Entrega

FR-001 a FR-019 implementados (detalhes em `implement.md`). As perguntas Q-001 a Q-003 foram resolvidas pelas recomendações; Q-005 e Q-006 foram abertas e resolvidas durante a etapa (`clarify.md`, D-014 a D-021).

## Verificação executada

| Verificação | Resultado |
|---|---|
| `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings` | ok |
| `cargo nextest run --locked` | 116 testes, todos passam |
| Matriz tipo × escopo × operação | 8 tokens × 26 operações em REST, MCP e CLI |
| Contrato §15 (itens 1 a 6) | OpenAPI, catálogo × REST × MCP (nomes, hints, parâmetros) × CLI × guias × erros × configurações |
| `./scripts/check-compose.sh` | ok |
| `docker build` + contêiner com bootstrap | `/health/ready` 200, `whoami` com o token do bootstrap e `tools/list` pelo MCP |
| Critério de sucesso (banco vazio + bootstrap → app, agente e token `agent` pela CLI → `whoami`) | coberto por `cli::serve_bootstraps_the_first_admin_and_the_cli_provisions_an_agent` (binário real) e, pelo MCP, por `mcp::hidden_tools_and_domain_errors_return_the_rest_envelope` |

## Pendências

- PR, CI nos shards e deploy ainda não executados.
- A skill (`skills/engramind/SKILL.md`) e o teste de contrato dela entram na 004, como previsto.
- `fields` (`title`/`full`) e `channels` nas listas só fazem sentido com memórias e canais (003/004).
