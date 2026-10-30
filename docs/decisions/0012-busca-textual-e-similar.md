# 0012 — Busca textual e `similar` antes dos embeddings

**Data**: 2026-10-30 · **Etapa**: 004 (0.4.0) · **Status**: aceita

## Contexto

O desenho (§9) define `search_memories` com fusão RRF de um ramo vetorial e um textual, e `similar` (§7) com o limiar `memory.similar_threshold` (0,85), uma similaridade em [0, 1]. Na 004 ainda não há embeddings, e o BM25 não é normalizado.

## Decisão

- **Contrato final já na 004.** `search_memories { query, channels?, categories?, limit?, include_archived?, exclude_ids?, fields? }` responde `{ results: [{ rank, score, memory }] }`. `score = 1 / (60 + rank)` é o RRF só do ramo textual, para a escala não mudar quando o ramo vetorial chegar (007); `rrf_k` e os pesos viram configuração na 007.
- **Consulta.** A consulta vira termos (letras e dígitos, sem acento e minúsculos, até 32), cada um entre aspas e unidos por `OR` no FTS5, ordenados por `bm25(memory_fts, 2.0, 1.0)` (título pesa o dobro). Isso evita erro de sintaxe do FTS5 com texto livre. `limit` padrão 10, máximo 50.
- **Alcance.** `Purpose::Search`: canais `inject` e `search`, mais `manage` e arquivados citados pelo slug exato. Padrão `active` e `permanent`; `include_archived` acrescenta as arquivadas.
- **Contadores.** Cada resultado conta uma recuperação; `get_memory` e `list_memories` não contam.
- **`similar` (Q-003).** Os 10 melhores candidatos do FTS no mesmo canal (ativos e fixados, sem a própria memória e sem texto idêntico) recebem a nota de Jaccard dos termos; entram os que alcançam `memory.similar_threshold` (configurável de 0 a 1), até três, da maior nota para a menor. A partir da 007, vale o cosseno quando houver vetor.

## Consequências

- Com Jaccard, 0,85 só pega textos quase idênticos; quem quiser mais sugestões baixa `memory.similar_threshold`.
- Sem stemming: "fila" não casa com "filas". O ramo vetorial da 007 cobre a semântica.
