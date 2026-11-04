# Integração de um sistema gestor

O sistema gestor é a tela de administração da rede: canais, memórias, apps, agentes, tokens, assinaturas, configurações, fila, consumo do Redator, auditoria e status. Toda chamada sai do servidor do gestor com um token `admin` guardado como segredo dele (nunca no navegador: o CORS é fechado). O tempo real vem dos webhooks; a importação traz as memórias que já existem em outro sistema.

## Webhook do app

Cada app pode receber eventos num `webhook_url` próprio. A URL é `http://` ou `https://`, com host e sem credenciais (até 2048 caracteres), e exige um `webhook_secret` de 32 a 256 caracteres. O segredo é cifrado com `SECRETS_KEY` e nunca volta: a leitura do app mostra `webhook: { url, secret: { configured, updated_at } }` (`webhook: null` sem URL). `null` remove a URL ou o segredo; um segredo sozinho fica guardado sem receber eventos.

```bash
openssl rand -base64 48 | engramind apps update gestor --webhook-url https://gestor.interno/engramind/events --webhook-secret-stdin
engramind apps update gestor --webhook-url ""
```

No REST, `PATCH /v1/apps/gestor` (ou `POST /v1/apps` na criação) com `{ "webhook_url": "…", "webhook_secret": "…" }`; no MCP, `update_app` com os mesmos campos. Gerar, rotacionar e guardar o segredo é do gestor; trocar o segredo vale para a próxima tentativa de entrega.

## Quem recebe

Os destinatários são calculados quando a mutação acontece:

- apps com um agente ativo cuja assinatura (exata ou por prefixo, em qualquer modo) alcança o canal afetado;
- apps com um token `admin` ativo (não revogado e não expirado) recebem todos os eventos;
- `subscription.updated` vai ao app do agente da assinatura e aos apps admin; `settings.updated` global, só aos apps admin; `identities.updated` ao próprio app (mesmo que o webhook acabe de ser gravado) e aos apps admin;
- app desativado ou sem URL e segredo não recebe nada.

Com `webhooks.enabled = false`, nenhuma mutação nem leitura enfileira eventos. Mudanças de apps, agentes e tokens não geram eventos: quem as faz é o próprio gestor.

## Eventos

O corpo nunca tem textos, títulos nem valores de configuração, só identificadores:

```json
{ "id": "evt_3f9e1a7c0b2d4e6f", "type": "memory.created", "channel": "eng/global",
  "channel_version": 42, "memory_ids": ["m_8f2a1c3d"], "at": "2026-10-30T12:00:00.000Z" }
```

| `type` | Quando | Campos extras |
|---|---|---|
| `memory.created` | `create_memory`; `import_memories` (um evento com todos os ids criados) | — |
| `memory.updated` | `update_memory`, `restore_memory`, `pin_memory`, `unpin_memory`, `revert_memory`, `move_memory` (um evento por canal envolvido), o título ou o texto aplicado pelo Redator e a memória mantida numa fusão do consolidador | — |
| `memory.archived` | `archive_memory`; a memória fundida pelo consolidador e o arquivamento por desuso (um evento com todos os ids) | — |
| `memory.deleted` | `delete_memory` | — |
| `channel.updated` | `create_channel`, `update_channel`, `archive_channel`, `restore_channel`, `delete_channel` | — |
| `subscription.updated` | `subscribe`, `update_subscription`, `unsubscribe` | `subscription`, `agent` e `target` (canal ou prefixo); `channel` só com alvo exato |
| `settings.updated` | `update_settings` e `set_secret` (`channel: null`); `update_channel_settings` (com o canal) | `keys`: as chaves alteradas |
| `identities.updated` | `update_app_webhook` (`channel: null`) | `app`: o slug do app cujo webhook mudou |
| `memory.consumed` | `get_context` (inclusive o `304` com o bloco em cache), `search_memories`, `get_hints` e `get_memory` (este só com token `agent`) que entregam memórias (um evento por canal) | `consumption`, `operation`, `app`, `agent`; no `get_context`, também `title_ids` e `context_version` (seção Consumo) |

`channel_version` é a versão do canal depois da mudança (`null` em `settings.updated` global). Uma mutação que não muda nada (arquivar o que já está arquivado) e a repetição de uma chamada idempotente não geram evento. A nota de importância e a sinalização de desuso do consolidador e `mark_verified` também não: não mudam a `channel_version` nem o contexto (releia com `list_memories`, por exemplo `stale=true`).

## Consumo

`memory.consumed` avisa que uma leitura entregou memórias a quem chamou. Não é mudança: nenhuma versão muda, `channel_version` é a versão atual do canal (não serve para desempate) e o contexto continua o mesmo, então **não marque o contexto como sujo** ao receber este tipo. Sai um evento por canal com ao menos uma memória entregue, para os mesmos destinatários dos eventos do canal; nenhum app recebe ids de um canal que não alcança.

```json
{ "id": "evt_9c1d…", "type": "memory.consumed", "channel": "eng/global", "channel_version": 42,
  "memory_ids": ["m_8f2a1c3d"], "at": "2026-10-30T12:00:00.000Z",
  "consumption": "cns_4b7e…", "operation": "get_context", "app": "app-a", "agent": "app-a/dev",
  "title_ids": ["m_77aa01ff"], "context_version": "cv_3f9e1a7c0b2d" }
```

