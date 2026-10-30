# Clarify - contexto

**Atualizado em**: 2026-10-30

## Perguntas

| ID | Tópico | Pergunta | Status | Resolução |
|----|--------|----------|--------|-----------|
| Q-001 | Corte da faixa 1 | Quando uma memória não cabe no orçamento de texto completo, as seguintes (menores) ainda podem entrar com texto completo? | aberta | Recomendação: não. O corte é na ordem: a primeira que não cabe abre a faixa 2, para que a ordem de prioridade nunca se inverta e o bloco seja previsível. |
| Q-002 | Orçamento por canal | O §12 permite sobrepor `budget_tokens` e `titles_budget_tokens` por canal. O que significa um orçamento por canal num bloco com vários canais? | aberta | Recomendação: teto da parcela daquele canal dentro de cada faixa; o orçamento global continua valendo para o bloco inteiro. |
| Q-003 | Contexto de token `admin` | Um token `admin` sem `channels` recebe o contexto de todos os canais? | aberta | Recomendação: sim, todos os canais ativos, como se fossem `inject`; o guia recomenda sempre passar `channels`. |

## Decisões

| ID | Decisão | Fonte | Impacto |
|----|---------|-------|---------|
| D-001 | `version` = `cv_` + 12 hex de SHA-256 sobre: formato; lista ordenada de `(channel_id, channel.version, mode, priority)` alcançados; hash das configurações `context.*` e `categories.list` efetivas. | desenho §8 | Determinística entre processos e reinícios. |
| D-002 | Na renderização, quebras de linha do texto viram espaço e todo `<` seguido (com ou sem `/`) de um nome de tag do bloco (`memories`, `channel`, `titles`, `overflow` ou uma chave de categoria) é escrito como `&lt;`. Nada mais é alterado. | desenho §18 (injeção) | Uma memória não forja estrutura do bloco. |
| D-003 | Cache LRU próprio (capacidade 1024 entradas) em `core::context::cache`; sem crate nova. | desenho §8 | — |
| D-004 | `usage_log` grava `get_context` e `search_memories` (agente, operação, canais, ids, tokens) em lote; retenção aplicada na 008. | desenho §6 | Tabela nasce aqui. |

## Bloqueios

Q-001 a Q-003 antes da implementação do renderizador.
