# Clarify - memórias

**Atualizado em**: 2026-10-30

## Perguntas

| ID | Tópico | Pergunta | Status | Resolução |
|----|--------|----------|--------|-----------|
| Q-001 | Fila antes dos workers | Enfileirar `redact`/`embed` (e webhooks) antes de existirem workers? | aberta | Recomendação: enfileirar `redact` e `embed` desde já (as memórias criadas antes da 006/007 serão processadas quando os workers chegarem); webhooks só a partir da 008, para não disparar eventos antigos. |
| Q-002 | Duplicata na edição | O desenho diz que a edição "refaz os passos 3–5"; a duplicata exata numa edição devolve a outra memória? | aberta | Recomendação: edição que resulta em texto idêntico a outra memória do canal → `409 MEMORY_DUPLICATE` com `details.existing_id`, sem alterar nada. |
| Q-003 | Similaridade sem vetor | `memory.similar_threshold` (0,85) é uma similaridade em [0,1], mas BM25 não é normalizado. | aberta | Recomendação: até a 007, candidatos pelo FTS (10 melhores) e nota = Jaccard dos termos normalizados; a partir da 007, cosseno quando houver vetor. |
| Q-004 | Remover categoria em uso | `update_settings` pode retirar uma categoria usada por memórias? | aberta | Recomendação: não; `409 CATEGORY_IN_USE` com a contagem. Renomear `label` e mudar `priority` são livres. |

## Decisões

| ID | Decisão | Fonte | Impacto |
|----|---------|-------|---------|
| D-001 | `content_hash` = SHA-256 do texto em NFC, sem espaços nas pontas, com espaços internos colapsados e minúsculo. | desenho §7 | Nova dependência `unicode-normalization`. |
| D-002 | `memory_revisions.memory_id` sem chave estrangeira, para a última revisão sobreviver ao `delete`. | desenho §6 | Limpeza de revisões órfãs não existe; é intencional. |
| D-003 | Autoria em `author_app_id`/`author_agent_id` sem chave estrangeira (ver 002 D-006). Revisões do Redator usam `app_id = 'system'`, `agent_id = 'redator'`. | desenho §7 | — |
| D-004 | Contadores acumulados em `Mutex<HashMap>` e gravados a cada 5 s pelo escritor, sem mudar `version` nem a ordem do contexto. | desenho §6, §8 | — |
| D-005 | Status iniciais aceitos em `create_memory`: `active` (padrão) e `permanent`. | desenho §7 | — |

## Bloqueios

Q-001 a Q-004 antes da migração `0004`.
