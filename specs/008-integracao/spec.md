# Spec - integração

**Criado em**: 2026-10-30
**Atualizado em**: 2026-10-31 (planejamento revisado sobre a 0.7.0)
**Status**: Implementado (0.8.0)
**Versão alvo**: 0.8.0 — **marco: migração de consumidor e gestor**
**Input**: Desenho §6 (eventos e retenção), §12 (`webhooks`, `retention`), §13 (`import_memories`, `get_usage`, `update_app`), §17 (notificações), §19 (integração), §20 (observabilidade), §21 (servidor de webhook de teste).

## Objetivo

O sistema gestor monta a tela completa em tempo real (webhooks), os consumidores trazem as memórias que já existem (`import_memories`) e o serviço fica observável em produção (`/metrics`, `get_status` completo, `get_usage`), com as tabelas de log limitadas pela retenção.

## Estado de partida (0.7.0)

- Toda mutação já grava auditoria e idempotência na mesma transação (`src/store/sqlite/journal.rs`); nenhuma enfileira evento.
- A fila `jobs` tem runners `redact` e `embed` (`src/core/jobs/runner.rs`) com lease, backoff e `list_jobs`/`retry_job`.
- `apps` não tem `webhook_url` nem segredo; `update_app` só altera `name`, `description` e `disabled`.
- `usage_log` recebe `get_context` (entregue, não `304`), `search_memories` e `get_hints`; não há leitura agregada.
- `idempotency` já é limpa a cada hora (24 h, `src/core/background.rs`); `usage_log`, `llm_calls`, `audit_log` e `jobs` crescem sem limite.
- `get_status` tem versão, banco (migrações), backup (`litestream ltx` a cada chamada), contagens, fila, Redator (últimos resultados) e embeddings; não tem uptime, tamanho do banco, idade numérica da réplica, custo do Redator nem webhooks.
- Não existe `/metrics` nem dependência `metrics`.
- `limits.max_batch` existe no catálogo e não é usado.

## Escopo

- Webhooks por app: URL, segredo cifrado (só de escrita), eventos, assinatura HMAC, destinatários, retentativas, job `webhook`.
- Enfileiramento de evento em toda mutação de memória, canal, assinatura e configuração, inclusive as feitas pelo Redator.
- `import_memories`.
- `/metrics` com as métricas do §20, protegido por token `admin`·`read`.
- `get_status` completo e `get_usage`.
- Retenção de `usage_log`, `llm_calls`, `audit_log`, `jobs` concluídos e `idempotency`.
- Guias `integracao-consumidor` (final), `integracao-gestor` (novo, admin) e `operacao` (final); skill.

## Requisitos

