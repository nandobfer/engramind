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
| `context.budget_tokens` | integer | 4000 | 0 a 200000 | sim | Tokens da faixa de texto completo do contexto. No canal, é o teto da parte dele nessa faixa; o global continua valendo para o bloco. |
| `context.titles_budget_tokens` | integer | 800 | 0 a 50000 | sim | Tokens da faixa `#id: título` do contexto. No canal, é o teto da parte dele nessa faixa. |
| `context.default_format` | enum | `tagged` | `tagged`, `markdown`, `json` | não | Formato do contexto quando `format` não é informado (e do resource `engramind://context`). |
| `context.token_estimator` | enum | `o200k` | `o200k`, `cl100k` | não | Codificação usada para estimar os tokens do contexto (`o200k_base` ou `cl100k_base`). |
| `context.overflow_line` | boolean | true | — | não | Mostra a linha de excedente (`<overflow count="N">`) quando sobram memórias fora das duas faixas. |
| `redator.enabled` | boolean | true | — | não | Liga o Redator. Desligado (ou sem provedor, modelo e chave), toda memória nova recebe `title_state = fallback`, sem chamar o LLM. |
| `redator.provider` | enum | — | `openai_compatible`, `anthropic`, `gemini` | não | Provedor do LLM: OpenAI-compatível (`/chat/completions`), Anthropic (`/v1/messages`) ou Gemini (`generateContent`). |
| `redator.model` | text | — | 1 a 200 caracteres | não | Modelo principal do Redator. |
| `redator.api_key` | secret | — | só gravado (`set_secret`) | não | Chave de API do provedor, cifrada com `SECRETS_KEY`. Grave com `set_secret`; opcional só no `openai_compatible` (servidores locais). |
| `redator.base_url` | url | — | `http://` ou `https://` | não | URL base do provedor. Obrigatória no `openai_compatible` (por exemplo `https://api.openai.com/v1`); no `anthropic` e no `gemini`, vazia usa a URL oficial. |
| `redator.instructions` | text | — | 1 a 4000 caracteres | sim | Instruções acrescentadas ao prompt do sistema do Redator (o canal substitui a global). |
| `redator.rewrite_text` | boolean | true | — | sim | O Redator também revisa o texto (nova revisão `system/redator`); `false` gera só o título. |
| `redator.title_max` | integer | 100 | 20 a 300 | não | Caracteres máximos do título gerado; título maior invalida a resposta. |
| `redator.min_similarity` | number | 0.8 | 0 a 1 | não | Guarda de sentido: similaridade de embedding mínima entre o original e o revisado. Sem embeddings (até a 007), o texto revisado nunca é aplicado. |
| `redator.timeout_ms` | integer | 30000 | 1000 a 300000 | não | Tempo limite de cada chamada ao LLM, em milissegundos. |
| `redator.max_attempts` | integer | 3 | 1 a 10 | não | Tentativas do job `redact` (backoff de 2 s × 2ⁿ, teto de 10 min); esgotadas, `title_state = fallback`. |
| `redator.fallback_models` | list | [] | até 5 itens de 1 a 200 caracteres | não | Modelos tentados em ordem quando o principal falha por indisponibilidade, limite (429) ou tempo limite. |

As categorias padrão são `PROJECT_RULES` (prioridade 50), `ARCHITECTURE` (40), `CONSTRAINTS` (30), `CONFIG_VALUES` (20) e `NAMING` (10). Mudar `label` e `priority` é livre; uma `key` nova vale na hora. Retirar uma `key` usada por alguma memória é recusado com `409 CATEGORY_IN_USE` e a contagem em `details.in_use`.

```bash
engramind settings set memory.similar_threshold 0.6
engramind settings set categories.list '[{"key":"RULES","label":"Regras","priority":10}]'
```

