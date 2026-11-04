# Contexto e cache

`get_context` devolve o **bloco de memórias pronto para o prompt**: as memórias `active` e `permanent` dos canais que o token alcança em modo `inject`, em ordem determinística, com orçamento de tokens. O bloco é estável: sem mudança real, o texto e a `version` são os mesmos, e o prefixo do prompt continua no cache do provedor de LLM.

| Via | Como chamar |
|---|---|
| REST | `GET /v1/context?channels=&format=&focus=` com `If-None-Match` opcional |
| MCP | ferramenta `get_context { channels?, format?, focus?, if_none_match? }` ou resource `engramind://context` (sem foco) |
| CLI | `engramind context [--format] [--focus] [--if-none-match]` |

Exige `read` (qualquer token). Um token `admin` alcança todos os canais ativos, como se fossem `inject`; passe sempre `channels` para não montar o bloco da rede inteira.

## Quais memórias entram

- Canais alcançados em modo `inject` (assinatura mais específica do agente), nunca os arquivados nem os `search`/`manage`.
- `channels` e o cabeçalho `Engramind-Channels` restringem dentro desse alcance.
- Só `active` e `permanent`; arquivadas ficam fora.

## Ordem

A escolha segue esta ordem, sempre a mesma:

1. prioridade da assinatura (ou do canal, quando a assinatura não tem uma), maior primeiro;
2. com `focus`, as memórias relevantes ao foco antes das demais, da mais relevante à menos (veja Foco);
3. `permanent` antes de `active`;
4. prioridade da categoria no canal da memória: a de `categories.list`, sobreposta por `context.category_priorities` (global e depois a do canal), maior primeiro;
5. `importance`, maior primeiro, só quando classificada (as não classificadas vêm depois);
6. `created_at`, mais antiga primeiro;
7. `id`.

`context.category_priorities` é um mapa `{ "<CATEGORIA>": prioridade }` (de -1000 a 1000) com sobreposição por canal: num canal em que restrições pesam mais, `{ "CONSTRAINTS": 60 }` põe essa categoria na frente só ali, sem rebaixar a categoria inteira nos outros canais. As chaves precisam existir em `categories.list`.

## Foco

`focus` é o texto da tarefa ou da conversa do turno (até 2000 caracteres depois de tirar os espaços das pontas e juntar os do meio; acima disso, `400 FOCUS_TOO_LONG`; só espaços = sem foco). Sem LLM, o serviço marca como **relevante** a memória dos canais do bloco que tem cosseno ≥ `context.focus_min_score` com o embedding do foco (até 50 por ramo) ou algum termo do foco no FTS5; entre as relevantes, a posição é a fusão RRF da busca. As relevantes sobem dentro de cada prioridade de canal: o foco reordena, não troca a regra de prioridade nem os orçamentos.

- `focus_mode` (na resposta e no cabeçalho `Engramind-Focus-Mode`) diz como o foco rodou: `hybrid` (FTS5 + vetor), `text` (só FTS5: embeddings desligados, com falha ou além de `context.focus_embed_timeout_ms`) ou `none` (o FTS5 também falhou: o bloco sai sem foco). O contexto nunca falha por causa do foco.
- A `version` com foco inclui o digest do foco normalizado, o modelo de embedding vigente, `context.focus_min_score` e o modo. Focos diferentes nunca recebem `304` um do outro, e sem foco a `version` é a de sempre. Um bloco degradado (`text` quando se esperava `hybrid`, ou `none`) sai com outra `version` e não entra em cache, então nunca é confundido com o bloco completo.
- Os blocos com foco ficam num cache próprio, pequeno (64 entradas), para não tirar os blocos sem foco do cache principal.
- O texto do foco nunca é gravado: nem em log, nem em `usage_log`, nem na auditoria.
- Limitação conhecida: um embedding que chega depois sem mudar a `version` do canal não muda a `version` do foco; o `304` pode manter a relevância calculada antes até a próxima mudança real.

```bash
engramind context --focus "como rodar os testes de integração" --json
```

Na saída, os canais aparecem pela prioridade e depois pelo slug; dentro de cada canal, as categorias pela prioridade e depois pela chave; dentro de cada categoria, as memórias na ordem da escolha.

## Duas faixas

1. **Texto completo** (`#id: texto`) até `context.budget_tokens`. Com `context.text_max_chars` (por canal, 0 = sem corte), o texto sai sem quebras e cortado nesse número de caracteres, com `…`; o orçamento mede o texto cortado, e a busca e as dicas devolvem o texto inteiro.
2. **Só o título** (`#id [CATEGORIA] @canal: título`) até `context.titles_budget_tokens`. A categoria e o canal vão na linha porque a faixa não é agrupada: assim o agente distingue um canal privado do público no mesmo bloco.
3. O resto entra na contagem `counts.overflow` e, com `context.overflow_line`, na linha `<overflow count="N">`.

A **primeira memória que não cabe** fecha a faixa: as seguintes, mesmo menores, já vão para a faixa seguinte. Assim a ordem nunca se inverte. O custo de cada memória é a linha `#id: texto` (ou `#id [CATEGORIA] @canal: título`), estimada com `context.token_estimator` (`o200k` ou `cl100k`); as tags de estrutura não contam. `rendered_tokens` é a estimativa do `content` inteiro.

As duas chaves de orçamento aceitam sobreposição por canal (`update_channel_settings`). No canal, ela é o **teto da parte daquele canal** em cada faixa: quando uma memória estoura o teto, a faixa fecha só para aquele canal, e os outros continuam. O orçamento global continua valendo para o bloco inteiro.

