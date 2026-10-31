# 0016 — Fila de jobs e aplicação do Redator

**Data**: 2026-10-31 · **Etapa**: 006 (0.6.0) · **Status**: aceita

## Contexto

Desde a 004 as escritas enfileiram `redact` e `embed` na tabela `jobs`, sem worker. A 006 liga a fila a workers e aplica o Redator (desenho §6 e §7). Faltava decidir como capturar, quando um job está superado, o que aplicar o título muda na memória e o que fazer sem a guarda de sentido, que depende dos embeddings da 007.

## Decisão

- **Runner por tipo (D-001).** `core::jobs::runner::spawn` sobe um runner por tipo com concorrência fixa no código (`redact`: 2), acordado por `Notify` depois de cada commit que enfileira e, sem aviso, a cada segundo. Só o `redact` tem runner na 006; o `embed` fica `queued` até a 007. `Core::run_redact_jobs(now)` executa os prontos em `now` (testes e diagnóstico).
- **Captura (FR-002).** Um `UPDATE … RETURNING` pega o job mais antigo do tipo que está `queued` com `run_after <= now` ou `running` com `locked_until < now` (lease vencido), marca `running`, soma a tentativa e grava `locked_until`. O lease do `redact` é `timeout_ms × (1 + fallback_models) + 60 s`. O fim só grava se o job ainda estiver `running` (um executor atrasado não sobrescreve outro).
- **Backoff (FR-003).** Falha temporária volta a `queued` em 2 s × 2ⁿ⁻¹ (teto de 10 min) até `redator.max_attempts`; esgotadas, `failed`, `title_state = fallback` e um único `ERROR jobs.failed`, no runner. `retry_job` recoloca só `failed`, com as tentativas zeradas.
- **Superado (FR-004).** Um job `redact` está superado quando a memória não existe mais ou quando há um job `redact` mais novo para a mesma memória (toda mudança de texto, e o `revert`, enfileiram outro). A `version` da memória não serve: fixar, arquivar ou mover a sobem sem mudar o texto, e o título nunca chegaria. Na aplicação, o `Store` confere o `content_hash` lido no início do job (`Conflict("MEMORY_CHANGED")` → superado). `jobs.outcome` (coluna nova) guarda `applied`, `title_only`, `superseded` ou `skipped`.
- **Título (FR-007).** Aplicar o título grava `title` e `title_state = ready`, incrementa `channels.version` e chama `network_changed()` (o título aparece na faixa 2 do contexto, decisão 0013), com auditoria `memory.redact`. Não cria revisão e não muda `memories.version`, para um agente editando não receber `MEMORY_VERSION_CONFLICT` por um título que ele não escreveu.
- **Texto revisado.** Só vale se mudou, não repete outra memória do canal e passa na guarda de sentido. Aplicado: revisão com autor `system/redator` (`app_id = system`, `agent_id = redator`, `reason = redact`), `memories.version + 1`, `embedding_state = pending` e um job `embed`; nunca outro `redact`.
- **Guarda de sentido sem embeddings (Q-001).** A guarda é a interface `MeaningGuard`. Até a 007 vale `NoGuard`, que não mede: o texto revisado nunca é aplicado e só o título entra (falha fechada). Os testes trocam por uma guarda determinística (`Core::set_meaning_guard`) para exercitar o caminho do texto.
- **Sem configuração (FR-009).** Redator desligado ou sem provedor, modelo, URL (OpenAI-compatível) ou chave (Anthropic e Gemini): `title_state = fallback`, `done/skipped`, sem chamada e sem `ERROR`.

## Consequências

- Com o Redator sem configuração, o backlog de títulos vira `fallback`; configurar depois não reprocessa (`retry_job` só cobre `failed`). O serviço ainda não está em uso (as etapas seguem antes do primeiro consumidor).
- A busca por job mais novo usa `json_extract` sobre `payload_json`; com volume alto, um índice por memória será a etapa de escala.
