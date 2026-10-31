# Integração de um sistema consumidor

Um sistema consumidor coloca as memórias do Engramind no prompt dos seus agentes. Este guia cobre o contexto, as dicas por turno, a busca, as ferramentas, os avisos de mudança e a importação das memórias que já existem.

## Preparação (gestor, token `admin`)

1. Crie o app e um agente por papel (`create_app`, `create_agent`).
2. Emita um token `agent` por agente com os escopos mínimos: `read` para quem só lê; `read,write` para quem grava (`issue_token`).
3. Assine os canais: `inject` para o que entra no prompt, `search` para o que só se busca, `manage` para curadoria (`subscribe`). Ajuste `priority` na assinatura para decidir a ordem entre canais.

O detalhe das assinaturas está no guia `canais-e-assinaturas`.

Para trazer memórias que já existem, o gestor usa `import_memories` (token `admin`): um canal por lista de origem, preservando datas, autoria e origem, com `Idempotency-Key` obrigatória (guia `integracao-gestor`).

## Por turno

1. Chame `get_context` com `channels` do projeto e `If-None-Match` com a `version` que você já usa.
2. `304`: o prompt continua igual. Com conteúdo: troque o bloco e guarde a nova `version`.
3. Coloque o bloco na **parte estável** do prompt (instruções de sistema ou entrada durável da sessão), nunca junto da mensagem do turno. Assim o prefixo continua no cache do provedor.

```bash
curl -s -H "Authorization: Bearer $TOKEN" -H "If-None-Match: \"$VERSION\"" \
  "$ENGRAMIND_URL/v1/context?channels=eng/global,eng/projeto-atlas" -D headers.txt
```

O bloco é determinístico: a mesma rede, as mesmas memórias e as mesmas configurações dão o mesmo texto e a mesma `version`, em qualquer réplica do seu sistema e depois de reinícios do Engramind. Ordem, faixas e formatos estão no guia `contexto-e-cache`.

4. Opcional: antes do turno, chame `get_hints { text }` com a mensagem do usuário e acrescente as dicas (`#id · título · canal`) **junto da mensagem do turno**, nunca na parte estável. São baratas (sem LLM), só trazem o que passa do limiar `search.hint_threshold` e nunca repetem o que já está no contexto; o agente busca o texto completo com `get_memory` se quiser. Sem embeddings, a lista vem vazia (guia `busca-e-dicas`).

## Quando reconstruir o prompt

- **Com webhook**: configure `webhook_url` e `webhook_secret` no app (`update_app`). Os eventos `memory.*`, `channel.updated` e `subscription.updated` dos canais que os seus agentes assinam chegam assinados com HMAC-SHA256; ao receber um, marque o contexto do canal como sujo e chame `get_context` no próximo turno. Verificação, tipos e retentativas estão no guia `integracao-gestor` (para tokens `admin`).
- **Sem webhook**: o `get_context` com `If-None-Match` no início de cada turno já é o aviso: um `304` de poucos bytes quando nada mudou.

## Ferramentas do agente

- **MCP**: registre `POST /mcp` com o token do agente e o cabeçalho `Engramind-Channels` do projeto. `tools/list` já mostra só o que o token pode usar; repasse ao agente só as ferramentas que a sua política permite (por exemplo, `search_memories` e `get_memory` para leitura, `create_memory` para quem grava). O resource `engramind://context` é o mesmo bloco, e o prompt `memory-writing` é um texto curto para anexar ao agente que grava.
- **CLI**: a alternativa é a skill `engramind`, com a CLI configurada por `ENGRAMIND_URL`, `ENGRAMIND_TOKEN` e `ENGRAMIND_CHANNELS`.

A busca é por sentido e por texto (`mode: hybrid`), com queda silenciosa para só texto (`mode: text`) quando os embeddings estão indisponíveis. Na busca, passe `exclude_context: true` para não repetir o que o agente já tem no prompt. Se `context_version` da resposta for diferente da `version` em uso, reconstrua o prompt no próximo turno.

## Erros

Trate os códigos estáveis do guia `erros`: `401 AUTH_INVALID` (token revogado ou expirado), `403 CHANNEL_NOT_SUBSCRIBED` (canal citado fora das assinaturas), `429 RATE_LIMITED` (respeite `Retry-After`) e `503 DATABASE_UNAVAILABLE` (tente de novo mais tarde, mantendo o último bloco).
