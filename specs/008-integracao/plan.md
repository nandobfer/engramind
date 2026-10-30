# Plan - integração

**Data**: 2026-10-30
**Referência de spec**: [`spec.md`](spec.md)

## Resumo

Eventos e webhooks em todas as mutações, importação, métricas, status completo e retenção.

## Contexto técnico

Novas dependências: `metrics`, `metrics-exporter-prometheus` (versões do sap-middleman).

## Áreas afetadas

**Core**: `src/core/events.rs` (tipos, destinatários), `src/core/webhooks.rs` (job), `src/core/import.rs`, `src/core/status.rs`, `src/core/usage.rs`, `src/core/retention.rs`; todas as mutações passam a gerar evento.
**Adaptadores**: REST, MCP, CLI; `src/metrics.rs` (portado).
**Dados**: `migrations/0008_webhooks.sql`.
**Docs**: guias `integracao-consumidor`, `integracao-gestor`, `operacao`; `aicontext/webhooks.md`.

## Estratégia de implementação

1. Resolver Q-001 a Q-003.
2. Eventos e destinatários; inserir o enfileiramento em todas as mutações existentes (memória, canal, assinatura, configuração).
3. Job `webhook` com assinatura HMAC e retentativa.
4. `import_memories`.
5. Métricas e `/metrics`.
6. `get_status` completo, `get_usage`, retenção.
7. Guias e versão 0.8.0.

## Estratégia de testes

**Integração**: servidor de webhook local (assinatura verificada com o segredo, retentativa após 500, deduplicação por `id`, destinatários corretos para agente assinante, app sem assinatura e app admin), evento por canal no `move`, importação (datas, autoria, duplicatas, idempotência, limite), retenção.
**Contrato**: todo tipo de evento documentado em `integracao-gestor`.

## Riscos

| Risco | Mitigação |
|-------|-----------|
| Esquecer o evento numa mutação | Teste que percorre `OPERATIONS` de escrita e verifica evento enfileirado para cada uma. |
| Destinatário lento travar a fila | Timeout curto e concorrência própria do runner `webhook`. |
