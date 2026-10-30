# Spec - embeddings e busca híbrida

**Criado em**: 2026-10-30
**Status**: Draft
**Versão alvo**: 0.7.0
**Input**: Desenho §7 (embeddings, guarda de sentido), §9, §11, §12 (`embedding`, `search`), §16 (ONNX Runtime na imagem).

## Objetivo

A busca encontra memórias pelo sentido, as dicas funcionam, a escrita aponta quase-duplicatas pelo vetor e a guarda de sentido do Redator passa a valer.

## Escopo

- `memory_vectors` e job `embed` (processa o backlog desde a 004).
- Provedores `local` (`fastembed`, `multilingual-e5-small` quantizado por padrão), `openai_compatible`, `ollama`, `gemini` e `off`.
- Ramo vetorial (`vec_distance_cosine`, varredura exata filtrada) e fusão RRF em `search_memories`.
- `similar` pelo vetor; `get_hints`; guarda de sentido do Redator.
- `reembed_all`, troca de modelo sem misturar dimensões, progresso em `get_status`.
- `test_embedding`, `list_embedding_models`; configurações `embedding.*` e `search.*`.
- ONNX Runtime na imagem; modelos baixados para `MODELS_PATH`.
- Guia `busca-e-dicas`.

## Requisitos

| ID | Requisito |
|----|-----------|
| FR-001 | O job `embed` gera o vetor de `document_prefix + title + "\n" + text` e grava `memory_vectors(model_id, dims, embedding)`; `embedding_state = ready`. |
| FR-002 | Só modelos de embedding; dimensão detectada na primeira chamada e conferida em todas as seguintes. |
| FR-003 | A busca vetorial compara só vetores do `model_id` vigente com a consulta (`query_prefix + query`); nunca mistura modelos ou dimensões. |
| FR-004 | `score = Σ peso / (search.rrf_k + posição)` com `search.vector_weight` e `search.text_weight`; sem embeddings (off, pendentes ou falha do provedor na consulta) a busca usa só o FTS e informa `mode: "text"` na resposta. |
| FR-005 | `get_hints { text, channels?, limit? }` busca em canais `inject` e `search`, exclui o contexto, aplica `search.hint_threshold` e devolve só `{ id, title, channel }`. |
| FR-006 | `similar` usa cosseno quando a memória e as candidatas têm vetor; senão o critério textual da 004. |
| FR-007 | Guarda de sentido: cosseno(original, revisado) < `redator.min_similarity` → mantém o original e aplica só o título; provedor indisponível → mesma regra (falha fechada). |
| FR-008 | Salvar outro provedor ou modelo exige `test_embedding` bem-sucedido na mesma requisição (validação interna) e enfileira `reembed_all`; até terminar, a busca usa os vetores ainda válidos do modelo novo e o FTS. |
| FR-009 | `reembed_all` processa em lotes de `embedding.batch_size`, com progresso `{ total, done, model_id }` em `get_status`; ao terminar, apaga vetores de outros modelos e remove do disco o modelo local que deixou de ser usado. |
| FR-010 | `embedding.provider = off` mantém contexto e busca textual funcionando; `embedding_state = off`. |
| FR-011 | `list_embedding_models { provider, base_url?, api_key? }` lista os modelos quando o provedor oferece listagem (local, Ollama, OpenRouter); senão `400 PROVIDER_LISTING_UNSUPPORTED`. |
| FR-012 | Chamadas remotas com timeout `embedding.timeout_ms` e retentativa limitada; logs sem texto. |
| FR-013 | `delete_memory` remove o vetor. |

## Fora do escopo

- Índice `vec0` particionado (escala).
- Consolidação (009).

## Critérios de sucesso

- Com embeddings por hash, "rodar todos os testes" encontra "suíte completa" quando o falso mapeia os dois para vetores próximos, e a ordem RRF é a esperada.
- Troca de modelo com dimensões diferentes não gera erro nem resultado misturado durante a reindexação.
- Imagem sobe com `provider = local`, baixa o modelo para `/data/models` e gera vetores.
