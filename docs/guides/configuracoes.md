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

Os demais grupos do desenho (contexto, memórias, Redator, embeddings, busca, consolidação, webhooks e retenção) entram junto com as capacidades que eles controlam.
