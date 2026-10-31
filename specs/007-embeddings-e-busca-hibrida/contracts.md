# Contracts - embeddings e busca híbrida

**Criado em**: 2026-10-30
**Atualizado em**: 2026-10-31 (revisão do plano e implementação)
**Status**: Planejado (0.7.0)
**Feature**: [`spec.md`](spec.md)

## Migração `0007_vectors.sql`

```sql
-- Um vetor por memória: reindexar substitui o anterior. `updated_at` é o marco do progresso
-- de `reembed_all` e a cascata mantém `delete_memory` simples.
CREATE TABLE memory_vectors (
    memory_id  TEXT PRIMARY KEY REFERENCES memories (id) ON DELETE CASCADE,
    model_id   TEXT NOT NULL,
    dims       INTEGER NOT NULL,
    embedding  BLOB NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE INDEX memory_vectors_model ON memory_vectors (model_id, updated_at);
```

`dims` tem `CHECK (dims > 0)`. Estado em `schema_meta` (sem migração; a tabela chave/valor da 0001):

- `embedding.model_id`: o `model_id` vigente (vazio com `off`), gravado na mesma transação das configurações e conferido por `save_vectors`;
- `embedding.reindex`: a reindexação em andamento.

```json
{ "model_id": "local:multilingual-e5-small", "started_at": "2026-10-31T12:00:00.000Z", "total": 1234 }
```

`done` é contado, não gravado: `memory_vectors.model_id = :model_id AND updated_at >= :started_at`, limitado a `total`. A reindexação termina quando não há job `embed` pendente nem falho com `payload.reindex = started_at`; então os vetores de outros modelos e o registro são apagados.

## Rust

### `src/providers/embeddings/`

```rust
pub type EmbedFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<Vec<f32>>, EmbedError>> + Send + 'a>>;

/// Os textos já chegam com o prefixo do modelo.
pub trait Embedder: Send + Sync {
    fn embed<'a>(&'a self, model: &'a str, inputs: &'a [String]) -> EmbedFuture<'a>;
}

pub enum EmbedError {
    Timeout, RateLimited, Unavailable,
    Runtime(String),          // ONNX Runtime ausente ou com falha
    Rejected(String), InvalidResponse,
    UnknownModel(String),     // modelo local desconhecido
    Dimension { expected: usize, got: usize },
}
```

- Future em `Box` (como o `MeaningGuard`): uma função `-> impl Future` no trait impediria `Arc<dyn Embedder>`.
- `local.rs`: `LocalEmbedder { models_path, onnx_runtime, loaded: Arc<Mutex<Option<Loaded>>> }`; `TextEmbedding::embed` exige `&mut self`, então as chamadas são serializadas, em `spawn_blocking`, com `intra_threads = 2` e `cache_dir = MODELS_PATH`. Antes do `ort`, confere `ORT_DYLIB_PATH` (sem ele o `ort` entra em pânico) → `EmbedError::Runtime`. `supported()`, `find(name)` (nome canônico: repositório sem dono nem `-onnx` quando único, senão o enum em kebab-case; casa qualquer grafia com as mesmas letras e dígitos) e `remove_unused(models_path, keep)` (diretórios `models--*`).
- `openai_compatible.rs`: `POST {base_url}/embeddings`, corpo `{ model, input: [...], dimensions? }`, `Authorization: Bearer` quando houver chave; ordena por `data[].index`; `GET {base_url}/models`.
- `ollama.rs`: `POST {base_url}/api/embed`, corpo `{ model, input: [...] }`, resposta `{ embeddings: [[...]] }`; `GET {base_url}/api/tags`.
- `gemini.rs`: `POST {base_url}/v1beta/models/{model}:batchEmbedContents`, `x-goog-api-key`, `outputDimensionality` opcional, resposta `{ embeddings: [{ values: [...] }] }`.
- `hash.rs`: `HashEmbedder` dos testes (palavras em baldes, `concept(&[...])`, `dims(model, n)`, `fail(Some(erro))`, `calls()`).

