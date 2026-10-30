# Canais, assinaturas e acesso efetivo

## Onde vive

- `src/core/selection.rs`: slug de canal, `ChannelPattern` (exato ou prefixo) e `Selection` (cabeçalho ∩ `channels`).
- `src/core/access.rs`: `resolve`, `reach_channel`, `require_scope`, `Core::require_channel` e o cache `NetworkCache`.
- `src/core/channels.rs` e `src/core/subscriptions.rs`: operações e validação.
- `src/store/sqlite/channels.rs`: SQL, `network()` (a rede inteira numa leitura), auditoria e idempotência na mesma transação.
- Adaptadores: `src/rest/channels.rs`, `src/rest/subscriptions.rs`, o extractor `ChannelsHeader` (`src/rest/mod.rs`), `src/mcp/tools.rs` (`channels_header`) e `src/cli/remote.rs`.
- Migração `migrations/0003_channels.sql`.

## Operações

- Qualquer token·read: `list_channels`, `get_channel`, `channel_stats` e `list_subscriptions` (tag `Channels`).
- admin·write: `create_channel`, `update_channel`, `archive_channel`, `restore_channel`, `subscribe`, `update_subscription` e `unsubscribe` (tag `Network`).
- admin·delete: `delete_channel`.

Guia: `canais-e-assinaturas`.

## Invariantes

- Todo acesso a canais passa por `core::access`. Use `require_channel` para um canal citado e `resolve` com o `Purpose` certo para listas, contexto e busca. Nunca filtre assinaturas num serviço ou adaptador.
- Acesso efetivo = escopos do token ∩ `access` da assinatura mais específica. A exata vence o prefixo; o prefixo mais longo vence o curto. O token `admin` alcança tudo em `inject`.
- `manage` só entra quando o canal é citado pelo slug exato. `Purpose::Context` nunca inclui `manage` nem arquivados.
- A seleção só restringe. Com o cabeçalho e o parâmetro, vale a interseção.
- O slug do canal é imutável; o prefixo é gravado sem o `*`.
- Toda mutação de canal incrementa `channels.version`. Toda mutação de canal, assinatura, agente ou app chama `Core::network_changed()` depois do commit.
- `delete_channel` só exclui canal arquivado e leva as assinaturas exatas, as memórias e as revisões delas (`memories_removed` na auditoria). A cascata de `agents` remove as assinaturas, e a auditoria registra `subscriptions_removed`.
- Canal arquivado não recebe escrita de memórias (`409 CHANNEL_ARCHIVED`).
- `Network.channel_settings` traz as sobreposições de configuração por canal; mudá-las chama `network_changed()`. Mutações de memória também invalidam o cache, porque mudam `channels.version`.
- Na CLI, nenhum subcomando tem argumento `channels` (o global vira o cabeçalho).

Desenho: §4, §5, §6 e §19. Decisão: `0010`.