As chaves `context.*` e `categories.list` mudam o bloco de `get_context`, então mudá-las muda a `version` do contexto (guia `contexto-e-cache`); `context.default_format` só vale para quem não passa `format`. Valores `enum` são texto JSON: `engramind settings set context.default_format markdown`.

Os demais grupos do desenho (embeddings, busca, consolidação, webhooks e retenção) entram junto com as capacidades que eles controlam.

Chaves sem padrão (—) valem `null` até serem definidas; `null` volta a elas. Um texto, uma URL ou uma lista são JSON: `engramind settings set redator.model gpt-4o-mini` e `engramind settings set redator.fallback_models '["gpt-4o"]'`.

## Segredos

As chaves do tipo `secret` (hoje, `redator.api_key`) nunca passam por `update_settings` (`400 SETTING_IS_SECRET`): `set_secret` cifra o valor com AES-256-GCM e a chave `SECRETS_KEY`, com um nonce novo a cada gravação, e devolve só `{ key, configured, updated_at }`. O valor nunca volta em resposta, log, erro ou auditoria; `get_settings` lista o segredo com `secret: true`, `configured` e `updated_at`. `value: null` apaga. Uma chave fora do catálogo dá `400 SECRET_UNKNOWN`. Exige `write`.

```bash
printf '%s' "$CHAVE_DO_PROVEDOR" | engramind secrets set redator.api_key
engramind secrets set redator.api_key --delete
```

A CLI lê o valor só da entrada padrão, nunca de argumento (que ficaria no histórico do shell). No REST, `PUT /v1/secrets/redator.api_key` com `{ "value": "…" }`; no MCP, `set_secret` com `{ "key": "redator.api_key", "value": "…" }`. Trocar a `SECRETS_KEY` torna os segredos gravados ilegíveis: o serviço os trata como não configurados (`WARN secrets.decrypt_failed`) até serem gravados de novo.

## Redator

O Redator dá a cada memória um título de uma linha e, com `redator.rewrite_text`, uma versão mais clara do texto, sempre depois da escrita (o agente nunca espera o LLM). Para ligá-lo, defina `redator.provider`, `redator.model`, a chave (`set_secret`) e, no `openai_compatible`, `redator.base_url`. O fluxo completo, a fila e as proteções estão no guia `operacao`.

`test_redator` chama o LLM com um texto de exemplo e devolve `{ title, text, model, latency_ms }`, sem gravar memória nenhuma (só a linha de `llm_calls`, com `purpose = test`). `settings` propõe uma configuração (chaves `redator.*`, inclusive `redator.api_key` em claro) usada só no teste, sem salvar. Sem provedor, modelo, URL ou chave, `400 PROVIDER_NOT_CONFIGURED` com as chaves em `details.missing`; falha do provedor, `502 PROVIDER_FAILED`; tempo limite, `504 PROVIDER_TIMEOUT`. A guarda de sentido não é aplicada no teste. Exige `write`.

```bash
engramind settings test-redator --text "Use cargo nextest para os testes locais."
engramind settings test-redator --text - --settings '{"redator.model":"gpt-4o"}' < exemplo.txt
```

## Por canal

As chaves com "sim" na coluna "Por canal" aceitam sobreposição num canal: `get_channel_settings` mostra o valor efetivo, o global e a sobreposição (`override`); `update_channel_settings` recebe um mapa parcial e `null` remove a sobreposição. Uma chave sem sobreposição por canal dá `400 SETTING_UNKNOWN`, com as aceitas em `details.valid`. Cada mudança incrementa a `version` do canal e gera auditoria.

```bash
engramind channels settings get eng/global
engramind channels settings set eng/global memory.text_max 2000
engramind channels settings set eng/global memory.text_max null
engramind channels settings set eng/global context.budget_tokens 1500
```

No REST, `PUT /v1/channels/{channel}/settings` com `{ "memory.text_max": 2000 }`; no MCP, `update_channel_settings` com `{ "channel": "eng/global", "settings": { "memory.text_max": 2000 } }`.
