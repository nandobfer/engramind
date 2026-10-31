# Clarify - integração

**Atualizado em**: 2026-10-31

## Perguntas

| ID | Tópico | Pergunta | Status | Resolução |
|----|--------|----------|--------|--------------|
| Q-001 | Segredo do webhook | Quem gera o segredo e por qual operação ele entra? | resolvida | O admin informa `webhook_secret` (32 a 256 caracteres) em `create_app` ou `update_app`, só de escrita, cifrado em `apps.webhook_secret_enc` com o `Vault` (AAD `webhook:<app_id>`), leitura `{ configured, updated_at }` — o mesmo padrão de `set_secret`. CLI: `--webhook-secret-stdin`. Alternativa descartada: o serviço gerar e mostrar uma vez (exigiria operação de rotação). |
| Q-002 | Proteção do `/metrics` | Token `admin`·`read` ou rede interna? | resolvida | Sempre `admin`·`read`, sem variável para abrir; o Prometheus usa `authorization.credentials`. Diverge do sap-middleman (lá é público) porque o desenho §20 pede token. |
| Q-003 | Autor na importação | O autor preservado precisa existir como app/agente? | resolvida | Sim: `author: { app, agent? }` (id ou slug) precisa existir, mesmo desativado (`APP_NOT_FOUND`/`AGENT_NOT_FOUND`, com `details.index`); omitido, o autor é o token que importa. Quem importa de outro sistema cria antes um app que representa a origem. |
| Q-004 | Campos extras no payload | O payload fixo não diz o que mudou em `subscription.updated` (o alvo pode ser um prefixo) nem em `settings.updated`. | resolvida | Acrescentar só identificadores, nunca valores: `subscription.updated` leva `subscription` (id), `agent` (id) e `target` (canal ou prefixo com `*`); `channel` é o slug quando o alvo é exato, senão `null`. `settings.updated` leva `keys` (as chaves alteradas, inclusive o nome do segredo de `set_secret`). Campos ausentes nos outros tipos. |
| Q-005 | Onde calcular os destinatários | No `Store` (SQL) ou no `core`? | resolvida | No `core`, com `core::access::subscription_pattern` (a mesma regra do acesso efetivo, sem duplicá-la em SQL), a partir da rede em cache mais um diretório de webhooks (`apps` com URL, agente → app, apps com token `admin` ativo e a expiração) cacheado na mesma geração. O `core` passa `Vec<EventDraft>` com os destinatários no `WriteContext`; o `Store` grava os jobs na transação, com a `channel_version` lida depois do incremento. Exige `network_changed()` em `issue_token`, `update_token`, `revoke_token` e `update_app`. |
| Q-006 | Backoff do webhook | O backoff do Redator (2 s, 4 s, 8 s, 16 s, teto 10 min) esgota 5 tentativas em ~30 s, curto para uma queda do gestor. | resolvida | Backoff próprio: `10 s × 4^(n-1)`, teto de 1 h (10 s, 40 s, 160 s, 640 s ≈ 14 min até a 5ª), constante documentada e sem chave nova. Lease = `webhooks.timeout_ms + 30 s`; concorrência 4. |
| Q-007 | Agendamento da retenção | O D-003 antigo pedia um job diário na fila, mas a fila não tem agendamento e a limpeza de `idempotency` já roda em `background.rs`. | resolvida | Rodar a retenção na tarefa periódica existente, a cada hora, com o instante da última execução em `schema_meta['retention.last_run']` (só roda se passou 1 h, sobrevive a reinício); remoção em lotes de 5.000 linhas por transação. Substitui o D-003 antigo. |
| Q-008 | Retenção dos jobs | Cada mutação passa a criar um job por destinatário; `jobs` cresce sem limite e o desenho não cobre. | resolvida | Chave nova `retention.jobs_days` (padrão 30) para jobs `done` e `failed` com `finished_at` mais antigo; `queued`/`running` nunca. Registra a mudança no desenho §6 e §12. |
| Q-009 | Idade da réplica | `litestream ltx` lista o S3 a cada `get_status`; num scrape de 15 s isso pesa. | resolvida | Amostrador em `background.rs` a cada 60 s guarda o último resultado (estado, idade, instante da medição); `get_status`, `/metrics` e o gauge leem o cache. `doctor` continua medindo na hora. |
| Q-010 | Forma do `get_usage` | Intervalo, agrupamento e paginação. | resolvida | `since` padrão = agora − 7 dias, `until` padrão = agora, intervalo máximo de 366 dias (`VALIDATION_ERROR`); `group_by` = `day` (padrão, UTC), `agent` (`app/agente`; token admin sem agente agrupa como `app`), `operation` ou `channel` (via `json_each`, uma memória conta no canal dela; requisições contam uma vez por canal). Linhas `{ key, label, requests, memories, rendered_tokens }`, ordenadas por `key`, sem paginação (o agrupamento já limita). Inclui o total. |
| Q-011 | Regras da importação | Status aceitos, datas, Redator e `similar`. | resolvida | `status` `active` (padrão) ou `permanent`; `created_at` RFC 3339 não futuro (padrão: agora); `updated_at` e a revisão 1 com o mesmo instante; auditoria `memory.import` com o instante real e a lista de ids (sem textos). `similar` não é calculado. `redact` e `embed` enfileirados como na criação (o Redator mantém o sentido pela guarda). Canal arquivado → `409 CHANNEL_ARCHIVED`. Resposta `201 { created: [{ index, id }], duplicates: [{ index, existing_id }] }`. |
| Q-012 | Chave da importação na CLI | Exigir `--idempotency-key` na mão? | resolvida | A CLI usa `--idempotency-key` quando informado; senão deriva `import-<sha256(canal + conteúdo do arquivo)>` (32 hex), para que rodar o mesmo arquivo de novo seja seguro. REST e MCP exigem a chave (`idempotency_key` obrigatório no MCP). Arquivo JSON (lista) ou JSONL. |
| Q-013 | Rótulo de operação nas métricas | O middleware só conhece a rota (`/mcp` é uma rota só). | resolvida | Métricas HTTP com os nomes do sap-middleman (`http_requests_total{method,route,status,operation}`, `http_request_duration_seconds{method,route,operation}`); o `operation` vem de um slot por requisição que o middleware cria e `Core::authorize` preenche (o mesmo ponto que já grava o campo `operation` do span), vazio quando não há operação. Métricas de domínio com prefixo `engramind_`. A taxa de `304` sai de `http_requests_total{operation="get_context",status="304"}`, sem métrica própria. Spike T003 confirma que o slot chega às ferramentas MCP. |
| Q-014 | Eventos de identidades | `delete_agent`/`delete_app` removem assinaturas em cascata; desativar muda o alcance. | resolvida | Sem eventos de identidade nesta etapa (o desenho §17 não lista); o gestor é quem dispara essas mudanças. Fica registrado em `integracao-gestor`. |
| Q-015 | Teste de webhook | Operação `test_webhook` para o gestor validar a URL? | resolvida | Fora do escopo; `list_jobs --kind webhook` e `retry_job` cobrem o diagnóstico. |

