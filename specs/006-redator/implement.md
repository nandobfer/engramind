# Implement - Redator

**Data**: 2026-10-31

## Ordem real

1. Planejamento: Q-001 e Q-002 resolvidas pelas recomendações; Q-003 a Q-012 abertas na leitura do código e aceitas pela usuária com as recomendações, exceto a Q-007, resolvida ao pé da letra do desenho (o serviço não é usado antes do fim das etapas). Q-013 aberta durante a implementação (`clarify.md`).
2. Dependência `aes-gcm` 0.10 (decisão 0015); `SecretKey::new`; `Core::new` recebe a `SECRETS_KEY`.
3. Migração `0006_redator.sql`: `secrets`, `llm_calls` (índice `at`), `jobs.outcome` e o índice `(kind, state, run_after)`.
4. Tipos no `core::model` (`Job`, `JobState`, `JobFinish`, `JobFilter`, `JobCount`, `SecretView`, `StoredSecret`, `NewLlmCall`, `LlmCall`, `LlmCallFilter`, `LlmActivity`, `Redaction`) e os métodos novos do `Store`; `src/store/sqlite/jobs.rs` (fila, cofre, `llm_calls`) e `apply_redaction`/`set_title_fallback` em `memories.rs`.
5. Configurações: tipos `text`, `url`, `list` e `secret`, padrão `null`, as 12 chaves `redator.*`, leitores do `SettingsSnapshot` (`in_channel`, `with` para propostas), segredos em `get_settings` e `SETTING_IS_SECRET`.
6. `core::secrets` (`Vault`, `set_secret`, `Core::secret`).
7. `src/providers/llm/` (trait, `Provider`, `Endpoint`, três adaptadores, classificação de erros).
8. `core::redator` (configuração efetiva, prompt, validação, backoff, guarda, `run_redact`, `test_redator`) e `core::jobs` (runner, `drain`, `spawn_workers`, `list_jobs`, `retry_job`, `list_llm_calls`); `serve` sobe os runners e os aborta no fim.
9. `get_status` com `queue` e `redator`.
10. Catálogo (5 operações, tag `Jobs`), REST, OpenAPI, MCP, CLI (`secrets set`, `settings test-redator`, `jobs list|retry`, `llm-calls list`) e os 7 códigos de erro.
11. Testes e documentação.

## Mudanças além do contrato

- `jobs.outcome` e o índice `(kind, state, run_after)` (D-005).
- Superado pelo `redact` mais novo e pelo `content_hash`, não pela `version` (Q-013).
- `JobHandler` recebe o `Core` e declara o próprio lease; `JobOutcome::Done` carrega o `outcome`.
- `MeaningGuard` trocável (`Core::set_meaning_guard`) para os testes do texto revisado.
- `SETTING_IS_SECRET` (Q-009); `set_secret --delete` na CLI; `test_redator --settings <json>`.
- Nome da chave como dado associado da cifra.
- OpenAI-compatível repete sem `response_format` quando o servidor recusa o campo (D-002).
- `test_redator` aceita `Idempotency-Key` (Q-011).

## Comandos de teste

```bash
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
UPDATE_SNAPSHOTS=1 cargo nextest run -E 'test(/contract::/)'
cargo nextest run -E 'test(/redator::|logging::|matrix::|contract::/)'
cargo nextest run --locked
```

## Rascunho de release notes

0.6.0 liga a fila de jobs ao Redator: cada memória ganha um título de uma linha depois da escrita, com o LLM configurado pela API (OpenAI-compatível, Anthropic ou Gemini, com modelos de reserva) e a chave guardada cifrada. A revisão do texto já existe, mas só é aplicada com a guarda de sentido, que chega com os embeddings na 0.7.0. Operadores acompanham a fila (`list_jobs`, `retry_job`), as chamadas ao LLM (`list_llm_calls`) e o Redator em `get_status`, e testam a configuração com `test_redator`.
