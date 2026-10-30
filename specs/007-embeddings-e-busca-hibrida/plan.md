# Plan - embeddings e busca híbrida

**Data**: 2026-10-30
**Referência de spec**: [`spec.md`](spec.md)

## Resumo

Adicionar embeddings configuráveis, o ramo vetorial com RRF, dicas, guarda de sentido e reindexação segura.

## Contexto técnico

Novas dependências: `fastembed` (ONNX Runtime). Imagem de runtime com a biblioteca do ONNX Runtime.

## Áreas afetadas

**Core**: `src/core/search.rs` (RRF), `src/core/hints.rs`, `src/core/redator/guard.rs`, `src/providers/embeddings/` (local, openai_compatible, ollama, gemini).
**Dados**: `migrations/0007_vectors.sql`, `src/store/sqlite/vectors.rs`.
**Deploy**: `Dockerfile` (ONNX Runtime), `.env.example` (`MODELS_PATH`).
**Docs**: guias `busca-e-dicas` e `configuracoes`, `aicontext/search.md`, `aicontext/embeddings.md`.

## Estratégia de implementação

1. Resolver Q-001 e Q-002.
2. Spike `fastembed` + ONNX Runtime na imagem `debian:bookworm-slim` (tamanho e carga do modelo).
3. `Embedder` e os provedores; `HashEmbedder` de teste.
4. Job `embed`, backlog e `delete` do vetor.
5. Ramo vetorial e RRF; `similar` e `get_hints`.
6. Guarda de sentido no Redator.
7. Troca de modelo, `reembed_all`, progresso e limpeza.
8. Testers, listagem, três vias, guias, versão 0.7.0.

## Estratégia de testes

**Unit**: RRF, prefixos, `model_id`, limiar das dicas.
**Integração**: `HashEmbedder` determinístico (busca, `similar`, dicas, guarda), servidor HTTP local para os provedores remotos (timeout, dimensão divergente), reindexação com mudança de dimensão sem mistura.
**Manual**: imagem com provedor local.

## Riscos

| Risco | Mitigação |
|-------|-----------|
| Imagem grande ou ONNX incompatível com bookworm | Spike no passo 2 antes do restante. |
| Primeira carga do modelo lenta | Carga no primeiro uso em background; jobs `embed` aguardam o modelo pronto. |
| `fastembed`/ONNX aumentar muito o tempo de compilação dos testes e do Clippy | Medir no spike; se pesar, isolar o provedor local na feature `local-embeddings` (ligada na imagem e no job de cobertura, desligada no archive dos shards, que usam `HashEmbedder`), registrando a decisão. |
| Varredura exata ficar lenta com volume | Medir em `get_status`; `vec0` particionado é a etapa de escala. |
