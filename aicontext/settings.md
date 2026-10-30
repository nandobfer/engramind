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

Chaves atuais: `limits.rate_per_minute` (600) e `limits.max_batch` (100). Desenho §12; guia `configuracoes`.