### Configuração (`src/core/embeddings/mod.rs`)

```rust
pub struct EmbeddingConfig {
    pub provider: Option<EmbeddingProvider>, // off => None
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<Zeroizing<String>>,
    pub dimensions: Option<usize>,
    pub query_prefix: Option<String>,
    pub document_prefix: Option<String>,
    pub batch_size: usize,
    pub timeout: Duration,
}
// model_name(), model_id(), missing(), prefixes(), expected_dims(), lease()

pub struct EmbeddingRuntime { provider, model, model_id, query_prefix, document_prefix,
                              expected_dims, timeout, embedder: Arc<dyn Embedder> }
// embed(texts, EmbedKind::{Query, Document}, timeout): prefixo, prazo, quantidade, dimensão, finitos
```

`model_id` = `<provider>:<model>[:<dimensions>]`; dimensão só quando configurada; no `local`, o nome canônico. Prefixos vazios recebem o automático da família e5 (modelo com `e5-` no nome) e ficam vazios nos demais modelos. `Core::set_local_embedder(Arc<dyn Embedder>)` substitui só o provedor local (testes).

### `Store` (novos métodos)

```rust
fn update_settings(&self, changes, retained_categories, embedding: Option<EmbeddingSwitch>, context);
fn embedding_model(&self) -> Option<String>;               // schema_meta
fn set_embedding_model(&self, model_id: String);           // startup
fn embed_sources(&self, ids: Vec<String>) -> Vec<EmbedSource>;
fn save_vectors(&self, batch: VectorBatch) -> u64;         // CAS: modelo vigente + content_hash + título
fn mark_embedding(&self, ids: Vec<String>, state: EmbeddingState) -> u64; // só as `pending`
fn search_vectors(&self, query: VectorQuery) -> Vec<VectorHit>;           // memória + distância
fn vector_counts(&self, model_id: String, since: Option<String>) -> VectorCounts;
fn reindex_state(&self) -> Option<ReindexState>;
fn reindex_jobs(&self, started_at: String, exclude_job: Option<i64>) -> ReindexJobs;
fn start_reindex(&self, model_id: String, plan: ReindexPlan, context) -> ReindexState; // 409
fn finish_reindex(&self, model_id: String, started_at: Option<String>) -> bool;
```

`EmbeddingSwitch { model_id: Option<String>, reindex: Option<ReindexPlan> }` e `ReindexPlan { all, batch_size, started_at }`: com `model_id = None`, grava `''` em `embedding.model_id`, apaga o registro e marca `embedding_state = off`; com plano, marca `pending` e enfileira os lotes `{ memory_ids, reindex }` (`all = false`: só o que não tem vetor do `model_id`). Tudo na transação das configurações.

`search_vectors` usa a varredura exata e devolve a distância:

```sql
SELECT <colunas da memória>, vec_distance_cosine(v.embedding, :query) AS distance
FROM memory_vectors v JOIN memories m ON m.id = v.memory_id <joins>
WHERE v.model_id = :model_id AND v.dims = :dims
  AND m.channel_id IN (…) AND m.status IN (…)
  [AND m.category IN (…)] [AND m.id NOT IN (…)]
ORDER BY distance, m.id
LIMIT :limit;
```

O BLOB é `Vec<f32>` little-endian, sem cabeçalho. A escrita grava `embedding_state` pela presença do job `embed` (`pending`, ou `off` com os embeddings desligados); `Redaction.embed` faz o mesmo na aplicação do Redator, que reenfileira `embed` também quando só o título muda.

### `src/core/search.rs`

