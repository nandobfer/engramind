# Configurações

O comportamento funcional do Engramind vive nas configurações, lidas e escritas pela API com um token `admin`. As variáveis de ambiente cuidam só da infraestrutura (porta, banco, pepper, chave de cifra, Litestream e hosts do MCP).

## Ler

`get_settings` devolve cada chave com o valor efetivo, o padrão, o tipo, o intervalo, se aceita sobreposição por canal, a última alteração e o app que a fez. Exige `read`.

```bash
engramind settings get
engramind settings get limits.rate_per_minute --json
```

## Alterar

`update_settings` recebe um mapa parcial `chave → valor` e exige `write`. É tudo ou nada: uma chave desconhecida (`SETTING_UNKNOWN`) ou um valor fora da regra (`SETTING_INVALID`) recusa o pedido inteiro. `null` volta ao padrão. Cada mudança gera um evento de auditoria.

```bash
engramind settings set limits.rate_per_minute 1200
engramind settings set limits.rate_per_minute null
```

No REST, `PUT /v1/settings` com `{ "limits.rate_per_minute": 1200 }`; no MCP, `update_settings` com `{ "settings": { "limits.rate_per_minute": 1200 } }`.

## Chaves

| Chave | Tipo | Padrão | Intervalo | Por canal | Descrição |
|---|---|---|---|---|---|
| `limits.rate_per_minute` | integer | 600 | 1 a 100000 | não | Requisições por minuto aceitas de cada token; acima disso, `429 RATE_LIMITED`. |
| `limits.max_batch` | integer | 100 | 1 a 1000 | não | Itens aceitos por operação em lote. |
| `categories.list` | categories | as 5 do Magic Context | 1 a 50 itens | não | Categorias aceitas nas memórias, `{ key, label, priority }`; a primeira é a padrão. Retirar uma categoria em uso dá `409 CATEGORY_IN_USE`. |
| `memory.text_min` | integer | 3 | 1 a 1000 | não | Caracteres mínimos do texto de uma memória. |
| `memory.text_max` | integer | 4000 | 10 a 20000 | sim | Caracteres máximos do texto de uma memória. |
| `memory.dedupe_exact` | boolean | true | — | não | Texto igual (`content_hash` normalizado) a outra memória do canal devolve a existente com `duplicate: true`. |
| `memory.similar_threshold` | number | 0.85 | 0 a 1 | não | Similaridade mínima para uma memória aparecer em `similar` na resposta de escrita (Jaccard dos termos até haver embeddings). |

As categorias padrão são `PROJECT_RULES` (prioridade 50), `ARCHITECTURE` (40), `CONSTRAINTS` (30), `CONFIG_VALUES` (20) e `NAMING` (10). Mudar `label` e `priority` é livre; uma `key` nova vale na hora. Retirar uma `key` usada por alguma memória é recusado com `409 CATEGORY_IN_USE` e a contagem em `details.in_use`.

```bash
engramind settings set memory.similar_threshold 0.6
engramind settings set categories.list '[{"key":"RULES","label":"Regras","priority":10}]'
```

Os demais grupos do desenho (contexto, Redator, embeddings, busca, consolidação, webhooks e retenção) entram junto com as capacidades que eles controlam.

## Por canal

As chaves com "sim" na coluna "Por canal" aceitam sobreposição num canal: `get_channel_settings` mostra o valor efetivo, o global e a sobreposição (`override`); `update_channel_settings` recebe um mapa parcial e `null` remove a sobreposição. Uma chave sem sobreposição por canal dá `400 SETTING_UNKNOWN`, com as aceitas em `details.valid`. Cada mudança incrementa a `version` do canal e gera auditoria.

```bash
engramind channels settings get eng/global
engramind channels settings set eng/global memory.text_max 2000
engramind channels settings set eng/global memory.text_max null
```

No REST, `PUT /v1/channels/{channel}/settings` com `{ "memory.text_max": 2000 }`; no MCP, `update_channel_settings` com `{ "channel": "eng/global", "settings": { "memory.text_max": 2000 } }`.
