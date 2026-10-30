# Plan - Redator

**Data**: 2026-10-30
**Referência de spec**: [`spec.md`](spec.md)

## Resumo

Ligar a fila criada na 004 a workers, criar o cofre de segredos e o cliente LLM, e aplicar o Redator com todas as proteções do desenho.

## Contexto técnico

Novas dependências: `aes-gcm`. `reqwest` já existe (CLI).

## Áreas afetadas

**Core**: `src/core/jobs/` (runner, backoff), `src/core/secrets.rs`, `src/core/redator/` (prompt, schema, apply), `src/providers/llm/` (openai_compatible, anthropic, gemini).
**Adaptadores**: REST, MCP e CLI das operações novas.
**Dados**: `migrations/0006_redator.sql`, `src/store/sqlite/jobs.rs`, `secrets.rs`, `llm_calls.rs`.
**Docs**: guias `configuracoes` (Redator e segredos) e `operacao` (fila), `aicontext/redator.md`, `aicontext/jobs.md`.

## Estratégia de implementação

1. Resolver Q-001 e Q-002.
2. Fila: captura, lease, backoff, superados; `list_jobs`/`retry_job`.
3. Cofre de segredos e `set_secret`.
4. Cliente LLM e os três adaptadores com timeout e `fallback_models`.
5. Redator: prompt com delimitadores, validação, aplicação (título, revisão `system/redator`), `llm_calls`.
6. `test_redator`; `get_status` com fila e Redator.
7. Três vias, guias, versão 0.6.0.

## Estratégia de testes

**Unit**: backoff, prompt (texto só entre delimitadores), validação da resposta, cifra/decifra e nonce único.
**Integração**: servidor HTTP local roteirizado para cada adaptador (sucesso, timeout, 429, 5xx, JSON inválido, título longo), fallback de modelo, lease expirado, job superado, backlog da 004 processado, segredo nunca em log (captura de eventos).

## Riscos

| Risco | Mitigação |
|-------|-----------|
| Provedores com saída estruturada diferente | Validação sempre no serviço, independente do modo do provedor. |
| Fila crescer sem limite com o Redator fora do ar | Backoff com teto e `failed` após `max_attempts`; `get_status` mostra o tamanho. |
