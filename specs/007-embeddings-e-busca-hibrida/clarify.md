# Clarify - embeddings e busca híbrida

**Atualizado em**: 2026-10-31

## Perguntas

| ID | Tópico | Pergunta | Status | Resolução |
|----|--------|----------|--------|-----------|
| Q-001 | Limiar das dicas | `search.hint_threshold` (0,80) é uma similaridade, mas o RRF não é normalizado. Sobre o que o limiar se aplica? | resolvida | Sobre o cosseno do ramo vetorial (`1 - vec_distance_cosine`). Sem embeddings, `off` ou falha do provedor, `get_hints` devolve lista vazia: dica é "lembrança vaga" e não deve trazer ruído textual. |
| Q-002 | Remoção do modelo local | Remover do disco o modelo local antigo após a reindexação é obrigatório? | resolvida | Sim, só depois de `reembed_all` terminar sem jobs falhos e se o modelo anterior era local e deixou de ser o vigente. Falha ao remover gera `WARN`, nunca falha a operação. |
| Q-003 | `similar` vetorial na escrita | FR-006 pedia cosseno, mas no `create_memory` a memória nova ainda não tem vetor e a escrita não pode esperar o provedor. | resolvida | `similar` continua textual na escrita (0.4.0). Para usar cosseno seria preciso vetorizar o texto novo na requisição (5–300 ms de provedor), o que viola o desenho §7. O cosseno entra na 009, onde o consolidador compara memórias já vetorizadas. FR-006 ajustado. |
| Q-004 | Busca durante a reindexação | O desenho §11 diz que, até terminar, a busca usa os vetores do modelo anterior; FR-008 falava do modelo novo. | resolvida | Vale FR-003: só o `model_id` vigente entra na busca. Durante a reindexação, o ramo vetorial cobre apenas as memórias já reprocessadas; o FTS cobre o resto. Não existe caminho para consultar com o modelo anterior sem guardar a configuração e a chave antigas. O desenho §11 será corrigido. |
| Q-005 | Estado da reindexação | Onde guardar `state`, `total` e `started_at` para o `409` e o progresso sobreviverem a reinício? | resolvida | Chave `embedding.reindex` em `schema_meta` (a tabela chave/valor da 0001; não existe `meta`), JSON `{ model_id, started_at, total }`, com `done` contado em `memory_vectors.updated_at >= started_at` e limitado a `total`. O fim é decidido pelos jobs da marca, não pela contagem (D-007). Nenhuma tabela nova. |
| Q-006 | Formato dos jobs da reindexação | Um job `reembed` longo, um `embed` por memória ou lotes? | resolvida | `reembed_all` enfileira jobs `embed` em lotes de `embedding.batch_size` (`payload.memory_ids`). O handler aceita `memory_id` (escrita normal) ou `memory_ids` (lote), faz uma chamada por lote e grava cada vetor com compare-and-set. Lease curto e backoff da fila continuam valendo. |
| Q-007 | Chave dos embeddings | FR-008 exige teste ao salvar; a chave é segredo gravado por `set_secret`. | resolvida | `embedding.api_key` entra no catálogo como `Secret`; `update_settings` continua recusando segredo (`SETTING_IS_SECRET`). Trocar a chave não invalida vetores nem reindexa; a próxima chamada falha visivelmente se a chave estiver errada. O teste ao salvar vale para provedor, modelo, `base_url`, dimensões e prefixos. |
| Q-008 | Pesos e candidatos do RRF | Quantos candidatos cada ramo devolve e como arredondar? | resolvida | Cada ramo devolve até o `limit` efetivo da requisição (máximo 50). `score` é a soma ponderada, arredondada a 6 casas; desempate por `id` crescente. `rank` é renumerado depois da fusão. |
| Q-009 | Registro das chamadas de embedding | Vão para `llm_calls`? | resolvida | Não. `llm_calls` é do Redator (finalidade `redact`/`test`). Embeddings aparecem em `get_status.embedding`, nos jobs e nos logs estruturados, sem texto nem vetor. |
| Q-010 | Runtime local | Como embarcar o ONNX Runtime e quanto ele pesa? | resolvida | `fastembed` 7.5 com `ort-load-dynamic` e `hf-hub-rustls-tls`; a imagem copia `libonnxruntime.so` de versão fixa (compatível com `ort` 2.0.0-rc.15) e define `ORT_DYLIB_PATH`. Se o spike medir custo alto de compilação, isola-se atrás da feature `local-embeddings` (ligada na imagem e na cobertura, desligada nos shards, que usam `HashEmbedder`). |
| Q-011 | Timeout e tentativas do job `embed` | Qual lease e quantas tentativas? | resolvida | Lease = `embedding.timeout_ms + 60 s`; até 5 tentativas com o backoff da 0.6, depois `failed` com um `ERROR jobs.failed`. Concorrência 2 (o ONNX já usa vários threads). |
| Q-012 | Modo `hybrid` quando há vetores mas nenhum resultado | `mode` descreve o ramo ou o resultado? | resolvida | Descreve o ramo: `hybrid` quando a consulta foi vetorizada e o ramo vetorial rodou, mesmo que devolva zero candidatos; `text` quando o ramo não rodou. |
| Q-013 | Contadores e `usage_log` das dicas | `get_hints` conta recuperação? | resolvida | Sim: incrementa `retrieval_count` das memórias devolvidas e grava `usage_log` com `op = get_hints` (sem `rendered_tokens`). Não há injeção, então `injected_count` não muda. |
| Q-014 | Dimensão detectada | Onde gravar a dimensão detectada na primeira chamada? | resolvida | Em `memory_vectors.dims`; `model_id` só inclui `:<dimensions>` quando a configuração traz o valor. Não há escrita escondida em `embedding.dimensions`. |
| Q-015 | Canais das dicas | `manage` citado entra nas dicas? | resolvida | Não. O desenho §9 diz só `inject` e `search`; `get_hints` filtra `Reach.mode` para esses dois, mesmo com o slug citado. |
| Q-016 | Rotas e tags | Onde ficam as operações novas no OpenAPI? | resolvida | Tag nova `Search` (qualquer token) para `search_memories` e `get_hints`, coberta pelo guia novo `busca-e-dicas`; tag nova `Embeddings` (admin) para `test_embedding`, `list_embedding_models` e `reembed_all`, coberta por `configuracoes`. `search_memories` sai de `Memories`. |
| Q-017 | Modelo local padrão | O `MultilingualE5Small` do fastembed 7.5 é fp32 (`onnx/model.onnx`, ~470 MB), não quantizado. | resolvida | Fica o fp32 do próprio fastembed; spec e desenho corrigidos. Um quantizado exigiria carregar arquivos de outro repositório e quebraria a listagem. |
| Q-018 | Validação do `local` ao salvar | Validar chamando o modelo baixaria centenas de MB na requisição de `update_settings`. | resolvida | No `local`, a troca só confere o modelo em `list_supported_models()` (e pega a dimensão dele); o download fica para o primeiro job. `test_embedding` carrega de verdade. |
| Q-019 | Prazo da consulta | `embedding.timeout_ms` (15 s) travaria `search_memories` e `get_hints`. | resolvida | Constante `QUERY_TIMEOUT` = 2 s (no máximo `embedding.timeout_ms`), documentada, sem chave nova; passou, `mode: text`. |
| Q-020 | `off` na escrita | Hoje toda escrita enfileira `embed` com `pending`. | resolvida | O `core` decide (`content_jobs(title_only, embed)` e `Redaction.embed`) e o `Store` grava `embedding_state` pela presença do job: `pending` ou `off`. |
| Q-021 | Lote falho na reindexação | O que acontece se um lote esgota as tentativas? | resolvida | `get_status.embedding.state = failed`, memórias `failed`, nada é apagado; `reembed_all` é aceito de novo. |