| ID | Requisito |
|----|-----------|
| FR-001 | Tipos de evento: `memory.created` (`create_memory`, `import_memories`), `memory.updated` (`update_memory`, `restore_memory`, `pin_memory`, `unpin_memory`, `move_memory`, `revert_memory` e as revisões do Redator: título e texto), `memory.archived`, `memory.deleted`, `channel.updated` (`create_channel`, `update_channel`, `archive_channel`, `restore_channel`, `delete_channel`), `subscription.updated` (`subscribe`, `update_subscription`, `unsubscribe`), `settings.updated` (`update_settings`, `update_channel_settings`, `set_secret`). |
| FR-002 | Payload `{ id, type, channel, channel_version, memory_ids, at }` sem textos, títulos nem valores de configuração; `channel` é o slug. `settings.updated` global usa `channel: null` e `channel_version: null`; por canal, o canal e a versão dele. Campos extras por tipo conforme Q-004. |
| FR-003 | Destinatários, calculados no enfileiramento: apps ativos com `webhook_url` que tenham um agente ativo com assinatura que alcança o canal (qualquer modo) ou um token `admin` ativo (não revogado, não expirado). `subscription.updated` vai ao app do agente da assinatura e aos apps admin; `settings.updated` global só aos apps admin. Sem destinatário, nenhum job. |
| FR-004 | Um job `webhook` por destinatário, criado na mesma transação da mutação (desenho §6); a resposta idempotente repetida não cria jobs. `webhooks.enabled = false` não enfileira. |
| FR-005 | Entrega: `POST <webhook_url>` com `Content-Type: application/json`, `User-Agent: engramind/<versão>`, `Engramind-Event-Id`, `Engramind-Event-Type` e `Engramind-Signature: t=<unix>,v1=<hex(HMAC-SHA256(segredo, t + "." + corpo))>`; sem seguir redirecionamentos; 2xx = entregue. URL e segredo são lidos na entrega (rotação vale para as tentativas seguintes). |
| FR-006 | Falha (rede, timeout `webhooks.timeout_ms`, não-2xx) → retentativa com backoff (Q-006) até `webhooks.max_attempts`; depois `failed`, visível em `list_jobs` e `retry_job`. Mesmo `id` e mesmo corpo em todas as tentativas. App removido, desativado ou sem URL na entrega → `done` com `outcome = skipped`. |
| FR-007 | `move_memory` gera um `memory.updated` por canal envolvido (origem e destino), cada um com a `channel_version` do próprio canal. |
| FR-008 | `update_app` (e `create_app`, Q-001) aceita `webhook_url` (http/https, sem credenciais na URL, até 2048 caracteres; `null` remove) e `webhook_secret` (só de escrita, 32 a 256 caracteres, cifrado com `SECRETS_KEY`; `null` remove). App lido mostra `webhook: { url, secret: { configured, updated_at } }`; auditoria nunca tem o segredo. URL sem segredo é recusada (`VALIDATION_ERROR`). |
| FR-009 | `import_memories { channel, memories[] }` (admin·write, até `limits.max_batch`) exige `Idempotency-Key` (`400 IDEMPOTENCY_KEY_REQUIRED` nas três vias), valida tudo antes de gravar (tudo ou nada), preserva `created_at`, autor (`author: { app, agent? }`, Q-003) e `origin`, ignora duplicatas exatas (no canal e dentro do lote) e as reporta, cria a revisão 1, enfileira `redact`/`embed` e um único `memory.created` com todos os ids criados, numa só transação. Acima do limite → `400 BATCH_TOO_LARGE`. |
| FR-010 | `/metrics` exige token `admin` com `read`; `401`/`403` como nas demais rotas. Formato de exposição do Prometheus. |
| FR-011 | Métricas: requisições e latência por rota e operação (inclusive cada ferramenta MCP), `304` do contexto, fila por tipo e estado, falhas definitivas por tipo, chamadas/tokens/custo do LLM por finalidade, duração e falhas dos embeddings, entregas de webhook por resultado, idade da réplica do Litestream, falhas de autenticação, tamanho do banco, prontidão e versão. Sem slug de app/agente, id de memória ou URL como rótulo. |
| FR-012 | `get_status` completo: versão, uptime, banco (estado, migrações, tamanho em bytes), backup (ligado, estado, idade da réplica em segundos, último instante, quando foi medido), fila por tipo e estado, Redator (configuração, últimos resultados, chamadas, falhas e custo em 24 h), embeddings (como na 0.7.0), webhooks (ligado, apps com webhook, pendentes, entregues e falhas em 24 h) e retenção (última execução e linhas removidas). |
| FR-013 | `get_usage { since?, until?, group_by? }` (admin·read) agrega `usage_log` por `day` (padrão), `agent`, `operation` ou `channel`: requisições, memórias entregues e tokens renderizados (Q-010). |
| FR-014 | Retenção (Q-007, Q-008): `usage_log` (`retention.usage_days`), `llm_calls` (`retention.llm_calls_days`), `audit_log` (`retention.audit_days`, `null` = sempre), jobs `done`/`failed` (`retention.jobs_days`) e `idempotency` (24 h, já existente); remoção em lotes, sem segurar o escritor; resultado em `get_status` e log `INFO` só quando remove algo. |
| FR-015 | Guias: `integracao-gestor` (novo, admin: webhooks, verificação da assinatura, tipos e payloads, importação, tela), `integracao-consumidor` (final: `304`/webhook por turno, importação) e `operacao` (final: `/metrics`, `get_status`, `get_usage`, retenção). Todo tipo de evento documentado em `integracao-gestor` (teste de contrato). |

## Fora do escopo

- Consolidação, `run_consolidation` e `mark_verified` (009).
- Tempo real para navegador (é do gestor).
- Eventos de identidades (apps, agentes, tokens): não estão no desenho §17 (Q-014).
- Operação de teste de webhook e reenvio manual além de `retry_job` (Q-015).
- Exportação de memórias.

## Critérios de sucesso

- Servidor de webhook de teste recebe eventos assinados e verificáveis com o segredo, com retentativa após `500`, mesmo `id` em todas as tentativas e os destinatários certos (agente assinante, app sem assinatura, app admin, app desativado, app sem URL).
- Toda operação de escrita do catálogo gera o evento previsto ou está na lista explícita de exceções (teste que percorre `OPERATIONS`).
- Importação de 100 memórias numa chamada, com datas e autoria preservadas, duplicatas reportadas e repetição idempotente sem jobs novos.
- Prometheus lê `/metrics` com Bearer `admin`·`read`; token `agent` recebe `403`.
- Retenção remove só o que passou do prazo e nunca auditoria com `retention.audit_days = null`.
