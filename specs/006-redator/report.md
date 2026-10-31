# Report - Redator

**Data**: 2026-10-31 · **Versão**: 0.6.0

## Entrega

FR-001 a FR-015 implementados (detalhes em `implement.md`). Q-001 a Q-013 resolvidas (`clarify.md`).

## Verificação executada

| Verificação | Resultado |
|---|---|
| `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings` | ok |
| `cargo nextest run --locked` | 237 testes, todos passam |
| Título e texto (critério de sucesso) | Backlog da 004 recebe título com `title_state = ready`, `version` da memória igual, `version` do canal e do contexto novas, auditoria `memory.redact` de `system/redator`; com guarda aprovando, revisão 2 `system/redator` (`reason = redact`), `embed` reenfileirado e nenhum `redact` novo |
| Guarda e limites | Guarda reprovando, texto duplicado no canal e `redator.rewrite_text = false` por canal (esquema só com `title`, `redator.instructions` no fim do prompt) mantêm o texto |
| Adaptadores | OpenAI-compatível (`Authorization`, `json_schema`, uso e `cost`), Anthropic (`x-api-key`, `anthropic-version`, tool forçada) e Gemini (`x-goog-api-key`, `responseSchema` em maiúsculas) contra o LLM roteirizado |
| Falhas (critério de sucesso) | `503` → `429` → sucesso pelos `fallback_models` em ordem; `500` reagenda com backoff de 2 s; título longo conta como falha; esgotadas as tentativas, `failed` e `fallback`; `retry_job` recoloca e o título chega; `409 JOB_NOT_RETRYABLE`, `404 JOB_NOT_FOUND`; tempo limite; desligado e sem configuração viram `fallback` sem chamada |
| Superados | Edição do texto supera o job antigo sem chamar o LLM; fixar não supera; memória excluída supera |
| Lease | Lease vivo bloqueia outra captura; vencido, o job volta e o executor antigo não grava o fim |
| Runner em segundo plano | Título aplicado logo depois da escrita, acordado pelo commit |
| Segredos (critério de sucesso) | Cifrado com `v1` e sem o valor em claro no banco; `get_settings` com `secret`/`configured`; `SETTING_IS_SECRET`, `SECRET_UNKNOWN`; `null` apaga; auditoria e idempotência sem o valor; MCP e CLI (entrada padrão) sem eco |
| `test_redator` | Proposta com chave em claro não grava nada além de `llm_calls`; idempotência sem nova chamada; `PROVIDER_NOT_CONFIGURED`, `PROVIDER_FAILED`, `PROVIDER_TIMEOUT`, `SETTING_UNKNOWN` |
| Status | `queue` e `redator` antes e depois de configurar, sem a chave |
| Logs | `redator.fallback` em `WARN`, `redator.completed` em `INFO` com provedor, modelo e duração; sem chave, texto ou prompt |
| Matriz tipo × escopo × operação | 8 tokens × 59 operações em REST, MCP e CLI |
| Contrato §15 | OpenAPI regenerado; argumentos MCP; códigos de erro, configurações (padrão `—`) e comandos dos guias documentados |

## Pendências

- PR, CI (lint, build, 4 shards, compose, Coverage) e deploy na `main` ainda não executados.
- Guarda de sentido real e o runner do `embed` na 007; até lá o texto revisado nunca é aplicado em produção.
- Retenção de `llm_calls` (90 dias) na 008.
- Sem chamada a provedor real: os adaptadores seguem a documentação pública e só foram exercitados contra o servidor roteirizado.
