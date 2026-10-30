# Implement - canais e assinaturas

**Data**: 2026-10-30

## Ordem real

1. Pré-requisito: a 002 estava só no diretório de trabalho; foi commitada, entrou pelo PR #3 (CI verde) e sofreu merge na `main`. A 003 saiu de `feat/003-canais` a partir da `main` atualizada.
2. Perguntas Q-001 a Q-005 resolvidas pelas recomendações, mais Q-006 a Q-012, abertas no planejamento (`clarify.md`, D-005 a D-012).
3. Tipos puros com testes: `ScopeSet::intersection`, `core::selection` (slug de canal, `ChannelPattern`, `Selection`), `Mode`, `Channel`, `Subscription`, `Network` e `pagination::paginate` (página em memória).
4. Migração `0003_channels.sql`, extensão do `Store` e `src/store/sqlite/channels.rs`, com auditoria e idempotência na mesma transação. `delete_app`/`delete_agent` passam a auditar `subscriptions_removed`.
5. `core::access` (`resolve`, `reach_channel`, `require_scope`, `Core::require_channel`) e o `NetworkCache` por geração.
6. Serviços `core::channels` e `core::subscriptions`, todos por `authorize` + `mutate`, invalidando o cache depois de cada mutação (também em `delete_app`/`delete_agent`).
7. Catálogo (11 operações, tags `Channels`/`Network`), códigos de erro, REST com `#[utoipa::path]`, o extractor `ChannelsHeader`, as ferramentas MCP (cabeçalho lido das `Parts` da requisição) e a CLI (`channels`, `subscriptions`).
8. Testes: unitários do resolvedor e da seleção; módulo de integração `tests/it/channels.rs`; matriz com as novas operações; snapshot do OpenAPI.
9. Documentação: guia `canais-e-assinaturas`; guias `erros`, `cli`, `integracao-mcp`, `overview` e `tokens-e-escopos`; `aicontext/channels.md` e `aicontext/README.md`; decisão `0010`; desenho §5 e §6; versão 0.3.0 e `CHANGELOG.md`.

## Mudanças além do contrato

- `reach` em `list_channels`/`get_channel` e `reaches` nas assinaturas (D-012).
- `list_subscriptions` aceita `channel` (assinaturas que alcançam o canal) e paginação; um token `agent` que pede outro agente recebe lista vazia, não erro.
- `priority` do canal e da assinatura limitada a -1000…1000 (`VALIDATION_ERROR` fora disso).
- Arquivar um canal já arquivado (ou restaurar um ativo) não muda nada, não grava auditoria e devolve o canal.
- A CLI usa `--token-access` e `--channel-priority` em `subscriptions update` para voltar `access` e `priority` a `null`; `--priority` aceita números negativos.
- `CHANNEL_ARCHIVED` (escrita em canal arquivado) não nasce nesta etapa: não há escrita em canal antes das memórias (004).

## Comandos de teste

```bash
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo nextest run -E 'test(/channels::|access::|selection::|subscriptions::|matrix::|contract::/)'
UPDATE_SNAPSHOTS=1 cargo nextest run -E 'test(/contract::/)'
cargo nextest run --locked   # suíte completa; ./scripts/test.sh usa llvm-cov, que não linka nesta máquina
```

## Rascunho de release notes

**0.3.0 — canais e assinaturas.** O gestor desenha a rede: canais hierárquicos (`eng/global`) e assinaturas de agentes por canal exato ou por prefixo (`eng/*`, `eng/projeto-*`), com modo (`inject`, `search`, `manage`), acesso restritivo e prioridade. O acesso efetivo (escopos do token ∩ acesso da assinatura mais específica) é resolvido no `core` e vale nas três vias. O cabeçalho `Engramind-Channels` e o parâmetro `channels` restringem os canais de cada requisição. Novos erros: `CHANNEL_NOT_SUBSCRIBED`, `CHANNEL_NOT_FOUND`, `SUBSCRIPTION_NOT_FOUND`, `SUBSCRIPTION_EXISTS` e `CHANNEL_NOT_ARCHIVED`.