1. Resolver acesso com `Purpose::Search` (inalterado).
2. Tentar o ramo vetorial: `EmbeddingConfig` vigente + `embed([query_prefix + query], Query)`; se `off`, sem configuração, erro/timeout ou dimensão divergente → `mode = text`, `WARN embeddings.query_failed`.
3. `search_text` (inalterado) e `search_vectors` com o mesmo `limit`, filtros, `statuses` e `exclude_ids`.
4. Fusão: `score(id) = vector_weight/(rrf_k + pos_v) + text_weight/(rrf_k + pos_t)`, ausente = 0; ordenar por `score` desc, `id` asc; renumerar `rank`; arredondar a 6 casas.
5. `mode = "hybrid"` quando o ramo vetorial rodou; `"text"` caso contrário.
6. Contadores e `usage_log` como hoje (inalterado).

`SearchResults` ganha `mode: "hybrid" | "text"`; `SearchHit.score` passa a documentar a fórmula ponderada.

### `src/core/hints.rs`

```rust
pub struct HintsBody { pub text: String, pub channels: Option<Vec<String>>, pub limit: Option<u32> }
pub struct Hint { pub id: String, pub title: String, pub channel: String }
pub struct HintsResult { pub hints: Vec<Hint> }
```

Fluxo: `authorize("get_hints")`; texto entre 1 e 500 caracteres; `limit` padrão `search.hint_limit` (máximo 20); `Selection::parse` + `Purpose::Search`; descartar `Reach.mode == Manage`; excluir o contexto (mesmo helper de `exclude_context`); vetorizar a consulta; `search_vectors` com `limit = max(hint_limit × 4, 10)`; converter distância em similaridade (`1 - d`) e filtrar `>= search.hint_threshold`; devolver até `hint_limit` com `{ id, title, channel }`. Sem embeddings → `{ hints: [] }`. Contadores: `retrieval_count` e `usage_log` (`op = get_hints`).

### Guarda de sentido

`EmbeddingGuard` (em `src/core/embeddings/mod.rs`) recebe o `EmbeddingRuntime`; `similarity` embute original e revisado com `EmbedKind::Document` numa única chamada e devolve o **cosseno** (similaridade). Qualquer erro → `None`. `Core::meaning_guard()` (assíncrono) devolve o override de teste (`set_meaning_guard`), senão o `EmbeddingGuard` da configuração vigente, senão `None` (só o título).

### Operações (`src/core/embeddings/mod.rs`)

- `test_embedding(body)`: aplica overrides `embedding.*` (inclusive `embedding.api_key` em claro) sobre o snapshot, monta o provedor, embute `[body.text]` como `Document`, mede a latência e devolve `TestEmbeddingResult`. Não grava configuração; com `Idempotency-Key`, grava só a resposta.
- `list_embedding_models(body)`: `local` → `local::supported()`; `ollama` → `GET {base_url}/api/tags`; `openai_compatible` → `GET {base_url}/models`; `gemini`/`off` → `PROVIDER_LISTING_UNSUPPORTED`. Sem `base_url`/`api_key`, os configurados.
- `reembed_all`: exige provedor configurado; `Store::start_reindex` grava o registro, marca todas as memórias como `pending` e enfileira os lotes na mesma transação (`409 REEMBED_IN_PROGRESS` com jobs pendentes de outra reindexação); `jobs_enqueued()`; sem memórias, encerra na hora.
- `embedding_status`: bloco `embedding` de `get_status`.
- `embedding_switch(atual, proposto)`: quando muda uma de `embedding.provider|model|base_url|dimensions|query_prefix|document_prefix`, valida o provedor **antes** da transação (remoto: uma chamada com `"amostra"`; local: só confere o modelo) e devolve o `EmbeddingSwitch` (outro `model_id` → só o que falta; mesmo `model_id` → tudo; `off` → desliga). `batch_size`/`timeout_ms` não reindexam.
- `sync_embeddings()` no startup: grava o `model_id` vigente (sem reindexar) e encerra a reindexação que terminou com o serviço parado.
- `finish_reindex(model_id, started_at)`: apaga vetores de outros modelos e o registro; remove os modelos locais fora de uso de `MODELS_PATH` (`WARN` se falhar).

### Job `embed` (`src/core/embeddings/job.rs`)

