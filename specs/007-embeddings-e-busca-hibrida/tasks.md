# Tasks - embeddings e busca híbrida

**Input**: `spec.md`, `clarify.md`, `contracts.md`, `plan.md`

## Planejamento

- [x] T001 - Resolver Q-001 a Q-016 (recomendações em `clarify.md`); confirmar com a usuária as mudanças de desenho D-006 (similar textual), D-008 (busca só no modelo vigente) e §16 (ONNX Runtime).

## Portão técnico

- [x] T002 - Spike `fastembed` 7.5 + `ort-load-dynamic` + ONNX Runtime fixo: compilação limpa, Clippy, tamanho do binário/imagem, carga do `multilingual-e5-small`, RAM, latência, `list_supported_models()` e layout de `MODELS_PATH`. Decidir feature `local-embeddings` e registrar decisão 0019. Resultado: sem feature; ONNX Runtime 1.31.1; e5-small fp32 (Q-017).

## Dados

- [x] T003 - Migração `0007_vectors.sql` (`memory_vectors`, índice por modelo, FK em cascata, `dims`, `updated_at`); teste de migração e de cascata.
- [x] T004 - `Store`: `search_vectors`, `upsert_vector` (compare-and-set), `vector_counts`, `set_embedding_state_all`, `enqueue_embed_batches`; BLOB `f32` LE; teste com `vec_distance_cosine`.

## Provedores

- [x] T010 - `trait Embedder`, `EmbedKind`, `EmbedError`, `EmbeddingConfig` (`model_id`, `missing`, prefixos, `model_id` com dimensão), `HashEmbedder`, `Core::set_embedder`.
- [x] T011 - Provedor `local` (cache por processo, `spawn_blocking`, `intra_threads`, `MODELS_PATH`, invalidação na troca).
- [x] T012 - Provedores `openai_compatible`, `ollama`, `gemini` (timeout, cabeçalhos, lote, ordem por índice, dimensão).

## Fila e reindexação

- [x] T013 - Job `Embed` (payload `memory_id`/`memory_ids`, lote, lease, 5 tentativas, `superseded`, limpeza final de vetores e do modelo local), `run_embed_jobs`, `spawn_workers`.
- [x] T014 - `reembed_all` com `meta.embedding.reindex`, `409 REEMBED_IN_PROGRESS`, enfileiramento em lotes e resposta `202`.
- [x] T015 - `update_settings` atômico para `embedding.*` que reindexam: validação prévia, estados, jobs e `meta` na mesma transação; `off`/`on` com a regra de não reembedar o que já tem vetor; `batch_size`/`timeout_ms` sem reindexar.

## Busca e dicas

- [x] T016 - Ramo vetorial + RRF ponderado em `search_memories`; `mode`; `search.default_limit`, `rrf_k`, pesos valendo; falha do provedor → `text` com `WARN`.
- [x] T017 - `get_hints` (`Purpose::Search`, sem `manage`, exclusão do contexto, limiar sobre cosseno, `hint_limit`, contadores e `usage_log`).
- [x] T018 - Guarda de sentido (`EmbeddingGuard`, precedência de `set_meaning_guard`, `Document` dos dois lados, falha fechada).
- [x] T019 - `test_embedding` e `list_embedding_models` (local/Ollama/OpenAI-compatível; `PROVIDER_LISTING_UNSUPPORTED`).
- [x] T020 - `get_status.embedding` (`off`/`ready`/`reindexing`, `total`, `done`, dimensão, `model_id`).

## Três vias e contrato

- [x] T021 - `OPERATIONS` (tags `Search`/`Embeddings`, exceção de leitura por POST), `app.rs`, `rest/embeddings.rs`, `rest/memories.rs` (`mode`), `rest/settings.rs`; `#[utoipa::path]` com todos os status.
- [x] T022 - MCP: quatro ferramentas novas com argumentos planos e `idempotency_key` quando couber; `read_only_hint` correto.
- [x] T023 - CLI: `hints`, `settings test-embedding`, `settings embedding-models`, `embeddings reembed` (`remote.rs`, `help.rs`, `--json`).
- [x] T024 - `docs/openapi.json` regenerado (`UPDATE_SNAPSHOTS=1`); `ADMIN_TAGS` e `catalog.rs` (guia `busca-e-dicas`, `configuracoes` com `Embeddings`).

## Testes

- [x] T030 - Unitários: `model_id`/prefixos/dimensão, RRF, cosseno/limiar, guarda, configuração que reindexa.
- [x] T031 - Integração com `HashEmbedder`: busca híbrida, pesos, `mode`, dicas, `similar` inalterado, guarda no Redator.
- [x] T032 - Provedores remotos roteirizados: rotas, `429`, `5xx`, timeout, dimensão errada, listagem, `test_embedding` sem gravar chave.
- [x] T033 - Reindexação: mudança de dimensão sem mistura, progresso, reinício, limpeza de modelo, `delete_memory`, `off`/`on`, backlog da 0.4.0 drenado.
- [x] T034 - Matriz e contrato: quatro operações nas três vias, `get_hints` como leitura por POST, guia único, erros e configurações documentados.

## Operação

- [x] T040 - `Dockerfile` com ONNX Runtime fixo e `ORT_DYLIB_PATH`; `.env.example`; build da imagem e teste manual com `provider = local` (download em `/data/models`).
- [x] T041 - Guias (`busca-e-dicas`, `configuracoes`, `erros`, `operacao`, `escrever-memorias`, `contexto-e-cache`, `integracao-consumidor`, `integracao-mcp`, `overview`, `cli`) e skill.
- [x] T042 - `aicontext/search.md`, `aicontext/embeddings.md`, `aicontext/README.md`; `docs/design.md` §6/§7/§9/§11/§16; decisões 0018–0021.
- [x] T043 - Versão `0.7.0`, `Cargo.lock`, `CHANGELOG.md`.
- [x] T044 - `implement.md` e `report.md` ao concluir; verificação final (`fmt`, `clippy`, testes afetados, snapshot).
