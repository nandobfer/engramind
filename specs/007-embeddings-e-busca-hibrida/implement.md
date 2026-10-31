# Implement - embeddings e busca híbrida

**Data**: 2026-10-31

## Ordem real

1. Revisão do plano contra o código da 0.6.0 e o código do `fastembed` 7.5.0: 15 correções (tabela `schema_meta`, trait com future em `Box`, distância no ramo vetorial, troca numa só transação, fim da reindexação por jobs, `embedding_state = failed`, `off` na escrita, guarda devolvendo o cosseno, modelo fp32, `Mutex` no local, prazo da consulta, testes sem ONNX, itens do spike, supersede do `embed`, `limit` das dicas) e Q-017 a Q-021, aprovadas pela usuária com as recomendações (`clarify.md`).
2. Spike T002 (decisão 0019): `fastembed` 7.5 com `ort-load-dynamic` + `hf-hub-rustls-tls` compila com o `cc` desta máquina; ONNX Runtime ≥ 1.24 exigido, 1.31.1 escolhido; e5-small fp32 465 MB, carga ~5 s, ~1 GB RSS, 32 textos ~280 ms, consulta ~20 ms; pânico do `ort` sem a biblioteca; cossenos do e5 comprimidos. Sem feature `local-embeddings`.
3. Migração `0007_vectors.sql`; tipos do `core::model` (`EmbedSource`, `NewVector`, `VectorBatch`, `VectorQuery`, `VectorHit`, `ReindexPlan`, `EmbeddingSwitch`, `ReindexState`, `ReindexJobs`, `VectorCounts`, `Redaction.embed`) e os métodos do `Store` (`src/store/sqlite/vectors.rs`); `update_settings` do `Store` recebe o `EmbeddingSwitch`.
4. `src/providers/embeddings/` (trait, erros, provedores, `local`, `HashEmbedder`); `get_json` e helpers do cliente LLM compartilhados.
5. Configurações `embedding.*` e `search.*`; `src/core/embeddings/` (configuração, runtime, guarda, troca, reindexação, operações, status) e o job `embed` com o runner `Embed`.
6. `search_memories` com ramo vetorial, `fuse` e `mode`; `get_hints`; `content_jobs(title_only, embed)`; guarda assíncrona no Redator; `get_status.embedding`; `ModelsSettings` (`MODELS_PATH`, `ORT_DYLIB_PATH`) no `Core::new` e `sync_embeddings` no `serve`; item `onnx_runtime` no `doctor`.
7. Catálogo (4 operações, tags `Search` e `Embeddings`), REST (`src/rest/search.rs`, `src/rest/embeddings.rs`), OpenAPI, MCP, CLI (`hints`, `settings test-embedding`, `settings embedding-models`, `embeddings reembed`) e os 3 códigos de erro.
8. Testes (`tests/it/embeddings.rs`, unitários, matriz e contrato), guias, skill, `aicontext`, desenho, decisões 0018–0021, `Dockerfile`, versão 0.7.0.
9. Imagem construída e testada localmente com `provider = local`: modelo baixado para `/data/models` (465 MB), 3 memórias vetorizadas, busca `hybrid` e dicas.

## Mudanças além do contrato

- O título aplicado pelo Redator reenfileira `embed` (D-014); o vetor só é gravado com título e `content_hash` iguais aos lidos (D-013).
- `get_status.embedding` ganhou `enabled`, `configured`, `missing` e `reindex_started_at`; `reembed_all` devolve `{ model_id, started_at, total }`.
- `list_embedding_models` usa `embedding.base_url`/`embedding.api_key` quando o corpo não traz; a CLI lê a chave com `--api-key-stdin`.
- `doctor` com o item `onnx_runtime`; `ORT_DYLIB_PATH` validado no startup quando definido.
- O teste de contrato das configurações mostra o padrão de `text` entre crases, como o de `enum`.

## Comandos de teste

```bash
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
UPDATE_SNAPSHOTS=1 cargo nextest run -E 'test(/contract::/)'
cargo nextest run -E 'test(/embeddings::|matrix::|contract::|redator::/)'
cargo nextest run --locked
docker build -t engramind-api:007-test .
```

## Rascunho de release notes

Busca por sentido: cada memória ganha um vetor (provedor local por padrão, sem custo e sem o texto sair do serviço, ou OpenAI-compatível, Ollama e Gemini), `search_memories` funde sentido e palavras e `get_hints` dá a "lembrança vaga" por turno. A troca de modelo valida o provedor e reindexa com segurança, com progresso em `get_status`. O Redator passa a aplicar o texto revisado quando a guarda de sentido confirma.
