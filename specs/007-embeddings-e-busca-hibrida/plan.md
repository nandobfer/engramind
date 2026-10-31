# Plan - embeddings e busca híbrida

**Data**: 2026-10-31
**Referência de spec**: [`spec.md`](spec.md)

## Resumo

Ligar o job `embed` que já é enfileirado desde a 0.4.0, adicionar os provedores de embedding, o ramo vetorial com fusão RRF, as dicas, a guarda de sentido real e a reindexação segura na troca de modelo — nas três vias, com a imagem pronta para o provedor local.

## Contexto técnico

- Hoje: `embedding_state` e jobs `embed` existem, mas nenhum runner os captura (`aicontext/README.md`); `search.rs` usa só FTS com `score` fixo `1/(60+rank)`; `MeaningGuard` é `NoGuard`; `get_status` não tem bloco de embeddings; `MODELS_PATH` é lido em `config.rs` e não chega ao `Core`.
- `fastembed` 7.5.0 (rust-version 1.88) oferece `MultilingualE5Small`, `TextEmbedding::list_supported_models()` e cache em diretório; com `ort-load-dynamic` o binário carrega `libonnxruntime.so` do sistema (`ORT_DYLIB_PATH`).
- `sqlite-vec` já está registrado (`src/store/sqlite/vec_extension.rs`) e `vec_distance_cosine` está disponível; não há índice `vec0` nesta etapa.
- `Core::new` recebe só `DatabaseSettings`; `models_path` precisa entrar (novo parâmetro, ajustando `main`, `tests/it/support` e `doctor` se necessário).
- O teste de catálogo exige que toda operação seja `GET` ou `search_memories` para `read_only`; `get_hints` entra na exceção.

## Áreas afetadas

**Core**: `src/core/embeddings.rs` (novo), `src/core/hints.rs` (novo), `src/core/search.rs` (RRF + modo), `src/core/redator/guard.rs` + `src/core/redator/mod.rs` (guarda), `src/core/settings.rs` (catálogo e sincronização atômica), `src/core/status.rs`, `src/core/jobs/runner.rs`, `src/core/model.rs`, `src/core/mod.rs`, `src/core/operations.rs`.
**Provedores**: `src/providers/embeddings/{mod,local,openai_compatible,ollama,gemini}.rs`; `src/providers/mod.rs`.
**Adaptadores**: `src/rest/{memories,settings,embeddings}.rs`, `src/rest/mod.rs`, `src/app.rs`; `src/mcp/tools.rs`; `src/cli/mod.rs`, `src/cli/remote.rs`, `src/cli/help.rs`.
**Dados**: `migrations/0007_vectors.sql`, `src/store/mod.rs`, `src/store/sqlite/{mod,memories,vectors}.rs`.
**Deploy**: `Cargo.toml`, `Dockerfile`, `.env.example`, `src/doctor.rs` (`onnx_runtime`); sem mudança no CI (sem feature `local-embeddings`, decisão 0019).
**Docs**: `docs/guides/busca-e-dicas.md` (novo), `configuracoes.md`, `erros.md`, `operacao.md`, `escrever-memorias.md`, `contexto-e-cache.md`, `integracao-consumidor.md`, `integracao-mcp.md`, `overview.md`, `cli.md`; `skills/engramind/SKILL.md`; `aicontext/search.md`, `aicontext/embeddings.md`, `aicontext/README.md`; `docs/design.md` §6/§7/§9/§11/§16; `docs/decisions/0018`–`0021`; `CHANGELOG.md`.

## Estratégia de implementação

1. **Spike T002 (portão).** Compilar `fastembed` com `ort-load-dynamic` + `hf-hub-rustls-tls` no toolchain 1.98; medir tempo de compilação limpa, Clippy, tamanho do binário e da imagem; carregar `multilingual-e5-small` e medir RAM/latência da primeira chamada. Medir `TextEmbedding::list_supported_models()` e o layout do cache em `MODELS_PATH`. Decidir sobre a feature `local-embeddings` com o número na mão; se ela entrar, ligar em `Dockerfile`, cobertura e documentar a decisão 0019.
2. Migração `0007_vectors.sql`; tipos `StoredVector`, `VectorQuery`, `VectorCounts`; métodos do `Store`.
3. `trait Embedder`, `EmbedKind`, `EmbedError`, `HashEmbedder` (testes) e `Core::set_local_embedder`; configuração `EmbeddingConfig` lida do snapshot + segredo.
4. Provedores remotos (`openai_compatible`, `ollama`, `gemini`) com `reqwest` próprio (o cliente HTTP do `Core` já existe), timeout e validação de dimensão; `local` em `spawn_blocking`.
5. Job `Embed` (lote, compare-and-set, lease, backoff, limpeza ao concluir); `run_embed_jobs` para testes; conectar `spawn_workers`.
6. `reembed_all`, estado em `schema_meta`, progresso, `REEMBED_IN_PROGRESS`; sincronização atômica em `update_settings` com validação prévia.
7. Ramo vetorial + RRF em `search_memories`; `get_hints`; `mode`; `search.*` valendo de verdade.
8. Guarda `EmbeddingGuard` e ligação no Redator.
9. `test_embedding`, `list_embedding_models`, `get_status.embedding`.
10. Três vias (REST/OpenAPI, MCP, CLI), tags/guia, testes de contrato e matriz.
11. ONNX Runtime na imagem, `ORT_DYLIB_PATH`, `docs/decisions`, guias, `aicontext`, versão 0.7.0.

