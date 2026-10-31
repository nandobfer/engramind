# Contexto e cache

`get_context` devolve o **bloco de memórias pronto para o prompt**: as memórias `active` e `permanent` dos canais que o token alcança em modo `inject`, em ordem determinística, com orçamento de tokens. O bloco é estável: sem mudança real, o texto e a `version` são os mesmos, e o prefixo do prompt continua no cache do provedor de LLM.

| Via | Como chamar |
|---|---|
| REST | `GET /v1/context?channels=&format=` com `If-None-Match` opcional |
| MCP | ferramenta `get_context { channels?, format?, if_none_match? }` ou resource `engramind://context` |
| CLI | `engramind context [--format] [--if-none-match]` |

Exige `read` (qualquer token). Um token `admin` alcança todos os canais ativos, como se fossem `inject`; passe sempre `channels` para não montar o bloco da rede inteira.

## Quais memórias entram

- Canais alcançados em modo `inject` (assinatura mais específica do agente), nunca os arquivados nem os `search`/`manage`.
- `channels` e o cabeçalho `Engramind-Channels` restringem dentro desse alcance.
- Só `active` e `permanent`; arquivadas ficam fora.

## Ordem

A escolha segue esta ordem, sempre a mesma:

1. prioridade da assinatura (ou do canal, quando a assinatura não tem uma), maior primeiro;
2. `permanent` antes de `active`;
3. prioridade da categoria (`categories.list`), maior primeiro;
4. `importance`, maior primeiro, só quando classificada (as não classificadas vêm depois);
5. `created_at`, mais antiga primeiro;
6. `id`.

Na saída, os canais aparecem pela prioridade e depois pelo slug; dentro de cada canal, as categorias pela prioridade e depois pela chave; dentro de cada categoria, as memórias na ordem da escolha.

## Duas faixas

1. **Texto completo** (`#id: texto`) até `context.budget_tokens` (padrão 4000).
2. **Só o título** (`#id: título`) até `context.titles_budget_tokens` (padrão 800).
3. O resto entra na contagem `counts.overflow` e, com `context.overflow_line`, na linha `<overflow count="N">`.

A **primeira memória que não cabe** fecha a faixa: as seguintes, mesmo menores, já vão para a faixa seguinte. Assim a ordem nunca se inverte. O custo de cada memória é a linha `#id: texto` (ou `#id: título`), estimada com `context.token_estimator` (`o200k` ou `cl100k`); as tags de estrutura não contam. `rendered_tokens` é a estimativa do `content` inteiro.

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
#m_77aa01: Prefixo de versão nos commits de release
</titles>
<overflow count="12">use search_memories para o restante</overflow>
</memories>
```

`<titles>` e `<overflow>` só aparecem quando não vazios. Quebras de linha do texto viram espaço, e todo `<` seguido de um nome de tag do bloco (`memories`, `channel`, `titles`, `overflow` ou uma chave de categoria, com ou sem `/`) sai como `&lt;`: uma memória não consegue fechar nem abrir a estrutura do bloco.

`markdown`, sem ids, para agentes sem ferramentas de escrita: `## <slug>`, `### <CATEGORIA>`, `- texto`, depois `## Títulos` e a linha `_+N memórias: use search_memories para o restante_`.

`json`, para sistemas que montam o próprio bloco: `content` é um objeto `{ channels: [{ slug, categories: [{ key, memories: [{ id, text }] }] }], titles: [{ id, title }], overflow }`, com o texto sem alteração.

## Resposta

```json
{ "content": "<memories version=\"cv_3f9e1a7c0b2d\">…</memories>",
  "version": "cv_3f9e1a7c0b2d",
  "counts": { "full": 4, "titles": 2, "overflow": 12 },
  "rendered_tokens": 612,
  "format": "tagged" }
```

O REST devolve `ETag: "cv_…"`, `Cache-Control: private, no-cache` e `Vary: Authorization, Engramind-Channels`.

## `version` e `304`

A `version` (`cv_` + 12 hex) é um hash do formato, dos canais alcançados (id, versão do canal, modo e prioridade) e das configurações que mudam o bloco (`context.budget_tokens`, `context.titles_budget_tokens`, `context.token_estimator`, `context.overflow_line`, `categories.list`). Ela muda quando:

- uma memória de um canal do bloco é criada, editada, arquivada, restaurada, fixada, movida, revertida ou excluída;
- uma assinatura muda o alcance, o modo ou a prioridade, ou um canal é criado, alterado, arquivado ou excluído;
- uma dessas configurações muda (global ou por canal).

Buscas e o próprio `get_context` nunca mudam a `version`: os contadores (`retrieval_count`, `injected_count`) ficam fora dela.

Com `If-None-Match` igual à `version` atual, a resposta é `304` sem corpo, sem ler o banco: só a rede em memória é consultada. O cabeçalho aceita a `version` com ou sem aspas, com `W/` e em lista separada por vírgula; `*` não vale. No MCP, `if_none_match` devolve `{ "not_modified": true, "version" }`; na CLI, `--if-none-match` não imprime nada e sai com 0 (com `--json`, o mesmo objeto).

```bash
curl -H "Authorization: Bearer $TOKEN" -H 'If-None-Match: "cv_3f9e1a7c0b2d"' \
  "$ENGRAMIND_URL/v1/context?channels=eng/global"
engramind context --if-none-match cv_3f9e1a7c0b2d
```

O bloco renderizado fica num cache em memória com até 1024 entradas, pela `version`. Tokens com o mesmo alcance usam a mesma entrada.

## Contexto e busca

`search_memories` devolve `context_version`, a `version` de `get_context` sem `format` para a mesma seleção. Com `exclude_context: true`, a busca deixa de fora as memórias com texto completo no contexto; as da faixa de títulos continuam buscáveis. `get_hints` deixa de fora as mesmas memórias (guia `busca-e-dicas`).

## Uso registrado

Cada `get_context` com conteúdo (não o `304`) soma `injected_count` nas memórias de texto completo e grava uma linha em `usage_log`, com o instante, o app, o agente, o token, os canais, os ids e `rendered_tokens`. Cada busca e cada pedido de dicas grava a operação (`search_memories` ou `get_hints`), os canais e os ids encontrados. A gravação é em lote, a cada 5 segundos.
