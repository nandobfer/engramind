# Clarify - contexto

**Atualizado em**: 2026-10-30

## Perguntas

| ID | Tópico | Pergunta | Status | Resolução |
|----|--------|----------|--------|-----------|
| Q-001 | Corte da faixa 1 | Quando uma memória não cabe no orçamento de texto completo, as seguintes (menores) ainda podem entrar com texto completo? | resolvida | Não. A primeira que não cabe fecha a faixa 1 (e o mesmo vale para a faixa 2), para a ordem de prioridade nunca se inverter. |
| Q-002 | Orçamento por canal | O que significa `budget_tokens`/`titles_budget_tokens` por canal num bloco com vários canais? | resolvida | Teto da parte daquele canal em cada faixa: estourar o teto fecha a faixa só para o canal; estourar o global fecha para todos. Com as duas faixas fechadas, o resto é excedente sem contar tokens. |
| Q-003 | Contexto de token `admin` | Um token `admin` sem `channels` recebe o contexto de todos os canais? | resolvida | Sim, todos os canais ativos como `inject` (já é o alcance da 003, FR-007); o guia recomenda passar `channels`. |
| Q-004 | Custo no orçamento | O que conta tokens? | resolvida | A linha `tagged` (`#id: texto`/`#id: título`, já escapada), sem as tags de estrutura. A escolha é a mesma em todos os formatos; `rendered_tokens` mede o `content` final. |
| Q-005 | Ordem na saída | Como os grupos aparecem? | resolvida | Escolha pela ordem completa do FR-002; saída com canais por prioridade (desc) e slug, categorias por prioridade (desc) e chave, memórias na ordem da escolha; `<titles>` na ordem da escolha, sem canal. Canais e categorias vazios não aparecem; sem memórias, só o invólucro `<memories>`. |
| Q-006 | Layout do `markdown` | Como fica? | resolvida | `## <slug>`, `### <CHAVE>`, `- texto`; `## Títulos` com `- título`; `_+N memórias: use search_memories para o restante_`. Quebras de linha viram espaço. |
| Q-007 | `content` no `json` | Texto ou objeto? | resolvida | Objeto `{ channels: [{ slug, categories: [{ key, memories: [{ id, text }] }] }], titles: [{ id, title }], overflow }`, com o texto sem alteração. A CLI imprime o JSON formatado. |
| Q-008 | Hash da `version` | O que entra? | resolvida | Formato resolvido, `(channel_id, channels.version, modo, prioridade)` alcançados e os valores de `context.budget_tokens`, `context.titles_budget_tokens`, `context.token_estimator`, `context.overflow_line` e `categories.list`. `context.default_format` fica fora. |
| Q-009 | Chave do cache | Qual? | resolvida | O digest SHA-256 completo da `version` (sem colisão dos 12 hex), compartilhado entre tokens com o mesmo alcance; 1024 entradas. |
| Q-010 | Escrita entre a rede e a leitura | Como evitar um bloco rotulado com outra versão? | resolvida | No cache miss, o `Store` lê memórias e versões dos canais na mesma transação de leitura; a `version` devolvida e a chave do cache usam essas versões. O `304` usa a rede em cache. |
| Q-011 | `If-None-Match` | Que formas aceita? | resolvida | Comparação fraca da RFC 9110: lista separada por vírgula, `W/`, com ou sem aspas; `*` não gera `304`. O `304` leva `ETag`; toda resposta leva `Cache-Control: private, no-cache` e `Vary: Authorization, Engramind-Channels`. |
| Q-012 | `304` no MCP e na CLI | Qual o resultado? | resolvida | MCP: `{ not_modified: true, version }`. CLI cru: nada impresso, saída 0. CLI `--json`: o mesmo objeto do MCP, saída 0. |
| Q-013 | `injected_count` | Quando conta? | resolvida | Só em respostas com conteúdo (cache ou banco) e só na faixa 1; `304` e a faixa de títulos não contam. |
| Q-014 | `usage_log` | O que grava? | resolvida | Contexto entregue e buscas (o `304` não). Colunas `(id, at, app_id, agent_id NULL, token_id, op, channel_ids, memory_ids, rendered_tokens NULL)`; `at` RFC 3339 UTC com milissegundos, ordenável como texto, e índices `(at)`, `(agent_id, at)` e `(op, at)` para consultar por período e agrupar por dia (pedido da usuária). Gravação em lote a cada 5 s, até 50.000 linhas pendentes (as excedentes são descartadas com `WARN usage.dropped`). |
| Q-015 | `exclude_context` | O que exclui? | resolvida | Só a faixa 1 do contexto sem `format` do mesmo token e seleção, somada a `exclude_ids` (o limite de 500 vale só para os ids informados). A resposta sempre traz `context_version`. |
| Q-016 | Resource `engramind://context` | Formato e tipo? | resolvida | `context.default_format`; `text/plain` (`tagged`/`markdown`) ou `application/json`. Mesma autorização de `get_context`. |
| Q-017 | Resource `engramind://channels/{slug}` | O que traz? | resolvida | Slug cru com `/`; `#id: título` das `active`/`permanent` na ordem do contexto, até 500 linhas e uma linha final com o restante; `read` efetivo e a autorização de `list_memories`. `resources/list` mostra o contexto, um resource por canal alcançado com `read` e os guias visíveis; os modelos são `channels/{slug}` e `guides/{id}`. |
| Q-018 | Resource `engramind://guides/{id}` | Entra agora? | resolvida | Sim (desenho §13, sem etapa prevista). Guias `admin` ocultos para `agent`. |
| Q-019 | Prompt `memory-writing` | De onde vem? | resolvida | `docs/prompts/memory-writing.md` (`include_str!`), derivado do guia `escrever-memorias`; sem argumentos. |
| Q-020 | Erros de resource | Como aparecem? | resolvida | Erro JSON-RPC: `-32002` para `*_NOT_FOUND` e `CHANNEL_NOT_SUBSCRIBED`, `-32603` para falha interna, `-32600` para o resto; o envelope do domínio vai em `data`. Novos códigos `RESOURCE_NOT_FOUND` e `PROMPT_NOT_FOUND`. Evento `mcp.resource.completed` em `DEBUG`. |
| Q-021 | Limites | Intervalos das chaves? | resolvida | `budget_tokens` 0 a 200.000; `titles_budget_tokens` 0 a 50.000; ambos por canal. Novo tipo `enum` para `default_format` e `token_estimator`, com `choices` em `get_settings`. |
| Q-022 | Carga do `tiktoken-rs` | Quando? | resolvida | No primeiro uso; `serve` aquece com `spawn_blocking` depois de subir (diverge do `plan.md`). Unitários com estimador de palavras; integração com o real. |
| Q-023 | Layout do código | Onde? | resolvida | `src/core/context/{mod,order,budget,render,version,cache,estimator}.rs` (único subdiretório do `core`; o resto segue a 004 D-010). |