## Decisões

| ID | Decisão | Fonte | Impacto |
|----|---------|-------|---------|
| D-001 | `model_id` = `<provider>:<model>[:<dimensions>]`; a dimensão só entra quando configurada. | desenho §11 | Chave de separação de vetores. |
| D-002 | Prefixos automáticos para a família e5 (`query: `/`passage: `), vazio no restante; `embedding.query_prefix`/`document_prefix` sobrescrevem. O embedder recebe `EmbedKind::{Query, Document}` e aplica o prefixo. | desenho §11 | `similar` e guarda usam `Document` dos dois lados. |
| D-003 | `fastembed` 7.5.0 (rust-version 1.88) com `default-features = false`, `ort-load-dynamic` e `hf-hub-rustls-tls`; `TextEmbedding::try_new` com `cache_dir = MODELS_PATH`; instância em cache no `Core`. ONNX Runtime fixado e copiado para a imagem. | desenho §16 | Imagem maior; sem OpenSSL no runtime. |
| D-004 | Provedor atrás de `trait Embedder { fn embed(&self, model, inputs) -> EmbedFuture }` com future em `Box` (uma função `-> impl Future` no trait impediria `Arc<dyn Embedder>`); prefixos, prazo e conferência ficam no `EmbeddingRuntime` do `core`. Local em `spawn_blocking` atrás de um `Mutex` (`TextEmbedding::embed` exige `&mut self`). `HashEmbedder` nos testes, injetável por `Core::set_local_embedder` (substitui só o provedor local). | `aicontext/use-quando-integrar-provedores.md` | Testes sem rede e sem ONNX. |
| D-005 | `memory_vectors` tem PK `memory_id` (um vetor por memória), `dims`, `updated_at` e FK `ON DELETE CASCADE`; sem tabela de progresso. | desenho §6 | Reindexar substitui o vetor; busca filtra o modelo vigente. |
| D-006 | `similar` fica textual na escrita; cosseno só na 009. | Q-003 | FR-006 ajustado; nenhuma espera de provedor na escrita. |
| D-007 | A reindexação é uma sequência de jobs `embed` em lote, marcados com `payload.reindex = started_at`; o estado vive em `schema_meta`; termina quando não sobra job pendente nem falho da marca. | Q-005, Q-006, Q-021 | Sem job longo nem lease especial; excluir memórias no meio não trava. |
| D-008 | A busca usa só o modelo vigente; durante a reindexação o ramo vetorial cresce de zero até `total`. | Q-004 | Corrige o desenho §11. |
| D-009 | `embedding.*` e `search.*` entram no catálogo `SETTINGS` sem sobreposição por canal; `embedding.api_key` é `Secret`. | desenho §12 | Guia `configuracoes` e teste de contrato 6. |
| D-010 | Guarda de sentido construída sob demanda a partir da configuração de embedding; `Core::set_meaning_guard` continua sendo o gancho de teste e tem precedência. | 006 Q-001 | Sem ciclo de `Arc` no `Core`. |
| D-011 | `get_hints` é leitura por POST (como `search_memories`), sem `Idempotency-Key`; a exceção do teste de catálogo passa a listar as duas operações. | contrato §15 | `read_only = true`, `annotations.read_only_hint = true`. |
| D-012 | A gravação de `update_settings` que mexe em embedding é atômica (um único método do `Store`: `update_settings(…, Some(EmbeddingSwitch), …)`) com o `model_id` vigente em `schema_meta`, estados, jobs em lote e o registro da reindexação; a validação do provedor acontece antes da transação. | FR-008 | Nada de transação aberta durante chamada HTTP. |
| D-013 | O `model_id` vigente fica em `schema_meta['embedding.model_id']`; `save_vectors` só grava com ele igual ao do lote e com `content_hash` **e** título iguais aos lidos. | revisão do plano | Vetor de modelo antigo nunca entra depois de uma troca. |
| D-014 | O título aplicado pelo Redator reenfileira `embed` (o vetor é de `título + texto`). | FR-001 | Vetor coerente com o título final. |
| D-015 | A guarda devolve o **cosseno** (similaridade), não `1 - cosseno`. | revisão do plano | Corrige o contrato. |
| D-016 | Nome canônico do modelo local: o repositório sem dono nem `-onnx` quando único (`bge-m3`), senão o enum em kebab-case; a busca aceita qualquer grafia com as mesmas letras e dígitos. | T002 | `model_id` estável (`local:bge-m3`). |

## Mudanças no desenho e na spec

1. **Desenho §11**: trocar "até terminar, a busca usa os vetores do modelo anterior ainda válidos" por "a busca usa só os vetores do modelo vigente; durante a reindexação o ramo vetorial cobre as memórias já reprocessadas e o FTS cobre o resto".
2. **Desenho §6**: `memory_vectors` ganha `updated_at` e FK com cascata; `dims` passa a `NOT NULL`.
3. **Desenho §7**: a frase "pelo vetor quando já existir" em `similar` fica marcada como 009.
4. **Desenho §16**: registrar a versão do ONNX Runtime, `ORT_DYLIB_PATH` e o tamanho acrescentado à imagem.
5. **Spec FR-006/FR-008**: ajustados conforme Q-003 e Q-004.

## Confirmações

A usuária aprovou as recomendações de Q-017 a Q-021, as três mudanças de desenho (D-006, D-008 e o §16), a troca de tag de `search_memories` (`Memories` → `Search`) e o teto fixo de 5 tentativas do `embed` (2026-10-31).

## Bloqueios

Nenhum.
