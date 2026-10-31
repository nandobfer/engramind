# Identidades, tokens e autorização

## Onde vive

- `src/core/principal.rs`: `TokenKind`, `Scope`, `ScopeSet` (invariante: `write`/`delete` ⇒ `read`), `Principal`.
- `src/core/operations.rs`: `OPERATIONS`, o catálogo único (nome base, tag, resumo, tipo, escopo, hints, rota, CLI).
- `src/core/authz.rs`: `allows` e `require` (`403 SCOPE_REQUIRED` com `required_kind`/`required_scope`).
- `src/core/auth.rs` e `src/auth/`: Bearer → `Principal`; `TokenService` (HMAC-SHA256 com `TOKEN_PEPPER`) e o middleware `require_auth`.
- `src/core/identities.rs`: apps, agentes e tokens; `src/core/idempotency.rs`: `Core::mutate`; `src/core/bootstrap.rs`; `src/core/usage.rs`: rate limit e `last_used_at`.
- `src/store/sqlite/identities.rs` e `journal.rs`: SQL, auditoria e idempotência na mesma transação.
- Adaptadores: `src/rest/identities.rs`, `src/mcp/tools.rs`, `src/cli/remote.rs`.

## Operações

`whoami` (qualquer token); `list_apps`, `get_app`, `list_agents`, `get_agent`, `list_tokens` (admin·read); `create_app`, `update_app`, `create_agent`, `update_agent`, `issue_token`, `update_token` (admin·write); `delete_app`, `delete_agent`, `revoke_token` (admin·delete). Tags `Authentication` e `Identities`; guia `tokens-e-escopos`.

## Invariantes

- Toda operação começa por `Core::authorize(principal, "<nome>")`; nunca autorize num adaptador.
- Token ausente, malformado, desconhecido, expirado, revogado ou de app/agente desativado → `401 AUTH_INVALID`. O HMAC é conferido sempre.
- O segredo aparece só na resposta de `issue_token`; nunca é persistido, logado, auditado nem gravado na idempotência (a repetição devolve o token sem `secret`).
- Ninguém concede escopos que não tem (`issue_token`, `update_token`).
- Token `agent` exige agente; token `admin` aceita agente do mesmo app.
- `LAST_ADMIN_TOKEN`: nenhuma mutação pode zerar os tokens `admin` ativos com `write` (checado na transação).
- Toda mutação passa por `Core::mutate`: auditoria e idempotência na mesma transação; a repetição é conferida antes de resolver os alvos.
- Exclusão remove em cascata (agentes, tokens); a auditoria não tem chave estrangeira e guarda o estado anterior.
- `create_app`/`update_app` aceitam `webhook_url` (http/https, sem credenciais) e `webhook_secret` (32 a 256, só gravado, cifrado com AAD `webhook:<slug>`); URL sem segredo é recusada; a leitura e a auditoria só mostram `webhook: { url, secret: { configured, updated_at } }`.
- Toda mutação de app, agente ou token chama `network_changed()` (o diretório dos webhooks vive na mesma geração da rede).
- Rate limit e `last_used_at` são em memória (uma instância por banco); `last_used_at` é gravado a cada 10 s e no encerramento.

Desenho: §4, §6 e §13. Decisões: `0007`, `0008`, `0009`.
