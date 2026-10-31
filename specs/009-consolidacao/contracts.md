# Contracts - consolidação

**Criado em**: 2026-10-30
**Atualizado em**: 2026-10-31
**Status**: Implementado (0.9.0)
**Feature**: [`spec.md`](spec.md)

## Migração `0009_consolidation.sql`

```sql
ALTER TABLE memories ADD COLUMN importance_pending REAL;   -- D-001
ALTER TABLE memories ADD COLUMN last_injected_at TEXT;     -- Q-003

-- Pares que o Redator recusou fundir (Q-014); valem enquanto os dois textos forem os mesmos.
CREATE TABLE consolidation_pairs (
    memory_a   TEXT NOT NULL REFERENCES memories (id) ON DELETE CASCADE,
    memory_b   TEXT NOT NULL REFERENCES memories (id) ON DELETE CASCADE,
    hash_a     BLOB NOT NULL,
    hash_b     BLOB NOT NULL,
    decided_at TEXT NOT NULL,
    PRIMARY KEY (memory_a, memory_b),
    CHECK (memory_a < memory_b)
) STRICT;
CREATE INDEX consolidation_pairs_b ON consolidation_pairs (memory_b);
```

## Rust

### Planejador (`src/core/consolidation/plan.rs`)

```rust
pub enum Task { Duplicates, Importance, Stale }          // ordem de execução

pub struct ChannelPlan {
    pub channel: ChannelRef,                               // { id, slug }
    pub skipped: BTreeMap<Task, SkipReason>,               // redator_not_configured | embeddings_off
    pub duplicates: Vec<PairCandidate>,                    // { keep, archive, similarity, hashes }
    pub classify: Vec<String>,                             // ids, já em lotes de 20, até 200
    pub stale: StalePlan,                                  // { flag, clear, archive }
}

/// Pura: o `dry_run` devolve e a execução consome (D-004).
pub fn plan(input: PlanInput<'_>, config: &ConsolidationConfig, now: DateTime<Utc>) -> ChannelPlan;
pub fn cosine_pairs(vectors: &[(String, Vec<f32>)], threshold: f32) -> Vec<(usize, usize, f32)>;
```

- `PlanInput` vem de `Store::consolidation_input(channel_id, model_id)`: memórias `active`/`permanent` do canal (id, `created_at`, `updated_at`, `content_hash`, `importance`, `importance_pending`, `last_*`, `verified_at`, `stale_candidate`), vetores do modelo vigente das `active` e os pares recusados.
- `ConsolidationConfig::new(&SettingsSnapshot, channel_settings)` — `enabled` efetivo, `tasks`, `duplicate_threshold`, `stale_after_days`, `auto_archive_stale`.

### Job `consolidate` (`src/core/consolidation/job.rs`)

```rust
pub const JOB_KIND: &str = "consolidate";
pub const CONSOLIDATE_CONCURRENCY: usize = 1;
pub const LEASE: Duration = Duration::from_secs(30 * 60);
pub const MAX_PAIRS: usize = 20;
pub const IMPORTANCE_BATCH: usize = 20;
pub const MAX_IMPORTANCE_CALLS: usize = 10;
pub const CONSOLIDATOR_AGENT: &str = "consolidador";      // autor system/consolidador
```

- `Core::run_consolidate(job)`: recarrega a configuração (canal desligado ou arquivado entre o enfileiramento e a execução → `Done("skipped")`), monta o plano e executa as tarefas; devolve `Done("completed")`.
- `Core::run_consolidate_jobs(now)` para os testes, como `run_redact_jobs`.

### Ações no `Store`

```rust
async fn merge_memories(&self, merge: MemoryMerge, context: WriteContext) -> Result<MergeOutcome, StoreError>;
//   MemoryMerge { keep: MemoryTarget, archive: MemoryTarget, keep_hash, archive_hash, text, ... }
//   CAS nos dois hashes e nos dois status `active`; senão MergeOutcome::Superseded.
async fn decline_pair(&self, pair: DeclinedPair) -> Result<(), StoreError>;
async fn set_importance_pending(&self, channel_id: String, scores: Vec<(String, Vec<u8>, f64)>) -> Result<u64, StoreError>;
//   CAS no hash; sem mudar version nem channels.version.
async fn apply_stale(&self, channel_id: String, plan: StaleWrite, context: WriteContext) -> Result<StaleOutcome, StoreError>;
//   flag/clear sem version; archive com revisão, auditoria, channels.version e evento.
async fn mark_verified(&self, target: MemoryTarget, at: String, context: WriteContext) -> Result<Memory, StoreError>;
```

- `bump_channel` (e as atualizações de `channels.version` em `channels.rs`) executam também `UPDATE memories SET importance = importance_pending, importance_pending = NULL WHERE channel_id = ?1 AND importance_pending IS NOT NULL` (Q-008).
- Edição de texto, `revert` e fusão zeram `importance` e `importance_pending` e limpam `stale_candidate`.
- `bump_injections` passa a avançar `last_injected_at` (como `last_retrieved_at`).

### Agendador (`src/core/consolidation/schedule.rs`)

```rust
pub fn next_after(expression: &str, after: DateTime<Utc>) -> Option<DateTime<Utc>>;  // croner
impl<S: Store> Core<S> { pub async fn consolidation_tick(&self, now: DateTime<Utc>) -> Result<usize, ApiError>; }
```

`background.rs` chama `consolidation_tick` a cada 60 s; o estado fica em `schema_meta['consolidation.last_scheduled']`.

