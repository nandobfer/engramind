# Redator

## Onde vive

- `src/core/redator/mod.rs`: `RedatorConfig` (configuração efetiva do canal, `missing`, `ready`, `lease`), `validate`, `backoff`, `Core::run_redact` (job `redact`) e `test_redator`.
- `src/core/redator/prompt.rs` e `prompt.md`: prompt do sistema, marcas aleatórias e esquema da saída.
- `src/core/redator/guard.rs`: `MeaningGuard` e `NoGuard`; a guarda real é a `EmbeddingGuard` de `src/core/embeddings/` (`Core::meaning_guard`: a dos testes, senão a dos embeddings vigentes, senão nenhuma → só o título).
- `src/core/secrets.rs`: `Vault` (AES-256-GCM) e `set_secret`; `Core::secret` decifra na hora do uso.
- `src/providers/llm/`: `LlmClient`, `Provider`, `Endpoint` e os adaptadores `openai_compatible`, `anthropic` e `gemini`.
- `Store::apply_redaction` e `set_title_fallback` em `src/store/sqlite/memories.rs`.

## Regras

- O texto vai ao LLM só na mensagem do usuário, entre `prompt::user(text, boundary)`; nunca no prompt do sistema, em log, em `llm_calls`, em `last_error` ou em erro de API.
- A resposta é sempre validada por `validate` (esquema, `redator.title_max`, `memory.text_min`/`text_max` do canal). Inválida conta como falha da tentativa.
- Superado: memória excluída ou job `redact` mais novo para a mesma memória; na aplicação, `expected_hash` precisa bater com o `content_hash` atual. Nunca use `memories.version` para isso (status e `move` a sobem).
- Título: `title_state = ready`, `channels.version + 1`, `network_changed()`, auditoria `memory.redact`; sem revisão e sem mudar `memories.version`.
- Texto: só com guarda aprovando (`similarity >= redator.min_similarity`), texto diferente e sem duplicata no canal; revisão `system/redator` (`reason = redact`), `version + 1` e job `embed`, nunca outro `redact`. O título aplicado também reenfileira `embed` (com os embeddings ligados; `Redaction.embed`).
- Sem configuração ou desligado: `set_title_fallback` e `Done("skipped")`, sem chamada e sem `ERROR`.
- Toda chamada grava `llm_calls`; `cost_usd` só quando o provedor informa.
- `redator.temperature` (`Ready.temperature` → `LlmRequest.temperature`) vale para Redator e consolidador; `None` omite o campo do corpo nos três adaptadores. Nunca volte a fixar `temperature` no adaptador.
- Recusa 4xx (exceto 408/429) vira `LlmError::Rejected("HTTP <status>: <motivo>")`, com o motivo de `provider_reason` (mensagem de erro do provedor, uma linha, sem a chave, ≤ 160 caracteres); vai para `details.reason`, a `message` e `llm_calls.error`, nunca para logs. Nas chamadas explícitas responde `422 PROVIDER_REJECTED`; `PROVIDER_FAILED` (502) é só indisponibilidade ou resposta inválida (decisão 0027).
- Segredos: só chaves `SettingType::Secret`; `update_settings` as recusa (`SETTING_IS_SECRET`). Nunca registre o valor; tipos que o carregam (`SetSecretBody`, `TestRedatorBody`, as requisições MCP) não derivam `Debug`.
- Logs: `redator.completed` (`INFO`), `redator.retry` e `redator.fallback` (`WARN`), `redator.skipped`, `redator.guard_*` (`DEBUG`); sempre provedor, modelo, outcome, tentativa e duração.

Desenho §7, §12 e §18; decisões 0015, 0016 e 0017; guias `operacao` e `configuracoes`.
