# 0026 — Consolidação: plano puro, fusão, importância pendente e desuso

**Data**: 2026-10-31 · **Etapa**: 009 (0.9.0) · **Status**: aceita

## Contexto

O desenho §10 pede um consolidador opcional por canal (duplicatas, importância, desuso) com `dry_run`, e o §8 exige que a classificação nunca mude a ordem do contexto sem mudar a `version`. A `version` sai da rede em cache (`channels.version`), e o bloco é refeito do banco quando o cache LRU perde a entrada ou o serviço reinicia.

## Decisão

- **Plano puro** (`consolidation::plan`): a partir das memórias `active` do canal, dos vetores do modelo vigente e dos pares recusados, devolve os candidatos de cada tarefa. O `dry_run` é síncrono, sem LLM e sem gravar; o job `consolidate` age só sobre esse plano (Q-004). Limites constantes por canal e execução: 20 pares, 200 classificações em lotes de 20.
- **Execução**: um job por canal, concorrência 1, lease fixo de 30 min (o runner não renova lease); cada ação é uma transação curta com compare-and-set no `content_hash` lido; o LLM roda fora das transações; falha do LLM é contada e o job termina `done`. Resultado na auditoria `consolidation.run` e em `schema_meta['consolidation.last_run']` (Q-005).
- **Fusão** (Q-006): cosseno exato em Rust sobre os vetores do canal (como a 0018), pares gulosos do mais parecido ao menos; o Redator decide com `{ merge, text }`; o texto passa pela guarda de sentido contra cada original (falha fechada). A mais antiga recebe texto, contadores e `redact`/`embed` numa revisão `system/consolidador`; a outra vira `archived` com `merged_into`. Eventos `memory.updated` + `memory.archived` (Q-009). Recusas ficam em `consolidation_pairs` até um dos textos mudar (Q-014).
- **Importância** (D-001, Q-008): a nota vai para `memories.importance_pending`; `promote_importance` a copia para `importance` em toda transação que incrementa `channels.version`. Edição de texto zera as duas. Sem reclassificação periódica.
- **Desuso** (Q-003): coluna `memories.last_injected_at`, avançada no lote de injeções; último uso = maior entre `updated_at`, `last_retrieved_at`, `last_injected_at` e `verified_at`. Sinalizar e limpar não mudam `version` nem geram evento; o arquivamento automático (`consolidation.auto_archive_stale`) grava revisão, auditoria e `memory.archived`.
- **`mark_verified`** (Q-001): any·write com `write` efetivo; grava `verified_at` e auditoria, limpa `stale_candidate`, sem revisão, sem `version` e sem evento.
- As chamadas usam a configuração `redator.*` e `Core::complete_with` (o mesmo laço de modelos e fallback do Redator), com `purpose = consolidation` (Q-012).

## Consequências

- A ordem nova só aparece na próxima mudança real do canal; um canal parado mantém a ordem anterior indefinidamente.
- Toda escrita futura que incrementar `channels.version` fora de `bump_channel` precisa chamar `promote_importance`.
- O custo O(n²) do cosseno por canal é pago só no horário; se não couber, a escala (`vec0`) ganha spec.
