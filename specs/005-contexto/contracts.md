# Contracts - contexto

**Criado em**: 2026-10-30
**Status**: Draft
**Feature**: [`spec.md`](spec.md)

## Migração `0005_usage.sql`

```sql
usage_log (id INTEGER PK, at, agent_id NULL, token_id, op, channel_ids TEXT, memory_ids TEXT, rendered_tokens INTEGER NULL)
CREATE INDEX usage_log_at ON usage_log(at);
```

## Core

```rust
pub enum ContextFormat { Tagged, Markdown, Json }
pub struct ContextRequest { pub selection: ChannelSelection, pub format: Option<ContextFormat>, pub if_none_match: Option<String> }
pub enum ContextResponse { NotModified { version: String }, Rendered(RenderedContext) }
pub struct RenderedContext { pub content: ContextContent, pub version: String, pub counts: Counts, pub rendered_tokens: u32, pub format: ContextFormat }
pub trait TokenEstimator: Send + Sync { fn count(&self, text: &str) -> u32; }
```

## Operação

| Operação | Tipo·escopo | REST | CLI |
|---|---|---|---|
| `get_context` | any·read | `GET /v1/context?channels=&format=` (+ `If-None-Match`) | `engramind context [--format] [--if-none-match]` |

## MCP

- Ferramenta `get_context { channels?, format?, if_none_match? }`.
- Resource `engramind://context` (contexto do token com o `Engramind-Channels` da conexão, `text/plain`).
- Resource template `engramind://channels/{slug}` (lista `#id: título` das memórias do canal alcançado; `text/plain`).
- Prompt `memory-writing` (texto curto do guia `escrever-memorias`).
- Resources e prompt também são filtrados pelo token.

## Configurações desta etapa

| Chave | Tipo | Padrão | Por canal |
|---|---|---|---|
| `context.budget_tokens` | inteiro ≥ 0 | 4000 | sim |
| `context.titles_budget_tokens` | inteiro ≥ 0 | 800 | sim |
| `context.default_format` | `tagged`/`markdown`/`json` | `tagged` | não |
| `context.token_estimator` | `o200k`/`cl100k` | `o200k` | não |
| `context.overflow_line` | booleano | true | não |

## Formato `tagged`

```
<memories version="cv_…">
<channel slug="…">
<CATEGORIA>
#m_…: texto
</CATEGORIA>
</channel>
<titles>
#m_…: título
</titles>
<overflow count="N">use search_memories para o restante</overflow>
</memories>
```

`<titles>` e `<overflow>` só aparecem quando não vazios.
