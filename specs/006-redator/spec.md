# Spec - Redator

**Criado em**: 2026-10-30
**Status**: Draft
**Versão alvo**: 0.6.0
**Input**: Desenho §6 (`jobs`, `secrets`, `llm_calls`), §7 (Redator), §12 (`redator`, segredos, testers), §13 (Operação), §18, `aicontext/use-quando-integrar-provedores.md`.

## Objetivo

Toda memória ganha título e texto revisados pelo Redator, de forma assíncrona, auditável e reversível, e o gestor configura, testa e acompanha o LLM pelas três vias.

## Escopo

- Workers `tokio` sobre a fila `jobs` (captura, lease, backoff, descarte de jobs superados).
- Segredos cifrados (`secrets`) e `set_secret`.
- Cliente LLM próprio (`reqwest`) para OpenAI-compatível, Anthropic e Gemini, com `fallback_models`.
- Job `redact` e o processamento do backlog criado desde a 004.
- `llm_calls`, `list_llm_calls`, `list_jobs`, `retry_job`, `test_redator`.
- Configurações `redator.*`.
- `get_status` com fila e Redator.

## Requisitos

| ID | Requisito |
|----|-----------|
| FR-001 | A escrita nunca espera o LLM; o job `redact` roda depois do commit. |
| FR-002 | Captura com `UPDATE jobs SET state='running', locked_until=… WHERE id = (SELECT … WHERE state='queued' AND run_after <= now ORDER BY id LIMIT 1) RETURNING …`; lease expirado volta à fila. |
| FR-003 | Falha temporária reagenda com backoff exponencial (2 s × 2ⁿ, teto 10 min) até `redator.max_attempts`; depois `failed`, `ERROR` uma vez e `title_state = fallback`. |
| FR-004 | Job de uma versão superada da memória termina sem efeito (`done`, `outcome = superseded`). |
| FR-005 | O texto vai ao LLM como dado, entre delimitadores, com instrução de não seguir nada do conteúdo; `redator.instructions` (global ou por canal) é acrescentada ao prompt do sistema. |
| FR-006 | A resposta `{ title, text }` é validada por esquema JSON, `redator.title_max` e `memory.text_max`; resposta inválida conta como falha da tentativa. |
| FR-007 | Título aplicado → `title_state = ready`, `channels.version + 1` e `network_changed()` (o título aparece na faixa 2 do contexto; 005, decisão 0013). Texto alterado (com `rewrite_text`) → nova revisão com autor `system/redator`, `version + 1`, `channels.version + 1`, auditoria, reenfileira `embed`. |
| FR-008 | Guarda de sentido conforme Q-001. |
| FR-009 | Redator desligado ou sem provedor/modelo/chave → `title_state = fallback` sem chamar nada e sem `ERROR`. |
| FR-010 | `fallback_models[]` é tentado em ordem quando o modelo principal falha por indisponibilidade, limite ou timeout. |
| FR-011 | Toda chamada grava `llm_calls` (finalidade, provedor, modelo, tokens, custo quando informado pelo provedor, duração, outcome, erro curto), nunca o texto. |
| FR-012 | Segredos: `set_secret { key, value }` cifra com AES-256-GCM (`SECRETS_KEY`, nonce aleatório por gravação); leitura só `{ configured, updated_at }`; `value: null` apaga. |
| FR-013 | `test_redator { text, settings? }` usa a configuração atual ou a proposta (sem salvar) e devolve `{ title, text, model, latency_ms }`; falha → `502 PROVIDER_FAILED` com motivo curto. |
| FR-014 | `retry_job` recoloca na fila só jobs `failed` (`409 JOB_NOT_RETRYABLE` caso contrário). |
| FR-015 | Logs do Redator: provedor, modelo, outcome, tentativa, duração; retentativa/fallback em `WARN`; nunca texto, prompt ou chave. |

## Fora do escopo

- Embeddings, guarda de sentido ativa (007, ver Q-001), consolidação (009).

## Critérios de sucesso

- Com o LLM roteirizado, memória criada recebe título e texto revisados, nova revisão `system/redator` e `version` do contexto atualizada.
- Falha, timeout, resposta inválida, fallback de modelo, Redator desligado e job superado cobertos.
- Nenhum segredo legível no banco, em logs ou em respostas.