### Prompts

`src/core/consolidation/merge.md` e `importance.md`, com as marcas aleatórias de `redator::prompt::boundary` e as `instructions` do canal no fim; esquemas JSON:

```json
{ "merge": true, "text": "…" }
{ "scores": [{ "id": "m_8f2a1c3d", "importance": 0.8 }] }
```

## Operações

| Operação | Tipo·escopo | REST | MCP | CLI |
|---|---|---|---|---|
| `run_consolidation` | admin·write | `POST /v1/consolidation/run` | `run_consolidation` | `engramind consolidation run [--channel …] [--tasks duplicates,importance,stale] [--dry-run]` |
| `mark_verified` | any·write | `POST /v1/memories/{memory}/verify` | `mark_verified` | `engramind memories verify <memory>` |
| `list_memories` (estendida) | any·read | `GET /v1/memories?stale=true` | `stale` | `engramind memories list --stale` |

- `run_consolidation`: tag `Consolidation`, `destructive = false`, `idempotent = false` (aceita `Idempotency-Key`); o argumento da CLI é `--channel` (nunca `--channels`, global).
- `mark_verified`: tag `Memories`, `destructive = false`, `idempotent = true`.

## Corpos e respostas

### `POST /v1/consolidation/run`

```json
{ "channel": "eng/global", "tasks": ["duplicates", "stale"], "dry_run": true }
```

`200` (`dry_run`):

```json
{ "dry_run": true,
  "channels": [{
    "channel": "eng/global",
    "skipped": { "importance": "redator_not_configured" },
    "duplicates": [{ "keep": "m_1b7d0e44", "archive": "m_8f2a1c3d", "similarity": 0.95 }],
    "classify": [],
    "stale": { "flag": ["m_44c2aa10"], "clear": [], "archive": [] } }],
  "jobs": [] }
```

`202` (execução):

```json
{ "dry_run": false, "channels": [],
  "jobs": [{ "channel": "eng/global", "job_id": 481, "already_queued": false }] }
```

Erros: `400 VALIDATION_ERROR` (`tasks`), `404 CHANNEL_NOT_FOUND`, `409 CHANNEL_ARCHIVED`, `409 CONSOLIDATION_DISABLED`, `409 IDEMPOTENCY_KEY_CONFLICT`.

### `POST /v1/memories/{memory}/verify`

Sem corpo. `200` com a `Memory` (`verified_at` preenchido, `stale_candidate: false`). Erros: `404 MEMORY_NOT_FOUND`, `403 SCOPE_REQUIRED`/`CHANNEL_NOT_SUBSCRIBED`.

### `Memory` (acréscimos)

```json
{ "…": "…", "stale_candidate": false, "merged_into": null }
```

### `get_status` (acréscimo)

```json
{ "consolidation": {
    "enabled_channels": 3, "schedule": "0 3 * * *",
    "next_run_at": "2026-11-01T03:00:00Z", "last_scheduled_at": "2026-10-31T03:00:00Z",
    "queued": 0, "running": 0,
    "last_24h": { "merged": 2, "declined": 5, "classified": 40, "flagged": 3, "cleared": 1,
                  "archived": 0, "superseded": 0, "failed": 0 },
    "llm_last_24h": { "calls": 9, "failures": 0, "cost_usd": 0.004 } } }
```

### Auditoria

| Ação | Entidade | Conteúdo (sem textos nem títulos) |
|---|---|---|
| `memory.merge` | a mantida | `{ merged: <id arquivada>, version }` |
| `memory.archive` | a fundida ou a em desuso | `{ reason: "merged" \| "stale", merged_into? }` |
| `memory.verify` | a memória | `{ verified_at }` |
| `consolidation.run` | o canal | contagens por tarefa, tarefas puladas e o `trigger` |
| `consolidation.request` | — (`entity = consolidation`) | tarefas e jobs do pedido manual |

## Configurações desta etapa

| Chave | Tipo | Padrão | Por canal | Validação |
|---|---|---|---|---|
| `consolidation.enabled` | booleano | false | sim | — |
| `consolidation.schedule` | cron (novo) | `0 3 * * *` | não | 5 campos, UTC, aceito pelo `croner` |
| `consolidation.tasks` | lista de opções (novo) | `["duplicates","importance","stale"]` | não | subconjunto sem repetição, 1 a 3 itens |
| `consolidation.duplicate_threshold` | número | 0.92 | não | 0 a 1 |
| `consolidation.stale_after_days` | inteiro | 90 | não | 1 a 3650 |
| `consolidation.auto_archive_stale` | booleano | false | não | — |

Nenhuma entra em `CONTEXT_RENDER_KEYS` (não mudam o bloco).

## Códigos de erro novos

| Código | Status | Saída da CLI |
|---|---|---|
| `CONSOLIDATION_DISABLED` | 409 | conflito |

## Métricas

| Nome | Tipo | Rótulos |
|---|---|---|
| `engramind_consolidation_actions_total` | counter | `task` (`duplicates`/`importance`/`stale`), `action` (`merged`/`declined`/`classified`/`flagged`/`cleared`/`archived`/`superseded`/`failed`) |

`llm_calls` e `engramind_llm_*` com `purpose = consolidation`.

## Exceções do teste de cobertura de eventos

Acrescentar `run_consolidation` (os eventos saem do job) e `mark_verified` (Q-009) a `WITHOUT_EVENTS`.