- `memory_ids`: no `get_context`, as memórias com texto completo (faixa 1) do canal; em `search_memories`, `get_hints` e `get_memory`, as devolvidas.
- `title_ids` e `context_version` (só no `get_context`): as memórias só com título (faixa 2) do canal e a `version` do bloco entregue. Um canal que só entrou na faixa 2 não gera evento.
- `consumption`: id comum a todos os eventos da mesma chamada (`cns_` + 16 hex), para agrupar uma leitura de vários canais.
- `operation`: a leitura (`get_context`, `search_memories`, `get_hints` ou `get_memory`).
- `get_memory` só emite com token `agent`. Para resolver os ids recebidos (títulos do pulso), use o token `admin` ou `app` do gestor: essa leitura não é consumo e não gera outro `memory.consumed`.
- `app` e `agent`: o slug do app do token e o do agente (`app/agente`); `agent: null` para tokens `app` e `admin`.
- Um `304` do `get_context` emite de novo, com os ids do bloco que ainda está no cache em memória do serviço; depois de um reinício (ou com o bloco fora do cache), o `304` não emite até o próximo `200`.
- Leitura sem memória entregue não emite. Uma falha ao gravar o evento não derruba a leitura (aviso `webhooks.consumed_failed` no log).
- O corpo nunca leva texto, título, `query`, `text` das dicas nem `focus`.

## Entrega e verificação

```http
POST <webhook_url>
Content-Type: application/json
User-Agent: engramind/<versão>
Engramind-Event-Id: evt_3f9e1a7c0b2d4e6f
Engramind-Event-Type: memory.created
Engramind-Signature: t=1793370000,v1=<hex>
```

`v1` é `hex(HMAC-SHA256(segredo, t + "." + corpo))`, com o corpo exato recebido; `t` é o instante Unix da tentativa. Para aceitar um evento:

1. separe `t` e `v1` do cabeçalho;
2. recuse `t` com mais de 5 minutos de diferença do seu relógio;
3. calcule o HMAC do corpo bruto e compare com `v1` em tempo constante;
4. descarte `id` já visto (as retentativas repetem o mesmo `id` e o mesmo corpo);
5. responda `2xx` rápido e processe depois.

Qualquer resposta fora de `2xx`, erro de rede ou tempo acima de `webhooks.timeout_ms` é uma falha: o job volta para a fila com espera de 10 s, 40 s, 160 s, 640 s… (×4, teto de 1 h) até `webhooks.max_attempts`, e então fica `failed` em `list_jobs --kind webhook`, de onde `retry_job` o reenvia. Redirecionamentos não são seguidos. App removido, desativado ou sem URL na hora da entrega termina `done` com `outcome = skipped`.

A ordem não é garantida entre eventos de retentativas diferentes: use `channel_version` para descartar um evento mais velho do que o estado que você já mostra, ou releia o canal (`list_memories`, `get_channel`).

## Importação

`import_memories` traz de uma vez até `limits.max_batch` memórias de outro sistema para um canal ativo, numa transação. Um canal por lista de origem; o autor original precisa existir como app (crie um app que represente a origem) e, opcionalmente, um agente dele.

```json
{ "channel": "eng/global",
  "memories": [
    { "text": "Rode só os testes afetados.", "category": "CONSTRAINTS",
      "created_at": "2026-01-10T12:00:00Z",
      "author": { "app": "legado", "agent": "curador" },
      "origin": { "source": "import", "label": "magic-context#42" } } ] }
```

- `Idempotency-Key` é obrigatória (`400 IDEMPOTENCY_KEY_REQUIRED`): repetir a mesma chave com o mesmo corpo devolve a mesma resposta sem gravar de novo.
- Tudo é validado antes de gravar; um item inválido recusa o lote com `details.index` (`VALIDATION_ERROR`, `CATEGORY_NOT_FOUND`, `APP_NOT_FOUND`, `AGENT_NOT_FOUND`). Acima do limite, `400 BATCH_TOO_LARGE` com `details.max` e `details.received`.
- `created_at` (não futuro; omitido, agora) vale para a memória e para a revisão 1; o autor da revisão é o autor original. `status` aceita `active` (padrão) e `permanent`.
- Com `memory.dedupe_exact`, um texto igual a uma memória do canal, ou a um item anterior do lote, vai para `duplicates` com o `existing_id`; o resto é criado.
- O Redator e os embeddings rodam depois, como numa criação; a auditoria registra `memory.import` com os ids criados.

```bash
engramind memories import --channel eng/global --file memorias.json --json
```

A CLI aceita uma lista JSON, `{ "memories": [...] }` ou JSONL (um item por linha; `--file -` lê da entrada padrão). Sem `--idempotency-key`, ela deriva a chave do canal e do conteúdo do arquivo, então rodar o mesmo arquivo de novo é seguro.

## A tela do gestor

| Área | Operações |
|---|---|
| Canais | `list_channels`, `channel_stats`, `create_channel`, `update_channel`, `archive_channel`, `restore_channel`, `delete_channel` |
| Memórias | `list_memories`, `search_memories`, `memory_history`, `revert_memory`, `pin_memory`, `archive_memory`, `move_memory`, `delete_memory`, `mark_verified`, `import_memories` |
| Consolidação | `run_consolidation` (com `dry_run`), `list_memories` com `stale=true`, `get_status.consolidation` |
| Identidades | `list_apps`, `update_app` (webhook), `list_agents`, `list_tokens`, `issue_token`, `revoke_token` |
| Assinaturas | `list_subscriptions`, `subscribe`, `update_subscription`, `unsubscribe` |
| Configurações | `get_settings`, `update_settings`, `set_secret`, `test_redator`, `test_embedding` |
| Operação | `get_status`, `get_usage`, `list_jobs`, `retry_job`, `list_llm_calls`, `get_audit` |

"Escritas recentes por agente" sai de `list_memories` com `author` e `since`, ou de `get_audit` com `entity=memory`.
