# 0019 — Provedores de embedding e ONNX Runtime local

**Data**: 2026-10-31 · **Etapa**: 007 (0.7.0) · **Status**: aceita

## Contexto

O desenho (§11, §16) quer o provedor `local` (fastembed, `multilingual-e5-small`) como padrão, com a biblioteca do ONNX Runtime na imagem e o modelo baixado para `MODELS_PATH`, e os remotos OpenAI-compatível, Ollama e Gemini. O spike T002 mediu o custo.

## Medições (T002)

- `fastembed` 7.5.0 com `default-features = false` e `ort-load-dynamic` + `hf-hub-rustls-tls` compila no toolchain 1.98 (inclusive o `onig` do `tokenizers` com o `cc` desta máquina); a compilação limpa de dev ganhou cerca de 2 min e o binário de release, poucos MB. Sem feature `local-embeddings`: os shards compilam tudo e usam o `HashEmbedder`.
- O `ort` 2.0.0-rc.15 exige ONNX Runtime 1.24 ou mais novo (o `fastembed` liga `api-24`) e empacota o 1.31; a imagem usa o `onnxruntime-linux-x64-1.31.1` oficial, conferido por SHA-256.
- `multilingual-e5-small` no fastembed é o ONNX **fp32** (`onnx/model.onnx`, 465 MB no cache), não quantizado: carga em ~5 s, ~1 GB de RSS, 32 textos em ~280 ms e uma consulta em ~20 ms na CPU (8 núcleos). Q-017: fica o fp32; um quantizado exigiria carregar arquivos de outro repositório e quebraria a listagem.
- Sem a biblioteca, o `ort` entra em pânico ("Failed to load ONNX Runtime dylib").
- Os cossenos do e5 ficam comprimidos (0,81–0,88 entre frases quaisquer).

## Decisão

- `trait Embedder { fn embed(&self, model, inputs) -> EmbedFuture }` com future em `Box`, para `Arc<dyn Embedder>` e para trocar o local nos testes (`Core::set_local_embedder`). O `core` aplica prefixos, prazo e confere quantidade, dimensão e valores finitos.
- **Local**: `TextEmbedding` em cache no processo atrás de um `Mutex` (`embed` exige `&mut self`), em `spawn_blocking`, `intra_threads = 2`, `cache_dir = MODELS_PATH`. Antes de tocar no `ort`, confere `ORT_DYLIB_PATH` (lido por `config.rs` e validado no startup quando definido); sem ele, `EmbedError::Runtime`. O pânico de outra origem fica contido no `spawn_blocking`.
- Nome canônico do modelo local: o repositório sem dono nem `-onnx` quando único (`bge-m3`, `multilingual-e5-small`), senão o enum em kebab-case; a busca aceita qualquer grafia com as mesmas letras e dígitos.
- **Remotos** sobre o `reqwest` do Redator (`post_json`/`get_json`, mesma classificação de erro): OpenAI-compatível `POST /embeddings` (`dimensions` opcional, ordem por `index`) e `GET /models`; Ollama `POST /api/embed` e `GET /api/tags`; Gemini `batchEmbedContents` com `x-goog-api-key` e `outputDimensionality`, sem listagem.
- `doctor` ganha o item `onnx_runtime` (pulado sem `ORT_DYLIB_PATH`).

## Consequências

- Imagem com ~29 MB a mais (a `.so`) e o volume `/data` com o modelo; o limite de 2 GB do compose comporta o fp32.
- Sem `ORT_DYLIB_PATH` (desenvolvimento local), o `local` falha visivelmente: jobs `failed` e busca só textual.