`Embed` em `src/core/jobs/runner.rs` (`KIND = "embed"`, lease `embedding.timeout_ms + 60 s`, `EMBED_CONCURRENCY = 2`), `Core::run_embed_jobs(now)` para testes. Payload `memory_id` (escrita; superado por um `embed` mais novo da mesma memória) ou `memory_ids` + `reindex` (lote). Desligado → `embedding_state = off`, `Done("off")`; sem configuração → `failed`, `Fail`; erro temporário → `Retry` com o backoff da fila até `MAX_ATTEMPTS = 5`; dimensão divergente ou modelo desconhecido → `failed`. Grava com `save_vectors`; nada gravado num job de uma memória → `Superseded`. No fim de um lote marcado, se não sobrou job pendente nem falho da marca, `finish_reindex`.

## Operações

| Operação | Tipo·escopo | REST | MCP | CLI |
|---|---|---|---|---|
| `get_hints` | any·read | `POST /v1/hints` | `get_hints` | `engramind hints <texto>` |
| `test_embedding` | admin·write | `POST /v1/settings/embedding/test` | `test_embedding` | `engramind settings test-embedding --text …` |
| `list_embedding_models` | admin·write | `POST /v1/settings/embedding/models` | `list_embedding_models` | `engramind settings embedding-models --provider …` |
| `reembed_all` | admin·write | `POST /v1/embeddings/reembed` | `reembed_all` | `engramind embeddings reembed` |

Regras de catálogo:

- `get_hints` é leitura por POST: `read_only = true`, sem `Idempotency-Key`; a exceção de `src/core/operations.rs` passa a aceitar `search_memories` **e** `get_hints`.
- `test_embedding` e `list_embedding_models` são `mutation(...)` com `destructive = false` e `idempotent = false` (como `test_redator`); `reembed_all` é `idempotent = true`.
- Tags novas: `TAG_SEARCH = "Search"` (any) e `TAG_EMBEDDINGS = "Embeddings"` (admin). `ADMIN_TAGS` ganha `Embeddings`; `configuracoes.md` passa a declarar `["Settings", "Embeddings"]`; `busca-e-dicas.md` declara `["Search"]`. `search_memories` sai de `Memories` para `Search`.
- `TAG_SEARCH` não entra em `ADMIN_TAGS`.

## Corpos e respostas

### `POST /v1/hints`

```json
{ "text": "rodar os testes", "channels": ["eng/global"], "limit": 3 }
```

```json
{ "hints": [ { "id": "m_8f2a1c3d", "title": "Suíte completa com nextest", "channel": "eng/global" } ] }
```

### `POST /v1/settings/embedding/test`

```json
{ "text": "amostra", "settings": { "embedding.provider": "openai_compatible", "embedding.base_url": "https://api.openai.com/v1", "embedding.model": "text-embedding-3-small" } }
```

```json
{ "provider": "openai_compatible", "model": "text-embedding-3-small", "model_id": "openai_compatible:text-embedding-3-small", "dimensions": 1536, "latency_ms": 218 }
```

`settings` aceita `embedding.*`, inclusive `embedding.api_key` em claro (nunca gravada nem devolvida). Corpo com `Debug` redigido, como `TestRedatorBody`.

### `POST /v1/settings/embedding/models`

```json
{ "provider": "ollama", "base_url": "http://127.0.0.1:11434" }
```

```json
{ "models": [ { "id": "bge-m3", "dimensions": 1024 }, { "id": "nomic-embed-text" } ] }
```

`dimensions` é `null` quando o provedor não informa. `api_key` opcional no corpo (MCP/REST), nunca obrigatória.

### `POST /v1/embeddings/reembed`

Sem corpo. Resposta `202`:

```json
{ "model_id": "local:multilingual-e5-small", "started_at": "2026-10-31T12:00:00.000Z", "total": 1234 }
```

`409 REEMBED_IN_PROGRESS` quando outra reindexação ainda tem jobs na fila; `400 PROVIDER_NOT_CONFIGURED` (com `details.missing`) quando `off` ou incompleto.

### `search_memories`

Resposta ganha `mode`; o resto não muda:

