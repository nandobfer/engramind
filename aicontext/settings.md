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
- Segredos de configuração (`set_secret`) chegam na 006; nunca entram nesta tabela.

- Tipos: `integer`, `number`, `boolean`, `categories` (lista `{ key, label, priority }`) e `enum` (`SettingType::Choice`, com `choices` em `get_settings`). `SettingType::range()` é o texto da coluna "Intervalo" do guia.
- Sobreposição por canal: só chaves com `channel_override`, gravadas em `channels.settings_json`, lidas do cache da rede (`Network.channel_settings`) e alteradas por `update_channel_settings` (incrementa `channels.version` e chama `network_changed()`). Chave sem sobreposição → `SETTING_UNKNOWN` com as aceitas.
- Retirar de `categories.list` uma categoria usada → `409 CATEGORY_IN_USE` (conferido na transação do `update_settings`).

Chaves atuais: `limits.rate_per_minute` (600), `limits.max_batch` (100), `categories.list` (5 categorias), `memory.text_min` (3), `memory.text_max` (4000, por canal), `memory.dedupe_exact` (true), `memory.similar_threshold` (0.85), `context.budget_tokens` (4000, por canal), `context.titles_budget_tokens` (800, por canal), `context.default_format` (`tagged`), `context.token_estimator` (`o200k`) e `context.overflow_line` (true). As que mudam o bloco do contexto ficam em `CONTEXT_RENDER_KEYS`. Desenho §12; guia `configuracoes`.
