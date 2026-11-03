# 0034 — Evento de consumo `memory.consumed`

- Status: aceita (0.12.0)
- Contexto: desenho §17; decisões 0013 (contexto e cache) e 0023 (eventos e destinatários). Issue nandobfer/engramind#17.

## Contexto

O consumo das memórias (quais memórias uma leitura entregou a um agente) só era visível para o próprio sistema consumidor, que publicava o próprio pulso. Um gestor não via o consumo dos agentes de outros apps em tempo real. Os eventos existentes só cobrem mutações e saem na transação da escrita; leituras não abrem transação de escrita, e o `304` do contexto não pode ler o banco.

## Decisão

- Novo tipo `memory.consumed`, emitido por `get_context`, `search_memories`, `get_hints` e `get_memory` quando entregam memórias.
- **Um evento por canal**, como `move_memory`: `channel` + `memory_ids` daquele canal, destinatários por `Audience::Channel` (apps cujos agentes alcançam o canal e apps admin). Nenhum app recebe ids de um canal que não alcança. Os eventos da mesma chamada compartilham `consumption` (`cns_` + 16 hex).
- Extras só com identificadores: `operation`, `app` (slug do app do token), `agent` (slug `app/agente`, `null` para tokens `app` e `admin`) e, no contexto, `title_ids` (faixa 2 do canal) e `context_version`. Canal sem nenhum id na faixa 1 (só títulos) não gera evento. Nunca texto, título, `query`, `text` ou `focus`.
- **Gravação na própria leitura**: `Store::emit_events` grava só os jobs `webhook` (via `journal::emit`) numa transação curta, sem auditoria, idempotência ou mudança de versão. Sem destinatário (ou com `webhooks.enabled = false`), nada é gravado. A emissão é aguardada, mas uma falha vira `WARN webhooks.consumed_failed` e a leitura responde normalmente.
- **`304` pelo cache**: o `get_context` com `304` emite com os ids do bloco que está no cache em memória (`context_cache` ou `focus_cache`, pela mesma chave do plano). Fora do cache (reinício, despejo), o `304` não emite: o invariante "304 sem banco" vale mais que a completude do pulso.
- Sem configuração própria: `webhooks.enabled` controla todos os eventos.

## Consequências

- O volume de jobs `webhook` cresce com as leituras (canais × destinatários por chamada); a limpeza de jobs (decisão 0024) cobre o crescimento, mas o tamanho de `jobs` e a espera do escritor único devem ser acompanhados.
- A leitura ganha uma escrita curta quando há destinatário: a latência passa a depender da fila do escritor.
- `channel_version` no consumo é a versão atual, não uma mudança; consumidores que tratam `memory.*` como invalidação precisam excluir `memory.consumed` (guia `integracao-consumidor`).