```json
{ "mode": "hybrid", "results": [ { "rank": 1, "score": 0.032787, "memory": { } } ], "context_version": "cv_3f9e1a7c0b2d" }
```

### `get_status`

```json
{
  "embedding": {
    "enabled": true,
    "configured": true,
    "missing": [],
    "provider": "local",
    "model": "multilingual-e5-small",
    "model_id": "local:multilingual-e5-small",
    "state": "reindexing",
    "dimensions": 384,
    "total": 1234,
    "done": 300,
    "reindex_started_at": "2026-10-31T12:00:00.000Z"
  }
}
```

`state`: `off`, `ready`, `reindexing` ou `failed` (lote da reindexação esgotou as tentativas). Sem reindexação, `total` = memórias e `done` = vetores do modelo vigente; durante, `total` da reindexação e `done` desde `started_at`.

## Configurações desta etapa

| Chave | Tipo | Padrão | Por canal |
|---|---|---|---|
| `embedding.provider` | `local`/`openai_compatible`/`ollama`/`gemini`/`off` | `local` | não |
| `embedding.model` | texto (200) | `multilingual-e5-small` | não |
| `embedding.base_url` | URL | `null` | não |
| `embedding.api_key` 🔒 | segredo | `null` | não |
| `embedding.dimensions` | inteiro 1–8192 | `null` | não |
| `embedding.query_prefix` | texto (200) | `null` (automático) | não |
| `embedding.document_prefix` | texto (200) | `null` (automático) | não |
| `embedding.batch_size` | inteiro 1–256 | `32` | não |
| `embedding.timeout_ms` | inteiro 100–120000 | `15000` | não (a consulta da busca espera no máximo 2 s) |
| `search.vector_weight` | número 0–10 | `1.0` | não |
| `search.text_weight` | número 0–10 | `1.0` | não |
| `search.rrf_k` | inteiro 1–1000 | `60` | não |
| `search.default_limit` | inteiro 1–50 | `10` | não |
| `search.hint_threshold` | número 0–1 | `0.80` | não |
| `search.hint_limit` | inteiro 1–20 | `3` | não |

## Códigos de erro novos

| Código | Status | Quando |
|---|---|---|
| `PROVIDER_LISTING_UNSUPPORTED` | 400 | `list_embedding_models` em `gemini`/`off`. |
| `EMBEDDING_DIMENSION_MISMATCH` | 502 | Dimensão devolvida diverge da configurada/da primeira. |
| `REEMBED_IN_PROGRESS` | 409 | `reembed_all` durante outra reindexação. |

Reutilizados: `PROVIDER_NOT_CONFIGURED` (400, com `details.missing`), `PROVIDER_FAILED` (502), `PROVIDER_TIMEOUT` (504), `SETTING_IS_SECRET` (400), `SETTING_INVALID`/`SETTING_UNKNOWN` (400), `RATE_LIMITED` (429).

## Contrato de arquivos e imagem

- `Cargo.toml`: `fastembed = { version = "7.5", default-features = false, features = ["ort-load-dynamic", "hf-hub-rustls-tls"] }`.
- `Dockerfile`: build args `ONNX_RUNTIME_VERSION=1.31.1` e `ONNX_RUNTIME_SHA256`; estágio `onnxruntime` baixa o `onnxruntime-linux-x64` oficial e confere o hash; o runtime copia só `libonnxruntime.so.1.31.1` e define `ORT_DYLIB_PATH`; `MODELS_PATH=/data/models` já existe.
- `.env.example`: comentário de `MODELS_PATH` e `ORT_DYLIB_PATH` (infra, não comportamento; `ORT_DYLIB_PATH` é validado no startup quando definido).
- `doctor`: item `onnx_runtime` (pulado sem `ORT_DYLIB_PATH`).
- `docs/decisions/0018-*.md` a `0021-*.md`: varredura exata e schema dos vetores; provedores e runtime local; reindexação e troca de modelo; busca híbrida, dicas e guarda.