## Estratégia de testes

**Unitários**
- `model_id`, prefixos automáticos e explícitos, `batch_size`, `missing()`.
- RRF: pesos iguais, peso vetorial zero, item só num ramo, desempate por id, arredondamento.
- Cosseno/limiar das dicas; `HashEmbedder` determinístico.
- Guarda: acima/abaixo do limiar, erro do provedor → `None`, `rewrite_text = false` não chama.
- Configuração: mudanças que reindexam × que não reindexam; `off`.

**Integração (`tests/it/embeddings.rs` + `support/embedding.rs`)**
- `HashEmbedder` mapeando "rodar todos os testes" e "suíte completa" para vetores próximos: ordem RRF esperada, `mode: hybrid`, pesos mudando a ordem.
- `off`/provedor sem configuração/falha/timeout/dimensão divergente: `mode: text`, FTS preservado; `get_hints` vazio.
- `get_hints`: limiar, exclusão do contexto, canais `manage` fora, `inject`/`search` dentro, contadores e `usage_log`.
- Guarda no Redator: texto revisado aplicado (revisão `system/redator`, `version + 1`, job `embed`) e texto abaixo do limiar mantendo o original.
- Reindexação: troca de dimensão no meio, `done` crescendo, `409` durante, limpeza de vetores de outro modelo, remoção do modelo local (com `MODELS_PATH` temporário), reinício preservando o estado.
- `delete_memory` remove o vetor; `embedding_state` transitions; `off` → `on` sem reembedar o que já tem vetor.
- Provedores remotos com `ScriptedLlm`-like (`support/embedding.rs`): cada rota, `429`, `5xx`, timeout, resposta com dimensão errada, `list_embedding_models` e `test_embedding` (sem gravar configuração nem chave).
- `get_status.embedding` nos três estados.

**Contrato/matriz**
- Quatro operações novas em `OPERATIONS`, OpenAPI, MCP, CLI e guia; `get_hints` como leitura por POST.
- `tests/it/contract.rs`: snapshot, tags (`Search`, `Embeddings`), guia único, comandos citados, códigos de erro e configurações documentados.
- `tests/it/matrix.rs`: casos REST/MCP/CLI para as quatro operações; `expected_error` de `test_embedding`/`reembed_all` sem provedor (`PROVIDER_NOT_CONFIGURED`).

**Manual/CI**
- Imagem com `provider = local` baixando o modelo para `/data/models` e gerando vetores; `doctor` mostrando `vec_version`; `docker compose` continua subindo no portão existente.

## Observabilidade

- Eventos novos: `embeddings.ready` (modelo carregado, com duração), `embeddings.query_failed` (`WARN`, só motivo), `embeddings.dimension_mismatch` (`WARN`/`ERROR`), `embeddings.reindex_started`, `embeddings.reindex_finished`, `embeddings.model_removed`.
- Nunca logar texto, vetor, chave, URL com credencial ou conteúdo de memória em `INFO+`.
- `get_status` é a leitura operacional; nenhum endpoint novo de diagnóstico.

## Riscos

| Risco | Mitigação |
|-------|-----------|
| ONNX Runtime incompatível com `debian:bookworm-slim` ou versão de `ort` | Spike T002 antes de qualquer código de produção; versão fixa em build arg; `doctor`/health falham cedo se a lib não carregar. |
| Compilação e Clippy pesados para os shards | Medir no spike; se necessário, feature `local-embeddings` ligada na imagem/cobertura e desligada nos shards com `HashEmbedder` (decisão registrada). |
| Primeira carga do modelo lenta ou sem rede | Job `embed` espera o modelo em `spawn_blocking`; falha vira retry/backoff; `get_status` mostra `done/total`; o serviço continua servindo FTS e contexto. |
| Reindexação longa monopoliza o provedor | Lotes de `batch_size`, concorrência 2, lease curto; progresso visível. |
| Varredura exata lenta em volume alto | Medir; `vec0` particionado é a etapa de escala; sem mudança de contrato. |
| Troca de modelo com a chave no meio | Validação antes da transação; chave antiga não é guardada (D-008); documentar que a busca degrada para FTS até a reindexação. |
| `TextEmbedding::embed` exige `&mut self` | Instância atrás de um `Mutex`; a consulta da busca espera no máximo 2 s e cai para o FTS. |
| Reindexação travada por memória excluída no meio | Fim decidido pelos jobs da marca, não por `done >= total` (decisão 0020). |
| Cossenos comprimidos do e5 (0,81–0,88) | Limiares padrão do desenho mantidos; documentado que ~0,88 filtra melhor nas dicas. |
| Duas instâncias durante reindexação | Fora do escopo (uma instância por banco, desenho §16); o estado em `schema_meta` não coordena hosts. |

## Versão e entrega

- `0.7.0` em `Cargo.toml`/`Cargo.lock`; `CHANGELOG.md` com a etapa 007.
- `docs/openapi.json` regenerado com `UPDATE_SNAPSHOTS=1`.
- `implement.md` e `report.md` ao final, como nas etapas anteriores.
