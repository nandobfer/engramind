# Contracts - contexto

**Criado em**: 2026-10-30
**Atualizado em**: 2026-10-30 (implementação)
**Status**: Implementado
**Feature**: [`spec.md`](spec.md)

## Migração `0005_usage.sql`

```sql
usage_log (id INTEGER PK, at TEXT, app_id TEXT, agent_id TEXT NULL, token_id TEXT, op TEXT,
           channel_ids TEXT /* JSON */, memory_ids TEXT /* JSON */, rendered_tokens INTEGER NULL)
CREATE INDEX usage_log_at ON usage_log(at);
CREATE INDEX usage_log_agent_at ON usage_log(agent_id, at);
CREATE INDEX usage_log_op_at ON usage_log(op, at);
```

## Core

```rust
pub enum ContextFormat { Tagged, Markdown, Json }
pub struct GetContextQuery { pub channels: Option<String>, pub format: Option<ContextFormat> }
pub enum ContextOutcome { NotModified(ContextNotModified), Rendered(Arc<Rendered>) }
pub struct Rendered { pub view: ContextView, pub full_ids: Vec<String>, pub channel_ids: Vec<String> }
pub struct ContextView { pub content: ContextContent, pub version: String, pub counts: ContextCounts, pub rendered_tokens: u32, pub format: ContextFormat }
pub enum ContextContent { Text(String), Document(ContextDocument) } // untagged
pub trait TokenEstimator: Send + Sync { fn count(&self, text: &str) -> u32; }

impl Core {
    pub async fn get_context(&self, principal, query: GetContextQuery, if_none_match: Option<&str>, header: Option<&str>) -> Result<ContextOutcome, ApiError>;
    pub async fn channel_titles(&self, principal, channel: &str, header: Option<&str>) -> Result<String, ApiError>;
}

// Store
fn context_memories(channel_ids) -> ContextSnapshot { versions, memories }; // mesma transação de leitura
fn bump_injections(Vec<InjectionBump>);
fn append_usage(Vec<UsageEntry>);
```

## Operação

| Operação | Tipo·escopo | REST | CLI |
|---|---|---|---|
| `get_context` | any·read (tag `Context`) | `GET /v1/context?channels=&format=` (+ `If-None-Match`; `200` com `ETag`, `304`) | `engramind context [--format] [--if-none-match]` |

`search_memories` ganha `exclude_context?: bool` no corpo e `context_version` na resposta.

## MCP

- Ferramenta `get_context { channels?, format?, if_none_match? }`; sem mudança, `{ not_modified: true, version }`.
- Resource `engramind://context` (contexto do token com o `Engramind-Channels` da conexão, no `context.default_format`).
- Resource template `engramind://channels/{slug}` (`#id: título` do canal alcançado com `read`; `text/plain`).
- Resource template `engramind://guides/{id}` (guia em `text/markdown`).
- Prompt `memory-writing` (`docs/prompts/memory-writing.md`).
- Resources e prompt também são filtrados pelo token; erros `-32002` com o envelope em `data`.

## Erros novos

| Código | Status | Saída | Quando |
|---|---|---|---|
| `RESOURCE_NOT_FOUND` | 404 | 5 | URI de resource desconhecida (só MCP) |
| `PROMPT_NOT_FOUND` | 404 | 5 | Prompt inexistente (só MCP) |

## Configurações desta etapa

| Chave | Tipo | Padrão | Intervalo | Por canal |
|---|---|---|---|---|
| `context.budget_tokens` | integer | 4000 | 0 a 200000 | sim |
| `context.titles_budget_tokens` | integer | 800 | 0 a 50000 | sim |
| `context.default_format` | enum | `tagged` | `tagged`, `markdown`, `json` | não |
| `context.token_estimator` | enum | `o200k` | `o200k`, `cl100k` | não |
| `context.overflow_line` | boolean | true | — | não |

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

`<titles>` e `<overflow>` só aparecem quando não vazios; sem memórias, só `<memories version="…">` e `</memories>`.