## Decisões

| ID | Decisão | Fonte | Impacto |
|----|---------|-------|---------|
| D-001 | `version` = `cv_` + 12 hex de SHA-256 sobre: formato resolvido; lista ordenada de `(channel_id, channel.version, mode, priority)` alcançados; valores das chaves de renderização (Q-008). | desenho §8 | Determinística entre processos e reinícios. |
| D-002 | Na renderização, quebras de linha do texto viram espaço e todo `<` seguido (com ou sem `/`) de um nome de tag do bloco (`memories`, `channel`, `titles`, `overflow` ou uma chave de categoria, sem diferenciar caixa, terminando o nome) é escrito como `&lt;`. Nada mais é alterado. No `markdown`, só as quebras; no `json`, nada. | desenho §18 (injeção) | Uma memória não forja estrutura do bloco. |
| D-003 | Cache LRU próprio (capacidade 1024 entradas) em `core::context::cache`; sem crate nova. | desenho §8 | — |
| D-004 | `usage_log` grava `get_context` (entregue) e `search_memories` em lote; retenção e `get_usage` na 008. | desenho §6 | Tabela nasce aqui. |
| D-005 | `core/usage.rs` já existe (rate limit e `last_used_at`); `injected_count` e `usage_log` ficam em `core/counters.rs`, gravados com os demais contadores. | código da 002/004 | Diverge do `plan.md` (`src/core/usage.rs`). |
| D-006 | O snapshot do §8 é um unitário puro (`core::context::tests`) com o estimador de palavras, comparado ao texto literal do desenho; os formatos `markdown` e `json` têm snapshots inline no mesmo módulo. | velocidade e determinismo | Diverge de `tests/snapshots/*` do `plan.md`. |
| D-007 | O teste de contrato dos argumentos MCP mapeia o cabeçalho `If-None-Match` para `if_none_match`; os demais cabeçalhos continuam fora. | `contracts.md` | Única exceção. |
| D-008 | O Redator (006) incrementa `channels.version` também quando aplica só o título (ajuste no FR-007 da 006). | Q-010 | Senão o cache entrega título antigo. |

## Bloqueios

Nenhum.
