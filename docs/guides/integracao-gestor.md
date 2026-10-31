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
- `subscription.updated` vai ao app do agente da assinatura e aos apps admin; `settings.updated` global, só aos apps admin;
- app desativado ou sem URL e segredo não recebe nada.

Com `webhooks.enabled = false`, nenhuma mutação enfileira eventos. Mudanças de apps, agentes e tokens não geram eventos: quem as faz é o próprio gestor.

## Eventos

O corpo nunca tem textos, títulos nem valores de configuração, só identificadores:

```json
{ "id": "evt_3f9e1a7c0b2d4e6f", "type": "memory.created", "channel": "eng/global",
  "channel_version": 42, "memory_ids": ["m_8f2a1c3d"], "at": "2026-10-30T12:00:00.000Z" }
```

| `type` | Quando | Campos extras |
|---|---|---|
| `memory.created` | `create_memory`; `import_memories` (um evento com todos os ids criados) | — |
| `memory.updated` | `update_memory`, `restore_memory`, `pin_memory`, `unpin_memory`, `revert_memory`, `move_memory` (um evento por canal envolvido) e o título ou o texto aplicado pelo Redator | — |
| `memory.archived` | `archive_memory` | — |
| `memory.deleted` | `delete_memory` | — |
| `channel.updated` | `create_channel`, `update_channel`, `archive_channel`, `restore_channel`, `delete_channel` | — |
| `subscription.updated` | `subscribe`, `update_subscription`, `unsubscribe` | `subscription`, `agent` e `target` (canal ou prefixo); `channel` só com alvo exato |
| `settings.updated` | `update_settings` e `set_secret` (`channel: null`); `update_channel_settings` (com o canal) | `keys`: as chaves alteradas |

`channel_version` é a versão do canal depois da mudança (`null` em `settings.updated` global). Uma mutação que não muda nada (arquivar o que já está arquivado) e a repetição de uma chamada idempotente não geram evento.

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
| Memórias | `list_memories`, `search_memories`, `memory_history`, `revert_memory`, `pin_memory`, `archive_memory`, `move_memory`, `delete_memory`, `import_memories` |
| Identidades | `list_apps`, `update_app` (webhook), `list_agents`, `list_tokens`, `issue_token`, `revoke_token` |
| Assinaturas | `list_subscriptions`, `subscribe`, `update_subscription`, `unsubscribe` |
| Configurações | `get_settings`, `update_settings`, `set_secret`, `test_redator`, `test_embedding` |
| Operação | `get_status`, `get_usage`, `list_jobs`, `retry_job`, `list_llm_calls`, `get_audit` |

"Escritas recentes por agente" sai de `list_memories` com `author` e `since`, ou de `get_audit` com `entity=memory`.
