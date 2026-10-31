# Busca e dicas

O Engramind encontra memórias pelo **sentido** e pelas **palavras**. Cada memória vira um vetor (*embedding*) de `título + texto`, gerado depois da escrita pelo job `embed`; a consulta vira um vetor na hora. Memórias parecidas ficam com vetores próximos, então "rodar todos os testes" encontra "suíte completa" sem palavras em comum. Quem gera os vetores é configurável (guia `configuracoes`); com `embedding.provider = off`, tudo continua funcionando só com a busca textual.

## Buscar

`search_memories { query, channels?, categories?, limit?, include_archived?, exclude_ids?, exclude_context?, fields? }` busca nos canais em modo `inject` e `search`. Canais `manage` e arquivados só entram quando citados pelo slug exato em `channels` ou no `Engramind-Channels`.

1. **Ramo vetorial**: o embedding da consulta é comparado por cosseno com os vetores do modelo vigente, filtrados por canal, categoria e status. Vetores de outro modelo ou de outra dimensão nunca entram.
2. **Ramo textual**: FTS5/BM25 sem acentos e sem caixa ("configuracao" encontra "configuração"), com o título pesando o dobro.
3. **Fusão RRF**: `score = Σ peso / (search.rrf_k + posição)` sobre os dois ramos, com `search.vector_weight` e `search.text_weight` (padrão 1 e 1, `k` = 60), arredondado a 6 casas. Desempate pelo `id`.

A resposta traz `mode`: `hybrid` quando a consulta foi vetorizada (mesmo que o ramo vetorial não ache nada) e `text` quando a busca rodou só com o FTS — embeddings desligados, provedor sem configuração, falha do provedor ou consulta acima de 2 s. A falha nunca vira erro: a busca segue pelo texto, com um `WARN embeddings.query_failed` no log.

- Padrão `limit = search.default_limit` (10, máximo 50); só memórias ativas e fixadas, a menos que `include_archived`. Cada ramo traz até `limit` candidatos.
- `exclude_ids` tira ids escolhidos por você; `exclude_context: true` tira as que já estão com texto completo no contexto do mesmo token e da mesma seleção (os títulos da segunda faixa continuam buscáveis).
- A resposta traz `context_version`, a `version` de `get_context` sem `format` para a mesma seleção: se ela mudou, o bloco do prompt está desatualizado (guia `contexto-e-cache`).
- Cada resultado traz `rank` e `score`; `score` só serve para comparar resultados da mesma busca.
- A busca conta uma recuperação de cada resultado (`retrieval_count`, `last_retrieved_at`).

```bash
engramind memories search testes locais --json
engramind memories search configuracao --fields title --channels eng/global
engramind memories search porta preview --exclude-context
```

Memórias recém-escritas entram no ramo vetorial alguns instantes depois (`embedding_state` vai de `pending` a `ready`); até lá, só o FTS as encontra. Durante uma reindexação (troca de modelo), o ramo vetorial cobre só as memórias já revetorizadas e o FTS cobre o resto.

## Dicas

`get_hints { text, channels?, limit? }` é a "lembrança vaga": feita para o sistema consumidor chamar antes do turno, barata e sem LLM. Busca pelo vetor do `text` (por exemplo, a mensagem do usuário) com o limiar alto `search.hint_threshold` sobre o cosseno, só em canais `inject` e `search` (nunca `manage`, mesmo citado), e deixa de fora o que já está com texto completo no contexto do mesmo token e seleção.

- Devolve só `{ id, title, channel }`, até `limit` (padrão `search.hint_limit` = 3, máximo 20). O agente busca o texto completo com `get_memory` se quiser.
- Sem embeddings (desligados, sem configuração, falha ou acima de 2 s), devolve lista vazia: uma dica nunca é ruído textual.
- Conta uma recuperação de cada dica e grava `usage_log` com `op = get_hints`.
- Leitura por `POST` (o corpo leva o texto): não aceita `Idempotency-Key`.

```bash
engramind hints como rodo os testes
engramind hints deploy na main --limit 5 --json
engramind hints configurar preview --channels eng/global
```

No REST, `POST /v1/hints` com `{ "text": "como rodo os testes?" }`; no MCP, a ferramenta `get_hints` com os mesmos campos.

O limiar depende do modelo: a família e5 (padrão local) comprime os cossenos entre 0,8 e 0,9 até para frases sem relação, então `0.8` deixa passar quase tudo e valores perto de `0.88` filtram melhor. Ajuste `search.hint_threshold` com `test_embedding` e algumas buscas reais.

## Escrita e `similar`

A escrita nunca espera o embedding: `create_memory` responde na hora e o vetor chega pelo job `embed`. Por isso `similar` (quase-duplicatas na resposta da escrita) continua pelo critério textual — o Jaccard dos termos dos candidatos do FTS acima de `memory.similar_threshold`. O cosseno entre memórias já vetorizadas entra com a consolidação.

## Estados do vetor

| `embedding_state` | Significa |
|---|---|
| `pending` | Job `embed` na fila (escrita, edição, título novo do Redator ou reindexação). |
| `ready` | Vetor gravado com o modelo vigente (ou com o anterior, durante uma reindexação). |
| `failed` | O job esgotou as tentativas ou não há provedor configurado; `retry_job` ou `reembed_all` refazem. |
| `off` | Escrita com `embedding.provider = off`; religar enfileira só o que não tem vetor do modelo vigente. |

`channel_stats` mostra as memórias ainda `pending` por canal; `get_status.embedding` mostra o total da rede.
