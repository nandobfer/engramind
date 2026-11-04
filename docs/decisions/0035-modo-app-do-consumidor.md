# 0035 — Modo app do consumidor: configurações exibidas, o próprio app e os canais criados

- Status: aceita (0.13.0)
- Contexto: desenho §12, §13 e §17; decisões 0023 (eventos e destinatários), 0028 (token de app) e 0033 (ciclo de vida dos canais do app). Issue nandobfer/engramind#19.

## Contexto

O consumidor que só tem o token `app` mostrava valores padrão onde o gestor mostra os reais: o prazo de desuso e os pesos da busca vinham de `get_settings`, e o estado do webhook do próprio app de `get_app`, as duas só do admin. O token de app criava, arquivava e restaurava os seus canais, mas não os editava. O `settings.updated` global ia só aos apps admin, então o app sem token admin não era avisado nem das chaves que `get_consumer_settings` já mostrava.

## Decisão

- `get_consumer_settings` ganha `consolidation.stale_after_days` e `search.{vector_weight, text_weight, rrf_k}`: valores globais efetivos (essas chaves não aceitam sobreposição por canal), só leitura, para qualquer token com `read`. `get_settings` continua do admin.
- `get_app` passa a `AdminOrApp`: o token de app lê só o próprio app (`403 FOREIGN_APP` em outro), com a mesma resposta (o webhook mostra a URL e `secret.{configured, updated_at}`, nunca o segredo). A busca vem antes da conferência do dono, como em `update_app_webhook`: um app inexistente dá `404 APP_NOT_FOUND` também ao token de app. `list_apps` continua do admin. Alternativa descartada: o webhook no `whoami`.
- `update_channel` passa a `AdminOrApp` com o critério de `archive_channel`/`restore_channel` (`ensure_app_lifecycle`): só o canal que o app criou, e `409 CHANNEL_SHARED` quando um agente ativo de outro app o alcança.
- `settings::CONSUMER_SETTINGS_KEYS` lista as chaves globais que `get_consumer_settings` mostra. Um `settings.updated` global que muda alguma delas gera, além do evento dos apps admin (com todas as chaves), um segundo evento, com `id` próprio, para `Audience::Consumers` (apps alcançáveis sem token admin válido), com `keys` limitado às chaves do consumidor. Nenhum nome de segredo ou de configuração de provedor chega ao consumidor; `set_secret` nunca o alcança. `create_category` (`categories.list`) passa a avisá-lo.

## Consequências

- Mais jobs `webhook` nas mudanças de configuração global que tocam o consumidor (um por app com webhook).
- Um campo novo de `ConsumerSettings` vindo de uma chave global precisa entrar em `CONSUMER_SETTINGS_KEYS`, senão o consumidor não é avisado da mudança.
- A diferença entre `404` e `403` em `get_app` revela ao token de app se um slug existe, como já acontecia em `update_app_webhook`.
