# Contracts - embeddings e busca híbrida

**Criado em**: 2026-10-30
**Status**: Draft
**Feature**: [`spec.md`](spec.md)

## Migração `0007_vectors.sql`

```sql
memory_vectors (memory_id TEXT PK, model_id TEXT, dims INTEGER, embedding BLOB)  -- float32 do sqlite-vec
CREATE INDEX memory_vectors_model ON memory_vectors(model_id);
```

## Busca vetorial (SQL de referência)

```sql
SELECT m.id, vec_distance_cosine(v.embedding, :query) AS distance
FROM memory_vectors v JOIN memories m ON m.id = v.memory_id
WHERE v.model_id = :model AND m.channel_id IN (…) AND m.status IN (…) [AND m.category IN (…)]
ORDER BY distance LIMIT :k;
```

## Operações

| Operação | Tipo·escopo | REST | CLI |
|---|---|---|---|
| `get_hints` | any·read | `POST /v1/hints` | `engramind hints <texto>` |
| `test_embedding` | admin·write | `POST /v1/settings/embedding/test` | `engramind settings test-embedding --text …` |
| `list_embedding_models` | admin·write | `POST /v1/settings/embedding/models` | `engramind settings embedding-models --provider …` |
| `reembed_all` | admin·write | `POST /v1/embeddings/reembed` | `engramind embeddings reembed` |

`search_memories` passa a devolver `mode: "hybrid" | "text"` e `score` por item.

## Configurações desta etapa

| Chave | Tipo | Padrão | Por canal |
|---|---|---|---|
| `embedding.provider` | `local`/`openai_compatible`/`ollama`/`gemini`/`off` | `local` | não |
| `embedding.model` | texto | `multilingual-e5-small` | não |
| `embedding.base_url` | URL | — | não |
| `embedding.api_key` 🔒 | segredo | — | não |
| `embedding.dimensions` | inteiro | — | não |
| `embedding.query_prefix` | texto | automático | não |
| `embedding.document_prefix` | texto | automático | não |
| `embedding.batch_size` | inteiro | 32 | não |
| `embedding.timeout_ms` | inteiro | 15000 | não |
| `search.vector_weight` | número | 1.0 | não |
| `search.text_weight` | número | 1.0 | não |
| `search.rrf_k` | inteiro | 60 | não |
| `search.default_limit` | inteiro | 10 | não |
| `search.hint_threshold` | número 0–1 | 0.80 | não |
| `search.hint_limit` | inteiro | 3 | não |

## Códigos de erro novos

| Código | Status |
|---|---|
| `PROVIDER_LISTING_UNSUPPORTED` | 400 |
| `EMBEDDING_DIMENSION_MISMATCH` | 502 |
| `REEMBED_IN_PROGRESS` | 409 |
