# Spec - contexto

**Criado em**: 2026-10-30
**Status**: Implementado (0.5.0)
**Versão alvo**: 0.5.0 — **marco: MVP do primeiro consumidor**
**Input**: Desenho §8, §12 (`context`), §13 (MCP resources e prompts), §19 (consumidor).

## Objetivo

Entregar o bloco de memórias pronto para o prompt, determinístico e estável para o cache do provedor de LLM, com `version`/ETag e `304`, nas três vias e como resource MCP. Com esta etapa um sistema consumidor já substitui a coleção de memórias que usa hoje (com títulos provisórios até a 006).

## Escopo

- `get_context` nos formatos `tagged` (padrão), `markdown` e `json`.
- Ordenação determinística, duas faixas de orçamento e linha de excedente.
- `version`, `ETag`, `If-None-Match` → `304`; cache LRU em memória.
- Configurações `context.*` com sobreposição por canal.
- `injected_count` e `usage_log` em lote.
- `exclude_context` em `search_memories`.
- MCP: resources `engramind://context` e `engramind://channels/{slug}`; prompt `memory-writing`.
- Guia `contexto-e-cache` e primeira versão do guia `integracao-consumidor`.

## Requisitos

| ID | Requisito |
|----|-----------|
| FR-001 | Entram as memórias `active` e `permanent` dos canais alcançados em modo `inject` (token `admin`: todos os canais ativos), restritos por `channels`/`Engramind-Channels`. |
| FR-002 | Ordem: prioridade da assinatura ou do canal (desc); `permanent` primeiro; prioridade da categoria; `importance` (desc, só quando classificada; não classificadas depois); `created_at`; `id`. |
| FR-003 | Faixa 1: texto completo até `context.budget_tokens`; faixa 2: `#id: título` até `context.titles_budget_tokens`; o resto vira `<overflow count="N">` quando `context.overflow_line`. Regra de corte em Q-001. |
| FR-004 | Tokens estimados com `tiktoken-rs` (`o200k_base` por padrão, `context.token_estimator`). |
| FR-005 | Resposta `{ content, version, counts: { full, titles, overflow }, rendered_tokens, format }` com `ETag: "<version>"`; `If-None-Match` igual → `304` sem corpo. |
| FR-006 | `version` muda só com mudança real: versão de algum canal selecionado, conjunto de canais alcançados (assinaturas, modos, prioridades), configurações de renderização ou categorias. Contadores nunca mudam `version`, ordem ou conteúdo. |
| FR-007 | O bloco `tagged` segue exatamente o exemplo do §8 do desenho; o `markdown` não traz ids; o `json` traz `channels[].categories[].memories[{ id, text }]`, `titles[]` e `overflow`. |
| FR-008 | Texto de memória não consegue fechar ou abrir as tags do bloco (D-002). |
| FR-009 | Sem mudança, `get_context` só lê as versões dos canais (cache LRU por `token, seleção, versões, hash de configurações, formato`). |
| FR-010 | `get_context` com `304` e sem falha é logado em `DEBUG`. |
| FR-011 | `search_memories { exclude_context: true }` exclui os ids presentes no contexto do mesmo token e seleção; a resposta traz `context_version`. |
| FR-012 | `engramind context` imprime `content` cru (para colar no prompt) e, com `--json`, o objeto completo; aceita `--format` e `--if-none-match`. |

## Fora do escopo

- `get_hints` (007), `importance` (009), webhooks (008).

## Critérios de sucesso

- Snapshot do exemplo `tagged` do §8 reproduzido a partir de fixtures.
- Cortes exatos nas duas faixas e `overflow` com contagem correta.
- `version` idêntica entre chamadas sem mudança, inclusive após buscas que incrementam contadores; muda após cada mutação de memória, assinatura, canal e configuração de renderização.
- `304` nas três vias (REST com cabeçalho, MCP com parâmetro `if_none_match`, CLI com `--if-none-match` e código de saída 0 com `not_modified: true`).
