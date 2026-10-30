# Clarify - canais e assinaturas

**Atualizado em**: 2026-10-30

## Perguntas

| ID | Tópico | Pergunta | Status | Resolução |
|----|--------|----------|--------|-----------|
| Q-001 | Canal arquivado | O desenho cria `archive_channel` sem definir o efeito. | resolvida | Canal arquivado sai do contexto, da busca e das dicas padrão. Continua legível quando citado explicitamente. Escrita → `409 CHANNEL_ARCHIVED` (entra na 004, com as memórias). Não aparece em `list_channels` sem `include_archived`. |
| Q-002 | Excluir canal com memórias | `delete_channel` apaga as memórias? | resolvida | Só exclui canal arquivado (`409 CHANNEL_NOT_ARCHIVED`). Na 003, apaga o canal e as assinaturas exatas; a 004 estende a exclusão a memórias, revisões e vetores. A auditoria guarda o canal e `subscriptions_removed`. |
| Q-003 | Filtro e cabeçalho juntos | `channels` substitui ou restringe o `Engramind-Channels`? | resolvida | Interseção (D-003). |
| Q-004 | Prefixo global | Aceitar `*` (todos os canais) como alvo? | resolvida | Não. O prefixo precisa de ao menos um segmento completo e da `/` antes do `*` (`eng*` também é recusado). Para alcançar tudo, use um token `admin`. |
| Q-005 | Prefixo parcial | Aceitar `eng/*` e `eng/projeto-*`? | resolvida | Os dois, com especificidade pelo comprimento do prefixo (D-005). |
| Q-006 | Slug mutável | O `slug` do canal pode mudar em `update_channel`? | resolvida | Não. `update_channel` muda só `name`, `description` e `priority`. |
| Q-007 | Citação explícita | O que conta como citar um canal `manage` (FR-009)? | resolvida | O slug exato numa lista de seleção (cabeçalho ou `channels`) ou no parâmetro `channel`. Prefixo não cita. `Purpose::Context` nunca inclui `manage` (D-006). |
| Q-008 | `manage` na listagem | `list_channels` do agente mostra os canais `manage`? | resolvida | Sim, com `mode: "manage"`. É o passo 1 do fluxo do curador (desenho §5). |
| Q-009 | Slug inexistente na seleção | `channels` ou o cabeçalho com slug inexistente dá `404`? | resolvida | Não. A seleção é filtro: o formato é validado (`VALIDATION_ERROR`), e um slug inexistente simplesmente não casa com nada. `404 CHANNEL_NOT_FOUND` vale para alvo único (`{channel}`, `channel`, `target` exato). |
| Q-010 | Alvo da assinatura | `subscribe` com alvo exato exige o canal? E arquivado? | resolvida | O canal precisa existir (`404`); arquivado é aceito. O prefixo não exige canal existente. |
| Q-011 | Tags OpenAPI | O teste de contrato separa tags de operações `any` e `admin`. | resolvida | `Channels` (qualquer token: `list_channels`, `get_channel`, `list_subscriptions`) e `Network` (admin: as 8 mutações, em `ADMIN_TAGS`), cobertas pelo guia `canais-e-assinaturas` (público `all`). |
| Q-012 | Cache do resolvedor | Como cachear sem servir acesso antigo? | resolvida | Contador de geração em memória, incrementado depois do commit de qualquer mutação de canal, assinatura, agente ou app. A leitura guarda a geração lida antes de consultar o banco (D-004). |

## Decisões

| ID | Decisão | Fonte | Impacto |
|----|---------|-------|---------|
| D-001 | `{channel}` no path aceita id `ch_…` ou slug com `/` codificado como `%2F`. | desenho §13 (CLI usa slugs) | Igual para apps e agentes. |
| D-002 | `core::access` é a única porta de acesso a canais: `resolve(network, principal, selection, purpose, include_archived) -> Vec<Reach>`, com `Reach { channel, mode, scopes, priority, subscription, target }` e `Purpose::{Context, Search, Explicit}`, e `Core::require_channel(principal, selection, canal, escopo)`. Serviços nunca consultam assinaturas por conta própria. | desenho §4, §18 | Toda operação de memória chama o resolvedor. |
| D-003 | Seleção efetiva = (`Engramind-Channels` ∩ `channels`) quando ambos existem; padrões com `*` final valem como prefixo. | Q-003 | Documentado no guia. |
| D-004 | O resolvedor lê a rede inteira (canais e assinaturas) numa consulta e a cacheia em memória por geração. Mutações de canal, assinatura, agente ou app invalidam depois do commit; um resultado lido antes de uma invalidação nunca substitui um mais novo. | desenho §8, Q-012 | Evita consulta por chamada; teste de revogação. |
| D-005 | `subscriptions.target` guarda o slug exato ou o prefixo sem o `*` (`eng/`, `eng/projeto-`), com `UNIQUE(agent_id, target, is_prefix)`. Casamento por `starts_with`; especificidade `(exata, comprimento)`. | Q-004, Q-005 | Decisão 0010. |
| D-006 | Modos por propósito: `Context` = só `inject`, sem arquivados; `Search` = `inject` + `search`, mais `manage` e arquivados citados; `Explicit` = qualquer modo (listagem e canal citado). | desenho §5, Q-007 | A 004 e a 005 escolhem o propósito. |
| D-007 | Token `admin` alcança todos os canais em `inject`, com os próprios escopos e a prioridade do canal. | FR-007 | — |
| D-008 | `access: null` = escopos do token no momento da consulta. `update_subscription` usa `double_option` para voltar a `null` e não muda `agent` nem `target`. | FR-006 | — |
| D-009 | `SCOPE_REQUIRED` por canal usa `details: { required_kind: "any", required_scope, channel }`. | FR-008 | Guia `erros`. |
| D-010 | CLI: nenhum subcomando tem argumento `channels`. O `--channels` global vira o cabeçalho `Engramind-Channels`, e o servidor faz a interseção. REST e MCP mantêm o parâmetro `channels` em `list_channels`. | memória do projeto (colisão de ids do clap) | O contrato da CLI compara os caminhos dos comandos. |
| D-011 | Arquivos planos no `core` (`channels.rs`, `subscriptions.rs`, `access.rs`, `selection.rs`), como na 002. | layout da 002 | Diverge dos diretórios previstos no `plan.md`. |
| D-012 | Respostas de `subscribe`, `update_subscription` e `list_subscriptions` trazem `reaches` (canais ativos alcançados); `list_channels` e `get_channel` trazem `reach`. | risco do prefixo parcial | Gestor confere o alcance real. |

## Bloqueios

Nenhum.
