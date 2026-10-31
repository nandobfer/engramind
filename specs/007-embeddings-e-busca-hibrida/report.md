# Report - embeddings e busca híbrida

**Data**: 2026-10-31 · **Versão**: 0.7.0

## Entrega

FR-001 a FR-019 implementados (detalhes em `implement.md`). Q-001 a Q-021 resolvidas (`clarify.md`); decisões 0018 a 0021.

## Verificação executada

| Verificação | Resultado |
|---|---|
| `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings` | ok |
| `cargo nextest run --locked` | 267 testes, todos passam |
| Busca híbrida (critério de sucesso) | Com `HashEmbedder`, "rodar todos os testes" encontra "Suite completa…" só pelo vetor; item nos dois ramos vem primeiro; `search.vector_weight = 0` muda a ordem; `search.default_limit` vale; `mode` `hybrid`/`text` |
| Degradação | Provedor com falha: `mode: text`, FTS preservado, dicas vazias; `off`: estados `off`, nenhum `embed`, religar enfileira só o que falta |
| Dicas | Limiar sobre o cosseno, contexto excluído, `manage` fora mesmo citado, só `{ id, title, channel }`, `usage_log` e `retrieval_count` |
| Troca de modelo (critério de sucesso) | e5-small (384) → bge-m3 (1024) em lotes de `batch_size`, busca sem o modelo antigo durante e sem erro de dimensão, `409 REEMBED_IN_PROGRESS`, vetores antigos apagados, diretório do modelo local fora de uso removido, modelo desconhecido recusado sem gravar |
| Reindexação | `reembed_all` `202` com idempotência, reinício preservando o estado, exclusão no meio sem travar, lote esgotado → `failed` e recomeço |
| Guarda de sentido | Revisão próxima aplicada; revisão de outro sentido mantém o texto e aplica o título |
| Provedores remotos | OpenAI-compatível (validação ao salvar com `503` e dimensão divergente recusados, `Authorization`, `dimensions`, sem prefixo e5), Ollama e Gemini (`x-goog-api-key`, `batchEmbedContents`), `test_embedding` sem gravar configuração nem chave, `list_embedding_models` local/Ollama/OpenAI e `PROVIDER_LISTING_UNSUPPORTED` |
| Matriz tipo × escopo × operação | 63 operações em REST, MCP e CLI |
| Contrato §15 | OpenAPI regenerado; tags `Search` e `Embeddings`; guia `busca-e-dicas`; códigos de erro e configurações documentados |
| Imagem (critério de sucesso) | `docker build` ok (+50 MB); `doctor` com `onnx_runtime ok`; com `provider = local`, o modelo foi baixado para `/data/models` (465 MB, carga em ~20 s, ~1 GB de RAM) e 3 memórias vetorizadas; busca `hybrid` e dicas respondendo |

## Pendências

- Com o e5, os limiares padrão (`0.8`) de `search.hint_threshold` e `redator.min_similarity` deixam passar quase tudo (cossenos de 0,81 a 0,88 entre frases sem relação); o ajuste fino fica para o operador (documentado).
- Na primeira carga do modelo local, os primeiros jobs podem estourar `embedding.timeout_ms` e voltam à fila (documentado em `operacao`).
- A imagem só traz o ONNX Runtime x86_64.
