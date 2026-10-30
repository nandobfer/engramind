# Clarify - canais e assinaturas

**Atualizado em**: 2026-10-30

## Perguntas

| ID | Tópico | Pergunta | Status | Resolução |
|----|--------|----------|--------|-----------|
| Q-001 | Canal arquivado | O desenho cria `archive_channel` sem definir o efeito. | aberta | Recomendação: canal arquivado sai do contexto, da busca e das dicas padrão; continua legível quando citado explicitamente; escrita → `409 CHANNEL_ARCHIVED`; não aparece em `list_channels` sem `include_archived`. |
| Q-002 | Excluir canal com memórias | `delete_channel` apaga as memórias? | aberta | Recomendação: só exclui canal arquivado (`409 CHANNEL_NOT_ARCHIVED` caso contrário) e apaga memórias, revisões, vetores e assinaturas exatas; a auditoria guarda slug e contagem. |
| Q-003 | Filtro e cabeçalho juntos | `channels` substitui ou restringe o `Engramind-Channels`? | aberta | Recomendação: interseção (D-003). O cabeçalho é a política da sessão definida pelo consumidor; o agente não deve escapar dela. |
| Q-004 | Prefixo global | Aceitar `*` (todos os canais) como alvo? | aberta | Recomendação: não. O prefixo precisa de ao menos um segmento completo antes do `*`; quem precisa de tudo usa token `admin`. |
| Q-005 | Prefixo parcial | O §19 do desenho usa `eng/projeto-*`, que é prefixo dentro de um segmento, enquanto o §5 usa `eng/*`. Aceitar os dois? | aberta | Recomendação: aceitar qualquer prefixo terminado em `*` (`eng/*` e `eng/projeto-*`), com especificidade pelo comprimento do prefixo. |

## Decisões

| ID | Decisão | Fonte | Impacto |
|----|---------|-------|---------|
| D-001 | `{channel}` no path aceita id `ch_…` ou slug com `/` codificado como `%2F`. | desenho §13 (CLI usa slugs) | Igual para apps e agentes. |
| D-002 | `core::access::resolve(principal, selection, purpose) -> Vec<Reach>` com `Reach { channel, mode, scopes, priority }` e `Purpose::{Context, Search, Explicit}` é a única porta de acesso a canais; serviços nunca consultam assinaturas por conta própria. | desenho §4, §18 | Toda operação de memória chama o resolvedor. |
| D-003 | Seleção efetiva = (`Engramind-Channels` ∩ `channels`) quando ambos existem; padrões com `*` final valem como prefixo. | Q-003 | Documentado no guia. |
| D-004 | O resolvedor carrega assinaturas do agente e canais ativos numa leitura e cacheia por `(agent_id, versão de assinaturas)` em memória; mudanças de assinatura ou canal invalidam. | desenho §8 (cache) | Evita consulta por chamada. |

## Bloqueios

Q-001 a Q-005 antes da migração `0003`.
