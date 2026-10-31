# Report - integração

**Data**: 2026-10-31 · **Versão**: 0.8.0

## Entrega

FR-001 a FR-015 implementados (detalhes em `implement.md`). Q-001 a Q-015 resolvidas (`clarify.md`); decisões 0022 a 0024.

## Verificação executada

| Verificação | Resultado |
|---|---|
| `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings` | ok |
| `cargo nextest run --locked` | 292 testes, todos passam |
| Webhook assinado (critério de sucesso) | Servidor de teste confere `Engramind-Signature` com o segredo (e recusa outro); `Content-Type`, `User-Agent`, `Engramind-Event-Id/Type`; corpo sem texto; mesmo evento para os destinatários |
| Destinatários (§19) | `app-a` (assinatura por prefixo) e o gestor admin recebem; `app-b` só no canal dele; `app-c` sem webhook não; `settings.updated` global só ao admin; app desativado e sem URL → `skipped`; `webhooks.enabled = false` não enfileira |
| Retentativa | `500` → `queued` com `last_error = "http 500"` (sem URL), nada antes de 10 s, `503` final → `failed`, `retry_job` reenvia; mesmo `id` e corpo nas 3 tentativas |
| Eventos específicos | `move_memory` com dois eventos e as versões de cada canal; repetição idempotente sem jobs; `subscription.updated` com `subscription`/`agent`/`target` também no `unsubscribe`; título e texto do Redator → `memory.updated` |
| Cobertura de eventos | Toda operação de escrita do catálogo enfileira evento, fora as 14 exceções explícitas |
| Importação (critério de sucesso) | 100 itens: 98 criados com `created_at`, autor e origem preservados, revisão `import`, `redact` + `embed` por memória, auditoria sem texto; duplicata do canal e do lote reportadas; repetição idempotente sem jobs; tudo ou nada com `details.index`; `BATCH_TOO_LARGE`; `IDEMPOTENCY_KEY_REQUIRED` no REST e no MCP; CLI com JSONL e chave derivada |
| `/metrics` (critério de sucesso) | `401` sem token, `403` com `agent`, `200` com admin; `operation="get_context",status="304"`, `route="/mcp",operation="whoami"`, falhas de autenticação, fila, versão e tamanho do banco; sem token nem slugs |
| `get_status` e `get_usage` | uptime, tamanho, backup `skip` com `checked_at`, Redator 24 h, webhooks, retenção; os quatro agrupamentos e as validações |
| Retenção (critério de sucesso) | Remove só o que passou do prazo; `audit_days = null` preserva a auditoria; `queued` preservado; `retention_tick` não roda duas vezes na mesma hora |
| Matriz e contrato | 65 operações nas três vias; OpenAPI regenerado (`get_metrics` como rota técnica); tag `Integration`; tipos de evento no guia `integracao-gestor`; erros e configurações documentados |

## Pendências

- Scrape real do Prometheus em produção depende de configurar o job com um token `admin`·`read`.
- Sem eventos de identidades e sem `test_webhook` (fora do escopo, Q-014 e Q-015).
