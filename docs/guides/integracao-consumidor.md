# Integração de um sistema consumidor

Um sistema consumidor coloca as memórias do Engramind no prompt dos seus agentes. Esta é a primeira versão do guia: cobre o contexto, a busca e as ferramentas. Dicas por turno (`get_hints`), webhooks e importação chegam em versões seguintes.

## Preparação (gestor, token `admin`)

1. Crie o app e um agente por papel (`create_app`, `create_agent`).
2. Emita um token `agent` por agente com os escopos mínimos: `read` para quem só lê; `read,write` para quem grava (`issue_token`).
3. Assine os canais: `inject` para o que entra no prompt, `search` para o que só se busca, `manage` para curadoria (`subscribe`). Ajuste `priority` na assinatura para decidir a ordem entre canais.

O detalhe das assinaturas está no guia `canais-e-assinaturas`.

## Por turno

1. Chame `get_context` com `channels` do projeto e `If-None-Match` com a `version` que você já usa.
2. `304`: o prompt continua igual. Com conteúdo: troque o bloco e guarde a nova `version`.
3. Coloque o bloco na **parte estável** do prompt (instruções de sistema ou entrada durável da sessão), nunca junto da mensagem do turno. Assim o prefixo continua no cache do provedor.

```bash
curl -s -H "Authorization: Bearer $TOKEN" -H "If-None-Match: \"$VERSION\"" \
  "$ENGRAMIND_URL/v1/context?channels=eng/global,eng/projeto-atlas" -D headers.txt
```

O bloco é determinístico: a mesma rede, as mesmas memórias e as mesmas configurações dão o mesmo texto e a mesma `version`, em qualquer réplica do seu sistema e depois de reinícios do Engramind. Ordem, faixas e formatos estão no guia `contexto-e-cache`.

## Ferramentas do agente

- **MCP**: registre `POST /mcp` com o token do agente e o cabeçalho `Engramind-Channels` do projeto. `tools/list` já mostra só o que o token pode usar; repasse ao agente só as ferramentas que a sua política permite (por exemplo, `search_memories` e `get_memory` para leitura, `create_memory` para quem grava). O resource `engramind://context` é o mesmo bloco, e o prompt `memory-writing` é um texto curto para anexar ao agente que grava.
- **CLI**: a alternativa é a skill `engramind`, com a CLI configurada por `ENGRAMIND_URL`, `ENGRAMIND_TOKEN` e `ENGRAMIND_CHANNELS`.

Na busca, passe `exclude_context: true` para não repetir o que o agente já tem no prompt. Se `context_version` da resposta for diferente da `version` em uso, reconstrua o prompt no próximo turno.

## Erros

Trate os códigos estáveis do guia `erros`: `401 AUTH_INVALID` (token revogado ou expirado), `403 CHANNEL_NOT_SUBSCRIBED` (canal citado fora das assinaturas), `429 RATE_LIMITED` (respeite `Retry-After`) e `503 DATABASE_UNAVAILABLE` (tente de novo mais tarde, mantendo o último bloco).
