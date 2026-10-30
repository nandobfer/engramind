# 0010 — Acesso efetivo, prefixos e seleção de canais

**Data**: 2026-10-30 · **Etapa**: 003 (0.3.0) · **Status**: aceita

## Contexto

O desenho (§4, §5) define o acesso efetivo (escopos do token ∩ acesso da assinatura), as assinaturas por canal exato ou por prefixo, os modos `inject`/`search`/`manage` e a seleção por requisição (`channels` e `Engramind-Channels`). Mas deixa abertos o formato do prefixo, o efeito de arquivar, a regra entre o cabeçalho e o parâmetro e onde o acesso é resolvido.

## Decisão

- **Porta única.** `core::access` é a única porta de acesso a canais. `resolve(network, principal, selection, purpose, include_archived)` devolve os canais alcançados (`Reach { channel, mode, scopes, priority, subscription, target }`). `Core::require_channel(principal, selection, canal, escopo)` resolve um canal citado e devolve `404 CHANNEL_NOT_FOUND`, `403 CHANNEL_NOT_SUBSCRIBED` ou `403 SCOPE_REQUIRED` com `details.channel`. Nenhum serviço consulta assinaturas por conta própria.
- **Propósito.**
  - `Context`: só `inject`, nunca arquivados.
  - `Search`: `inject` e `search`; `manage` e arquivados só quando o canal é citado pelo slug exato.
  - `Explicit`: qualquer modo. Usado na listagem e nos canais citados.
- **Prefixo.** Qualquer alvo terminado em `*` com ao menos um segmento completo e a `/` antes (`eng/*`, `eng/projeto-*`). `*` e `eng*` são recusados. O `target` é gravado sem o `*` (`eng/`, `eng/projeto-`) e o casamento é `slug.starts_with(target)`. A unicidade é `(agent_id, target, is_prefix)`.
- **Especificidade.** A exata vence qualquer prefixo; entre prefixos, o mais longo vence. Uma assinatura repetida é recusada (`SUBSCRIPTION_EXISTS`), então não há empate.
- **`access`.** `null` usa os escopos do token no momento da consulta. Uma lista é interseccionada com eles e nunca amplia.
- **Token `admin`.** Alcança todos os canais em modo `inject`, com os próprios escopos e a prioridade do canal.
- **Seleção.** O cabeçalho e o parâmetro são listas de slugs e prefixos. Com os dois, vale a interseção. Um slug inexistente numa lista não é erro, só não casa com nada. Um canal citado fora da seleção recebe `CHANNEL_NOT_SUBSCRIBED`.
- **Canal arquivado.** Sai do contexto, da busca padrão e de `list_channels` sem `include_archived`, e continua legível quando citado. Só um canal arquivado pode ser excluído (`CHANNEL_NOT_ARCHIVED`). A exclusão remove as assinaturas exatas e mantém as de prefixo.
- **Slug imutável.** O slug do canal não muda, porque assinaturas e prefixos dependem dele.
- **Cache.** A rede inteira (canais e assinaturas) é lida numa consulta e guardada em memória com um contador de geração. Toda mutação de canal, assinatura, agente ou app incrementa a geração depois do commit. Uma leitura grava o resultado com a geração lida antes de consultar o banco, então um resultado obsoleto nunca fica marcado como atual. Como há uma instância por banco, não é preciso coordenar instâncias.
- **CLI.** O filtro vem do `--channels` global, enviado como cabeçalho. Nenhum subcomando tem argumento `channels`, para não colidir com o global.

## Consequências

- A 004 (memórias) e a 005 (contexto) chamam `require_channel` e `resolve` com o propósito adequado, sem tocar em assinaturas.
- Um prefixo parcial alcança mais do que parece (`eng/proj*` também alcança `eng/proj/x`). O campo `reaches` da assinatura e o guia `canais-e-assinaturas` mostram o alcance real.
- Tags OpenAPI: `Channels` (leitura, qualquer token) e `Network` (mutações, admin), cobertas pelo guia `canais-e-assinaturas`.
