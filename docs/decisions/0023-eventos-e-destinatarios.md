# 0023 — Eventos e destinatários

**Data**: 2026-10-31 · **Etapa**: 008 (0.8.0) · **Status**: aceita

## Contexto

O desenho (§6, §17) pede que toda mutação enfileire o webhook na mesma transação, com destinatários "apps com agente assinando o canal e apps com token `admin`". A regra de alcance (padrão exato ou prefixo, especificidade) já vive em `core::access`, e o `Store` não pode ganhar regra de negócio.

## Decisão

- O `core` monta os `EventDraft` (id `evt_` + 16 hex, tipo, slug do canal, ids, extras e destinatários) antes da escrita e os passa em `WriteContext.events`; o `Store` chama `journal::emit` só nos caminhos que mudaram algo de fato, gravando um job `webhook` por destinatário com `{ app_id, event }` e a `channel_version` lida depois do incremento. Respostas idempotentes repetidas e mutações sem efeito não gravam jobs.
- Destinatários pelo `core` (Q-005), com `subscription_pattern` sobre a rede em cache e um `WebhookDirectory` (apps com URL e segredo, agente → app, tokens `admin` não revogados com a expiração) cacheado na mesma geração da rede. `network_changed()` passa a valer também para mudanças de apps, agentes e tokens.
- A entrega lê URL e segredo na hora (rotação vale para a próxima tentativa); `skipped` para app removido, desativado ou sem URL; backoff próprio `10 s × 4^(n-1)`, teto de 1 h (Q-006), lease `webhooks.timeout_ms + 30 s`, 4 em paralelo; `last_error` só com status ou classe da falha (D-004).
- Segredo do webhook informado pelo admin (Q-001) em `create_app`/`update_app`, 32 a 256 caracteres, cifrado pelo `Vault` com AAD `webhook:<slug>` em `apps.webhook_secret_enc`; leitura `{ configured, updated_at }`. URL sem segredo é recusada.
- Sem eventos de identidade (Q-014).

## Consequências

- Um evento com N destinatários vira N jobs: entrega e retentativa independentes por app; daí a retenção de jobs (decisão 0024).
- A ordem entre eventos não é garantida; o consumidor desempata por `channel_version`.
- O teste `every_write_operation_enqueues_its_event` percorre o catálogo com uma lista explícita de exceções.