## Decisões

| ID | Decisão | Fonte | Impacto |
|----|---------|-------|---------|
| D-001 | Evento = um job `webhook` por destinatário, criado na transação da mutação; o payload é o mesmo em todos e o `id` (`evt_…`) é o do evento. | desenho §6, §17 | Entrega independente por app; ordem não garantida entre retentativas — o consumidor usa `channel_version` para descartar eventos velhos (documentado). |
| D-002 | `/metrics` fica fora de `/v1` e de `OPERATIONS` (não é ferramenta MCP nem comando da CLI); autoriza com uma verificação própria do `core` (`admin` + `read`) e é documentado no OpenAPI na tag `Operations` (`get_metrics`), com Bearer. | desenho §20 | Teste de contrato aceita a rota técnica. |
| D-003 | Recorder Prometheus instalado uma vez (`OnceLock`, como `src/metrics.rs` do sap-middleman), `metrics` 0.24 e `metrics-exporter-prometheus` 0.18.3; gauges de fila, banco e backup preenchidos no scrape (consulta agregada indexada) ou pelo amostrador. | sap-middleman | Duas dependências novas (decisão 0022). |
| D-004 | O job `webhook` registra em `last_error` só o status HTTP ou a classe do erro (`timeout`, `connect`, `tls`), nunca a URL, o corpo ou cabeçalhos; logs mostram o host. | `AGENTS.md` (segurança) | URL com token na query não vaza. |
| D-005 | Revisões do Redator (título e texto) geram `memory.updated` na transação de `apply_redaction`; embeddings não geram evento (não mudam o que o contexto mostra). | FR-001 | O gestor reflete o título final. |
| D-006 | Nova tag `Integration` (admin) para `import_memories`, coberta pelo guia novo `integracao-gestor`; `get_usage` entra na tag `Status` (guia `operacao`). | contrato §15 | `ADMIN_TAGS` ganha `Integration`. |

## Mudanças no desenho

1. **§6**: `apps.webhook_secret_updated_at`; índice `audit_log (at)`; retenção de `jobs` (Q-008).
2. **§12**: `retention.jobs_days` (Q-008).
3. **§17**: campos extras por tipo (Q-004), ordem não garantida e `channel_version` como desempate (D-001), `skipped`, backoff (Q-006).
4. **§20**: nomes das métricas e Bearer obrigatório (Q-002, Q-013).

## Confirmações

A usuária aprovou todas as recomendações de Q-001 a Q-015 e as mudanças de desenho (2026-10-31).

## Bloqueios

Nenhum.
