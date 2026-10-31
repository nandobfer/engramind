# Embeddings

## Onde vive

- `src/providers/embeddings/`: `trait Embedder` (future em `Box`, para `Arc<dyn Embedder>`), `EmbedError`, `EmbeddingProvider`, os adaptadores `openai_compatible`, `ollama`, `gemini`, o `local` (fastembed + ONNX Runtime carregado por `ORT_DYLIB_PATH`) e o `HashEmbedder` dos testes.
- `src/core/embeddings/mod.rs`: `EmbeddingConfig` (configuração efetiva, `model_id`, prefixos, `missing`), `EmbeddingRuntime` (prefixo, prazo e conferência de quantidade, dimensão e valores), `EmbeddingGuard` (guarda de sentido do Redator), a troca de modelo (`embedding_switch`, `embedding_switched`, `finish_reindex`, `sync_embeddings`), `test_embedding`, `list_embedding_models`, `reembed_all` e o bloco `embedding` de `get_status`.
- `src/core/embeddings/job.rs`: job `embed` (`run_embed`); runner `Embed` em `src/core/jobs/runner.rs`.
- `src/store/sqlite/vectors.rs`: `memory_vectors`, troca/reindexação, contagens e o ramo vetorial. Migração `migrations/0007_vectors.sql`.
- Adaptadores: `src/rest/embeddings.rs`, `src/mcp/tools.rs`, `engramind settings test-embedding|embedding-models` e `engramind embeddings reembed`.

## Operações

- admin·write, tag `Embeddings`: `test_embedding` (`POST /v1/settings/embedding/test`), `list_embedding_models` (`POST /v1/settings/embedding/models`), `reembed_all` (`POST /v1/embeddings/reembed`, `202`). Guia: `configuracoes`.

## Invariantes

- `model_id = <provider>:<model>[:<dimensions>]`; no `local`, o nome canônico do fastembed (`local::find`). A dimensão esperada é a configurada ou a do modelo local; nos remotos sem `dimensions`, a da primeira resposta de cada chamada.
- O `model_id` vigente fica em `schema_meta['embedding.model_id']` (vazio com `off`), gravado na mesma transação das configurações; `save_vectors` só grava se o `model_id` do lote ainda for o vigente **e** a memória ainda tiver o `content_hash` e o título lidos. Um vetor por memória (PK `memory_id`, cascata no `delete`).
- Mudar `embedding.provider|model|base_url|dimensions|query_prefix|document_prefix` (`EMBEDDING_IDENTITY_KEYS`) valida o provedor antes da transação (remoto: uma chamada com `"amostra"`; local: só confere o modelo, sem download) e grava `EmbeddingSwitch` junto: outro `model_id` → lotes só do que não tem vetor dele; mesmo `model_id` → todos; `off` → `embedding_state = off`, sem jobs.
- Reindexação: `schema_meta['embedding.reindex']` = `ReindexState { model_id, started_at, total }`; lotes `embed` com `payload { memory_ids, reindex: started_at }`. Termina quando não há jobs pendentes nem falhos da marca (`reindex_jobs`): apaga vetores de outros modelos, o registro e os diretórios `models--*` de `MODELS_PATH` fora de uso. Lote falho de vez → `state = failed` em `get_status`; `reembed_all` recomeça. `409 REEMBED_IN_PROGRESS` só com jobs pendentes.
- Job `embed`: superado por um `embed` mais novo da mesma memória (escrita); `off` → `embedding_state = off`; sem configuração → `failed` na hora; erro temporário → backoff da fila até `MAX_ATTEMPTS = 5`; dimensão divergente → `failed`. Lease = `embedding.timeout_ms + 60 s`, concorrência 2.
- O título aplicado pelo Redator também reenfileira `embed` (o vetor é de `título + "\n" + texto`).
- O provedor local carrega o modelo uma vez por processo (`Mutex`, `TextEmbedding::embed` exige `&mut self`) em `spawn_blocking`; sem `ORT_DYLIB_PATH` válido, falha com `EmbedError::Runtime` antes do `ort` entrar em pânico.
- Testes: `TestContext` troca o provedor local por `HashEmbedder` (`Core::set_local_embedder`); provedores remotos contra o `ScriptedLlm`. Nunca rede nem ONNX nos testes.
- Logs sem texto, vetor, chave ou URL com credencial; `embeddings.*` como `event`.

Desenho: §6, §7, §11, §12 e §16. Decisões: `0018` a `0021`.
