# Spec - canais e assinaturas

**Criado em**: 2026-10-30
**Status**: Implementada
**Versão alvo**: 0.3.0
**Input**: Desenho §4 (acesso efetivo), §5, §13 (Canais e Assinaturas), §18, §19.

## Objetivo

Permitir que o gestor desenhe a rede: criar canais hierárquicos, ligar agentes a canais exatos ou prefixos com modo e acesso, e resolver no `core` o acesso efetivo que todas as operações de memória usarão.

## Escopo

- Canais: criar, listar, consultar, atualizar, arquivar, restaurar e excluir.
- Assinaturas: assinar, listar, atualizar e cancelar; alvo exato ou prefixo terminado em `*`; modos `inject`, `search`, `manage`; acesso; prioridade opcional.
- Resolvedor de acesso efetivo (`core::access`) com seleção por requisição (`channels` e `Engramind-Channels`).
- Cabeçalho `Engramind-Channels` no REST e no MCP; `ENGRAMIND_CHANNELS`/`--channels` na CLI.
- Guia `canais-e-assinaturas`.

## Requisitos

| ID | Requisito |
|----|-----------|
| FR-001 | Slug de canal: segmentos `[a-z0-9][a-z0-9-]*` separados por `/`, até 8 segmentos e 120 caracteres; único. |
| FR-002 | Só tokens `admin` criam, alteram, arquivam, restauram e excluem canais e assinaturas; token `agent` → `403 SCOPE_REQUIRED`. |
| FR-003 | Slug inexistente como alvo único (`{channel}`, `channel`, `target` exato) → `404 CHANNEL_NOT_FOUND`; canais nunca nascem implicitamente. Numa lista de seleção, um slug inexistente só não casa (Q-009). |
| FR-004 | Assinatura por prefixo `eng/*` alcança `eng/x` e `eng/x/y`, inclusive canais criados depois; não alcança `eng`. Prefixo parcial (`eng/projeto-*`) também é aceito (Q-005); `*` e `eng*` não (Q-004). |
| FR-005 | Com várias assinaturas alcançando o mesmo canal, vale a mais específica: exata vence prefixo; prefixo mais longo vence o mais curto. Empate é impossível (assinatura duplicada → `409 SUBSCRIPTION_EXISTS`). |
| FR-006 | `access` da assinatura ⊆ `read`/`write`/`delete` com `read` obrigatório; padrão = escopos do token do agente no momento da consulta (armazenado como `null`). |
| FR-007 | Acesso efetivo = escopos do token ∩ acesso da assinatura; token `admin` alcança todos os canais com os próprios escopos e modo `inject`. |
| FR-008 | Token `agent` sem assinatura no canal → `403 CHANNEL_NOT_SUBSCRIBED`; com assinatura e sem o escopo → `403 SCOPE_REQUIRED` com `details.channel`. |
| FR-009 | Modo `manage` só é alcançado quando o canal é citado de forma explícita (slug exato em `channels`, no `Engramind-Channels` ou no parâmetro `channel`); `list_channels` o mostra com `mode: manage` (Q-007, Q-008). |
| FR-010 | `channels` (lista de slugs e prefixos) e `Engramind-Channels` restringem; com os dois, vale a interseção. Nunca ampliam. |
| FR-011 | `list_channels` e `get_channel` para token `agent` mostram só os canais alcançados, com `mode` e `access` efetivos. |
| FR-012 | `list_subscriptions` para token `agent` mostra só as do próprio agente. |
| FR-013 | Toda mutação de canal incrementa `channels.version` e grava auditoria; mudança de assinatura grava auditoria. |
| FR-014 | `delete_agent` e `delete_app` removem as assinaturas do agente. |

## Fora do escopo

- Memórias, `channel_stats`, configurações por canal (004).
- Webhooks `channel.updated` e `subscription.updated` (008).

## Critérios de sucesso

- A tabela de exemplo do §19 do desenho é montada pela CLI e cada agente vê exatamente os canais e modos esperados.
- A matriz token × assinatura × operação cobre exato, prefixo, especificidade, acesso restrito, `manage` explícito e interseção de filtros.
