# Clarify - embeddings e busca híbrida

**Atualizado em**: 2026-10-30

## Perguntas

| ID | Tópico | Pergunta | Status | Resolução |
|----|--------|----------|--------|-----------|
| Q-001 | Limiar das dicas | `search.hint_threshold` (0,80) é uma similaridade, mas o RRF não é normalizado. Sobre o que o limiar se aplica? | aberta | Recomendação: sobre o cosseno do ramo vetorial; sem embeddings, `get_hints` devolve lista vazia (dica é "lembrança vaga" e não deve trazer ruído textual). |
| Q-002 | Remoção do modelo local | Remover do disco o modelo local antigo após a reindexação é obrigatório? | aberta | Recomendação: sim, como no desenho, só depois de `reembed_all` terminar sem falhas. |

## Decisões

| ID | Decisão | Fonte | Impacto |
|----|---------|-------|---------|
| D-001 | `model_id` = `<provider>:<model>[:<dimensions>]`. | desenho §11 | Chave de separação de vetores. |
| D-002 | Prefixos conhecidos preenchidos automaticamente para modelos locais (`query: `/`passage: ` na família e5); configuráveis para os demais. | desenho §11 | — |
| D-003 | `fastembed` com ONNX Runtime ligado dinamicamente; a imagem de runtime copia a biblioteca do ONNX Runtime de versão fixa; `MODELS_PATH` é o cache do fastembed. | desenho §16 | `docs/decisions/` registra versão e tamanho da imagem. |
| D-004 | Provedor de embeddings atrás de `trait Embedder { async fn embed(&self, inputs: &[String], kind: EmbedKind) -> Result<Vec<Vec<f32>>, EmbedError>; fn model_id(&self) -> &str; }`; o local roda em `spawn_blocking`. | `aicontext/use-quando-integrar-provedores.md` | Testes usam `HashEmbedder`. |

## Bloqueios

Q-001 antes de `get_hints`.