A escolha é a mesma em todos os formatos: `counts` e os ids não mudam de um formato para outro.

## Formatos

`format` aceita `tagged` (padrão `context.default_format`), `markdown` ou `json`.

`tagged`, o mais compacto, com ids para agir:

```
<memories version="cv_3f9e1a7c0b2d">
<channel slug="eng/global">
<CONSTRAINTS>
#m_8f2a1c: Nunca rodar a suíte completa de testes localmente; só os testes afetados.
</CONSTRAINTS>
</channel>
<titles>
#m_77aa01 [NAMING] @eng/global: Prefixo de versão nos commits de release
</titles>
<overflow count="12">use search_memories para o restante</overflow>
</memories>
```

`<titles>` e `<overflow>` só aparecem quando não vazios. Quebras de linha do texto viram espaço, e todo `<` seguido de um nome de tag do bloco (`memories`, `channel`, `titles`, `overflow` ou uma chave de categoria, com ou sem `/`) sai como `&lt;`: uma memória não consegue fechar nem abrir a estrutura do bloco.

`markdown`, sem ids, para agentes sem ferramentas de escrita: `## <slug>`, `### <CATEGORIA>`, `- texto`, depois `## Títulos` (`- [CATEGORIA] @canal: título`) e a linha `_+N memórias: use search_memories para o restante_`.

`json`, para sistemas que montam o próprio bloco: `content` é um objeto `{ channels: [{ slug, categories: [{ key, memories: [{ id, text }] }] }], titles: [{ id, title, category, channel }], overflow }`, com o texto sem alteração (com `context.text_max_chars`, cortado como nos outros formatos).

## Resposta

```json
{ "content": "<memories version=\"cv_3f9e1a7c0b2d\">…</memories>",
  "version": "cv_3f9e1a7c0b2d",
  "counts": { "full": 4, "titles": 2, "overflow": 12 },
  "rendered_tokens": 612,
  "format": "tagged" }
```

Com `focus`, a resposta traz também `"focus_mode": "hybrid"` (ou `text`/`none`); sem foco, o campo não aparece.

O REST devolve `ETag: "cv_…"`, `Cache-Control: private, no-cache` e `Vary: Authorization, Engramind-Channels`.

## `version` e `304`

A `version` (`cv_` + 12 hex) é um hash do formato, dos canais alcançados (id, versão do canal, modo e prioridade) e das configurações que mudam o bloco (`context.budget_tokens`, `context.titles_budget_tokens`, `context.token_estimator`, `context.overflow_line`, `categories.list`, `context.category_priorities`, `context.text_max_chars`), mais o foco quando houver. Desde a 0.11.0, que mudou a linha de títulos, o hash usa outro domínio: uma `version` de antes nunca recebe `304`. Ela muda quando:

- uma memória de um canal do bloco é criada, editada, arquivada, restaurada, fixada, movida, revertida ou excluída;
- uma assinatura muda o alcance, o modo ou a prioridade, ou um canal é criado, alterado, arquivado ou excluído;
- uma dessas configurações muda (global ou por canal).

Buscas e o próprio `get_context` nunca mudam a `version`: os contadores (`retrieval_count`, `injected_count`) ficam fora dela.

Com `If-None-Match` igual à `version` atual, a resposta é `304` sem corpo, sem ler o banco para decidir: só a rede em memória é consultada. A única escrita é a do consumo: com webhooks e algum destinatário, o `304` grava os jobs `memory.consumed` numa transação curta (decisão 0034), o que custa uma escrita por poll; sem webhooks ou sem destinatário, o `304` não toca o banco. O cabeçalho aceita a `version` com ou sem aspas, com `W/` e em lista separada por vírgula; `*` não vale. No MCP, `if_none_match` devolve `{ "not_modified": true, "version" }`; na CLI, `--if-none-match` não imprime nada e sai com 0 (com `--json`, o mesmo objeto).

```bash
curl -H "Authorization: Bearer $TOKEN" -H 'If-None-Match: "cv_3f9e1a7c0b2d"' \
  "$ENGRAMIND_URL/v1/context?channels=eng/global"
engramind context --if-none-match cv_3f9e1a7c0b2d
```

O bloco renderizado fica num cache em memória com até 1024 entradas, pela `version`. Tokens com o mesmo alcance usam a mesma entrada.

Com webhooks, cada entrega do bloco gera `memory.consumed` por canal (guia `integracao-gestor`), inclusive o `304`, com os ids do bloco em cache; fora do cache (depois de um reinício, por exemplo), o `304` não emite. O consumo não muda a `version`.

## Contexto e busca

`search_memories` devolve `context_version`, a `version` de `get_context` sem `format` para a mesma seleção. Com `exclude_context: true`, a busca deixa de fora as memórias com texto completo no contexto; as da faixa de títulos continuam buscáveis. `get_hints` deixa de fora as mesmas memórias (guia `busca-e-dicas`).

## Uso registrado

Cada `get_context` com conteúdo (não o `304`) soma `injected_count` nas memórias de texto completo e grava uma linha em `usage_log`, com o instante, o app, o agente, o token, os canais, os ids e `rendered_tokens`. Cada busca e cada pedido de dicas grava a operação (`search_memories` ou `get_hints`), os canais e os ids encontrados. A gravação é em lote, a cada 5 segundos.
