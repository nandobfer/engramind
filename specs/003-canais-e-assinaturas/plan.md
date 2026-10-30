# Plan - canais e assinaturas

**Data**: 2026-10-30
**Referência de spec**: [`spec.md`](spec.md)

## Resumo

Adicionar canais e assinaturas e o resolvedor de acesso efetivo, que passa a ser a única porta de acesso a canais para todas as operações seguintes.

## Áreas afetadas

**Core**: `src/core/channels/`, `src/core/subscriptions/`, `src/core/access.rs`.
**Adaptadores**: `src/rest/channels.rs`, `src/rest/subscriptions.rs`, extractor `ChannelSelection` (cabeçalho `Engramind-Channels`), ferramentas MCP, `engramind channels|subscriptions`, `--channels`/`ENGRAMIND_CHANNELS`.
**Dados**: `migrations/0003_channels.sql`, `src/store/sqlite/channels.rs`.
**Docs**: guia `canais-e-assinaturas`, `aicontext/channels.md`, `docs/openapi.json`.

## Estratégia de implementação

1. Resolver Q-001 a Q-005.
2. Tipos `Slug`, `ChannelPattern`, `ChannelSelection` com validação e testes.
3. Migração e `Store`.
4. Serviços de canais e assinaturas (auditoria, versão, idempotência).
5. `core::access::resolve` com especificidade, modos, propósito e interseção de filtros; cache invalidado por mudança.
6. REST, MCP e CLI; `Engramind-Channels` nas três vias.
7. Matriz e contrato; guia; versão 0.3.0.

## Estratégia de testes

**Unit**: validação de slug e padrões; especificidade; interseção; `manage` explícito.
**Integração**: CRUD, arquivamento, exclusão (conforme Q-002), cascata na exclusão de agente.
**Matriz**: cenário do §19 do desenho com os quatro agentes, verificando `list_channels` e o acesso efetivo de cada um nas três vias.

## Riscos

| Risco | Mitigação |
|-------|-----------|
| Cache do resolvedor servir acesso antigo após revogar assinatura | Invalidação síncrona na mesma operação que muda assinatura/canal; teste específico. |
