# 0018 — Vetores numa tabela comum e varredura exata

**Data**: 2026-10-31 · **Etapa**: 007 (0.7.0) · **Status**: aceita

## Contexto

O desenho (§6, §9) prevê `memory_vectors` com o vetor `float32` do sqlite-vec e a busca por `vec_distance_cosine` sobre as linhas filtradas, deixando o índice `vec0` particionado para a escala. Faltava decidir quantos vetores por memória, como separar modelos e onde marcar o modelo vigente.

## Decisão

- **Um vetor por memória** (`memory_vectors.memory_id` PK, `ON DELETE CASCADE`), com `model_id`, `dims NOT NULL`, o BLOB `f32` little-endian sem cabeçalho e `updated_at` (marco do progresso da reindexação). Reindexar substitui o vetor; não há tabela de progresso (migração `0007_vectors.sql`).
- **Varredura exata**: `ORDER BY vec_distance_cosine(v.embedding, ?)` com `v.model_id = ?` **e** `v.dims = ?` e os mesmos filtros de canal, categoria, status e exclusão do FTS. Nenhum vetor de outro modelo ou dimensão entra na comparação.
- **Modelo vigente no `Store`**: `schema_meta['embedding.model_id']` (vazio com `off`), gravado na mesma transação das configurações e conferido por `save_vectors`. Um job que vetorizou com o modelo anterior não grava depois de uma troca, mesmo que a troca caia entre a chamada e a gravação.
- **Compare-and-set por memória**: o vetor só é gravado se `content_hash` **e** título ainda forem os lidos pelo job; o título conta porque o vetor é de `título + "\n" + texto`.

## Consequências

- A busca nunca dá erro de dimensão durante uma troca: o ramo vetorial só encolhe.
- Com dezenas de milhares de memórias, a varredura continua em milissegundos; o `vec0` particionado por canal troca a consulta sem mudar o contrato.
