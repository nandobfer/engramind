# Spec - embeddings e busca híbrida

**Criado em**: 2026-10-30
**Atualizado em**: 2026-10-31 (implementado na 0.7.0)
**Status**: Implementado (0.7.0)
**Versão alvo**: 0.7.0
**Input**: Desenho §6 (`memory_vectors`), §7 (embeddings e guarda de sentido), §9 (busca e dicas), §11 (local ou remoto), §12 (`embedding`, `search`), §16 (ONNX Runtime na imagem).

## Objetivo

A busca encontra memórias pelo sentido, as dicas funcionam, a escrita aponta quase-duplicatas pelo critério disponível e a guarda de sentido do Redator passa a aplicar o texto revisado. Tudo configurável pelas três vias, com reindexação segura na troca de modelo.

## Escopo

- `memory_vectors` (um vetor por memória) e o job `embed`, drenando o backlog enfileirado desde a 0.4.0.
- Provedores `local` (`fastembed` 7.5 + ONNX Runtime, `multilingual-e5-small` fp32 do fastembed por padrão), `openai_compatible`, `ollama`, `gemini` e `off`.
- Ramo vetorial (`vec_distance_cosine`, varredura exata filtrada) e fusão RRF ponderada em `search_memories`.
- `get_hints`; guarda de sentido real no Redator; `similar` mantido no critério textual da 0.4.0 (ver FR-006 e D-006).
- `reembed_all`, troca de modelo sem misturar dimensões nem modelos, progresso em `get_status`.
- `test_embedding`, `list_embedding_models`; configurações `embedding.*` e `search.*`.
- ONNX Runtime na imagem; modelo baixado para `MODELS_PATH`; cache local por processo.
- Guia `busca-e-dicas`, tag OpenAPI `Search` (qualquer token) e `Embeddings` (admin).

## Requisitos

| ID | Requisito |
|----|-----------|
| FR-001 | O job `embed` gera o vetor de `title + "\n" + text` com o prefixo de documento do modelo e grava `memory_vectors(memory_id, model_id, dims, embedding, updated_at)`; `embedding_state = ready`. O prefixo é aplicado pelo `EmbeddingRuntime`, uma única vez. O título aplicado pelo Redator também reenfileira o `embed`. |
| FR-002 | Só modelos de embedding; a dimensão vem da configuração (`embedding.dimensions`) ou da primeira resposta e é conferida em todas as seguintes. Divergência numa gravação falha o job com `EMBEDDING_DIMENSION_MISMATCH`. |
| FR-003 | A busca vetorial compara só vetores do `model_id` vigente com a consulta (`query_prefix + query`); nunca mistura modelos nem dimensões. |
| FR-004 | `score = Σ peso / (search.rrf_k + posição)` com `search.vector_weight` e `search.text_weight`, arredondado a 6 casas. Sem ramo vetorial (off, provedor sem configuração, falha/timeout na consulta, dimensão divergente ou nenhum vetor do modelo vigente) a busca usa só o FTS e devolve `mode: "text"`; com os dois ramos, `mode: "hybrid"`. |
| FR-005 | `get_hints { text, channels?, limit? }` busca em canais `inject` e `search` (nunca `manage`, nem citado), exclui o que está no contexto, aplica `search.hint_threshold` sobre o cosseno do ramo vetorial e devolve só `{ id, title, channel }`, até `search.hint_limit`. |
| FR-006 | `similar` na escrita continua pelo Jaccard dos termos sobre os candidatos do FTS (0.4.0): o texto novo ainda não tem vetor e a escrita não espera o provedor. O cosseno entra na 009, quando o consolidador compara memórias já vetorizadas. |
| FR-007 | Guarda de sentido: cosseno(original, revisado) < `redator.min_similarity` → mantém o original e aplica só o título; provedor indisponível, desligado ou sem configuração → mesma regra (falha fechada). Com `redator.rewrite_text = false`, a guarda não é chamada. |
| FR-008 | Salvar outro provedor, modelo, `base_url`, dimensão ou prefixo de embedding chama o provedor na mesma requisição (teste interno; no `local`, só confere o modelo, Q-018) e só grava se ele responder; a gravação enfileira `reembed_all` na mesma transação. Falha do provedor recusa a mudança inteira. |
| FR-009 | `reembed_all` processa em lotes de `embedding.batch_size`, com progresso `{ state, model_id, total, done }` em `get_status`; termina quando não sobra job pendente nem falho da reindexação e então apaga vetores de outros modelos e remove do disco os modelos locais fora de uso. Lote falho de vez → `state = failed` (Q-021). |
| FR-010 | `embedding.provider = off` mantém contexto e busca textual funcionando; memórias ficam `embedding_state = off`, nenhum job `embed` é enfileirado e `get_hints` devolve lista vazia. Religar enfileira só o que não tem vetor do modelo vigente. |
| FR-011 | `list_embedding_models { provider, base_url?, api_key? }` lista quando o provedor oferece listagem (local, Ollama, OpenAI-compatível); Gemini e `off` respondem `400 PROVIDER_LISTING_UNSUPPORTED`. |
| FR-012 | Chamadas remotas com `embedding.timeout_ms`; falha temporária volta à fila com o backoff da 0.6 e teto de 5 tentativas. Logs nunca têm texto, vetor, chave ou URL com credencial. |
| FR-013 | `delete_memory` remove o vetor na mesma transação (cascata da chave estrangeira). |
| FR-014 | Falha do provedor na consulta de `search_memories` não é erro: `WARN` e `mode: "text"`; a consulta espera no máximo 2 s (Q-019). Em `get_hints`, lista vazia. |
| FR-015 | `search.default_limit`, `search.rrf_k`, `search.vector_weight`, `search.text_weight`, `search.hint_threshold` e `search.hint_limit` valem de fato (hoje `RRF_K` e `DEFAULT_LIMIT` são constantes). |
| FR-016 | `get_status` ganha `embedding` com provedor, modelo, `model_id`, estado (`off`/`ready`/`reindexing`/`failed`), `total`, `done` e dimensão. |
| FR-017 | `reembed_all` com reindexação em andamento responde `409 REEMBED_IN_PROGRESS`; o estado sobrevive a reinício. |
| FR-018 | `test_embedding` valida uma configuração proposta sem gravá-la e devolve provedor, modelo, `model_id`, dimensão detectada e latência. |
| FR-019 | O provedor local carrega o modelo uma vez por processo, em `spawn_blocking`, com `MODELS_PATH` como cache; trocar de modelo invalida o cache. |

## Fora do escopo

- Índice `vec0` particionado (escala).
- Consolidação e similaridade vetorial na escrita (009).
- Registro de chamadas de embedding em `llm_calls` (a tabela é do LLM; o desenho não pede).
- Tabela de preços e custo de embeddings.
- Feature flag `local-embeddings`: o spike T002 mostrou custo de compilação aceitável (decisão 0019).

## Critérios de sucesso

- Com `HashEmbedder`, "rodar todos os testes" e "suíte completa" caem perto no vetor e a ordem RRF é a esperada com os pesos padrão; mudar os pesos muda a ordem de forma previsível.
- Troca de modelo com dimensões diferentes não gera erro nem resultado misturado durante a reindexação: o ramo vetorial simplesmente encolhe até cobrir tudo.
- `get_hints` não devolve ruído textual quando não há embeddings (lista vazia) e respeita o limiar sobre o cosseno quando há.
- A imagem sobe com `provider = local`, baixa `multilingual-e5-small` para `/data/models` e gera vetores; `off` mantém contexto e busca textual.
