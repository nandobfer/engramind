# Configurações

## Onde vive

- `src/core/settings.rs`: catálogo `SETTINGS` (chave, tipo, padrão, intervalo, sobreposição por canal, versão), validação, `SettingsSnapshot` em memória e as operações `get_settings`/`update_settings`.
- `src/store/sqlite/settings.rs`: tabela `settings(key, value_json, updated_by_app, updated_at)` e auditoria por chave.
- Adaptadores: `src/rest/settings.rs`, ferramentas `get_settings`/`update_settings` em `src/mcp/tools.rs`, `engramind settings get|set`.

## Regras

- Operações admin: `get_settings` (read, `?key=`) e `update_settings` (write, mapa parcial; `null` volta ao padrão; tudo ou nada).
- Chave desconhecida → `400 SETTING_UNKNOWN` com `details.valid`; valor fora da regra → `400 SETTING_INVALID` com a regra em `details`.
- O `core` lê os valores do `SettingsSnapshot` (atualizado em `update_settings` e carregado no startup por `reload_settings`); nunca leia a tabela por requisição.
- Chave nova: entrada em `SETTINGS`, linha em `docs/guides/configuracoes.md` (`| chave | tipo | padrão | intervalo | por canal | descrição |`) e o uso no `core`. O teste de contrato 6 confere o guia.
- Segredos (`SettingType::Secret`, hoje `redator.api_key` e `embedding.api_key`) nunca entram nesta tabela: `set_secret` grava em `secrets` (cifrado, `src/core/secrets.rs`); `update_settings` os recusa com `400 SETTING_IS_SECRET`; `get_settings` os lista com `secret: true`, `configured` e `updated_at`.

- Tipos: `integer`, `number`, `boolean`, `categories` (lista `{ key, label, priority }`), `enum` (`SettingType::Choice`, com `choices` em `get_settings`), `text`, `url`, `list` (de textos; `SettingType::ChoiceList` é a lista de opções, com `choices`), `cron` (5 campos, UTC, validado por `consolidation::schedule::parse`) e `secret`. Padrão `null` ("—" no guia) aceita `null`. `SettingType::range()` é o texto da coluna "Intervalo" do guia.
- Sobreposição por canal: só chaves com `channel_override`, gravadas em `channels.settings_json`, lidas do cache da rede (`Network.channel_settings`) e alteradas por `update_channel_settings` (incrementa `channels.version` e chama `network_changed()`). Chave sem sobreposição → `SETTING_UNKNOWN` com as aceitas.
- Retirar de `categories.list` uma categoria usada → `409 CATEGORY_IN_USE` (conferido na transação do `update_settings`).

Chaves atuais: `limits.rate_per_minute` (600), `limits.max_batch` (100), `categories.list` (5 categorias), `memory.text_min` (3), `memory.text_max` (4000, por canal), `memory.dedupe_exact` (true), `memory.similar_threshold` (0.85), `context.budget_tokens` (4000, por canal), `context.titles_budget_tokens` (800, por canal), `context.default_format` (`tagged`), `context.token_estimator` (`o200k`) `context.overflow_line` (true) e as `redator.*` (`enabled`, `provider`, `model`, `api_key` 🔒, `base_url`, `instructions` e `rewrite_text` por canal, `title_max`, `min_similarity`, `timeout_ms`, `max_attempts`, `fallback_models`), as `embedding.*` (`provider` (`local`), `model` (`multilingual-e5-small`), `base_url`, `api_key` 🔒, `dimensions`, `query_prefix`, `document_prefix`, `batch_size` (32), `timeout_ms` (15000)) as `search.*` (`vector_weight`, `text_weight`, `rrf_k` (60), `default_limit` (10), `hint_threshold` (0.8), `hint_limit` (3)), as `webhooks.*` e `retention.*` (0.8.0) e as `consolidation.*` (`enabled` (false, por canal), `schedule` (`0 3 * * *`), `tasks`, `duplicate_threshold` (0.92), `stale_after_days` (90), `auto_archive_stale` (false)). Mudar uma de `EMBEDDING_IDENTITY_KEYS` passa por `Core::embedding_switch` antes da transação (ver `embeddings.md`). As que mudam o bloco do contexto ficam em `CONTEXT_RENDER_KEYS`. Desenho §12; guia `configuracoes`.
