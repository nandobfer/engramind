# Tokens e escopos

Todo acesso ao Engramind usa um token Bearer `eng.<token_id>.<segredo>`, por exemplo `eng.tk_3f9e1a7c.<segredo>`. O `token_id` (`tk_` + 8 hex) localiza o registro; o segredo é guardado como HMAC-SHA256 com o `TOKEN_PEPPER` e conferido em tempo constante.

## Identidades

- **App**: um sistema que usa o Engramind (um consumidor ou um gestor). Id `app_…`, slug único na rede.
- **Agente**: um agente de um app. Id `ag_…`, slug único dentro do app; o endereço completo é `app/agente`.
- **Token**: pertence a um app e, opcionalmente, a um agente.

Onde uma operação pede um app, aceite o id ou o slug (`app-a`). Onde pede um agente, aceite o id ou `app/agente` (no caminho REST, `app-a%2Fcurador`).

## Tipos

| `kind` | Para quê | Pertence a |
|---|---|---|
| `agent` | Operar memórias nos canais que o agente assina | um agente (obrigatório) |
| `admin` | Gerir a rede: apps, agentes, tokens, canais, assinaturas, configurações, status e auditoria; alcança todos os canais | um app e, opcionalmente, um agente do mesmo app |
| `app` | O app consumidor gerir os **próprios** agentes, tokens de agente e assinaturas sem token admin; emitido só pelo admin | um app (nunca um agente) |

Um token `agent` nunca alcança as operações administrativas: elas devolvem `403 SCOPE_REQUIRED` e nem aparecem no `tools/list` do MCP.

### O token de app

O token `app` vale só dentro do próprio app. Alvo de outro app → `403 FOREIGN_APP`; o que só o admin faz → `403 SCOPE_REQUIRED` (`details.required_kind`).

| Pode | Não pode |
|---|---|
| `list_agents`, `get_agent`, `create_agent`, `update_agent` (inclusive `disabled`), `delete_agent` do próprio app | Agentes, tokens ou assinaturas de outro app |
| `issue_token` só `kind: agent`, para agente do próprio app, com escopos ⊆ os do token de app; `list_tokens`, `update_token`, `revoke_token` e `rotate_token` desses tokens | Emitir token `admin` ou `app`; mexer no próprio token ou em outro token de app |
| `subscribe`, `update_subscription`, `unsubscribe` para os próprios agentes (o alvo exato inexistente cria o canal) | `create_app`, `update_app`, `delete_app`, `create_channel`, `update_channel`, `delete_channel` |
| `archive_channel` e `restore_channel` dos canais que ele criou ao assinar (`409 CHANNEL_SHARED` se outro app os assina) | Arquivar canal criado pelo admin ou por outro app |
| `import_memories` nos canais em que os agentes ativos dele escrevem; `update_app_webhook` do próprio app | `get_settings`, `update_settings`, segredos, consolidação, status, auditoria, jobs e `/metrics` |
| Ler canais (todos, para assinar; `reach` onde os agentes dele chegam), contexto, memórias, busca, dicas, categorias e `get_consumer_settings` | Criar, editar, arquivar, mover ou excluir uma memória individual (é do token `agent`) |

O alcance do token de app é a união das melhores assinaturas dos agentes **ativos** do app: no canal, os escopos são os do token ∩ a união dos acessos, o modo é o mais amplo (`inject` > `search` > `manage`) e a prioridade, a maior.

```bash
engramind tokens issue --app dndicas --kind app --scopes read,write,delete --label "provisionamento"
```

## Escopos

| Escopo | Permite |
|---|---|
| `read` | Ler e consultar. Num token `admin`, também ler identidades, configurações, status e auditoria. |
| `write` | Criar e alterar. Num token `admin`, também identidades, canais, assinaturas e configurações. |
| `delete` | Excluir de vez. Num token `admin`, também excluir canais, apps e agentes e revogar tokens. |

`write` e `delete` exigem `read`. As combinações válidas são `read`, `read+write`, `read+delete` e `read+write+delete`; qualquer outra recebe `400 VALIDATION_ERROR`.

Quando o tipo ou o escopo não bastam, a resposta é `403 SCOPE_REQUIRED` com `details: { required_kind, required_scope }`.

## Emissão

`issue_token` exige um token `admin` com `write` (ou um token `app`, só para `kind: agent` do próprio app):

```bash
engramind tokens issue --agent app-a/curador --scopes read,write --label "curador producao"
```

- Token `agent` exige `agent`; `app` pode ser omitido quando `agent` é informado. Token `app` exige `app` e não aceita `agent`.
- Quem emite não pode conceder escopos que não tem (`403 SCOPE_REQUIRED`).
- `expires_at` (RFC 3339, no futuro) é opcional.
- O segredo aparece **uma única vez**, em `secret`. Listagens mostram só o prefixo público `eng.<token_id>`. Uma repetição com a mesma `Idempotency-Key` devolve o token sem `secret`.

## Ciclo de vida

- `update_token` altera `label`, `scopes` (sem escalada) e `expires_at` (`null` remove a expiração).
- `revoke_token` revoga na hora: a próxima requisição com o token recebe `401 AUTH_INVALID`.
- `rotate_token` emite um token igual (app, agente, tipo, escopos, rótulo e expiração) e revoga o antigo na mesma transação (auditoria `token.rotate`); o segredo novo aparece só nesta resposta. Só um token ativo é rotacionado (`400 VALIDATION_ERROR` com `details.status`).

```bash
engramind tokens rotate tk_7c2e9a10 --json
```
- `last_used_at` é gravado em lote a cada 10 s, então pode atrasar alguns segundos.
- `list_tokens` mostra `status`: `active`, `expired` ou `revoked`.

## Apps e agentes

```bash
engramind apps create --slug app-a --name "App A"
engramind agents create --app app-a --slug curador --name Curador
engramind apps update app-a --disable
```

- `update_app` e `update_agent` com `disabled: true` desativam sem perder o vínculo: os tokens deixam de autenticar e voltam com `disabled: false`.
- `create_app` e `update_app` aceitam `webhook_url` e `webhook_secret` (só gravado, cifrado): o app passa a receber os eventos dos canais que os agentes dele assinam (todos, se ele tiver um token `admin`). A leitura mostra só `webhook: { url, secret: { configured, updated_at } }`; detalhes no guia `integracao-gestor`.
- `delete_app` remove o app com os agentes, tokens e assinaturas; `delete_agent` remove o agente com os tokens e as assinaturas. A autoria das memórias guarda o id sem chave estrangeira, e a auditoria preserva o registro anterior.
- `create_agent` repetido com os mesmos campos devolve o agente existente com `200` e `created: false`; o mesmo slug com outro nome ou descrição recebe `409 SLUG_ALREADY_EXISTS` com `details.existing`. Um app com slug repetido também recebe `409 SLUG_ALREADY_EXISTS`.
- `update_app_webhook` grava ou remove só o webhook (admin em qualquer app; token de app só no próprio) e emite `identities.updated`.

## Receita: agente com canal próprio

Um agente que lê o canal do app e escreve no próprio canal (por exemplo, um agente revisor `opex/arquiteto` com o canal `agentes/arquiteto`). Com um token `app` `read+write+delete` do app, rode os passos na ordem; cada um é idempotente, então a receita inteira pode rodar de novo sem duplicar nada:

```bash
engramind agents create --app opex --slug arquiteto --name Arquiteto --json
engramind subscriptions add --agent opex/arquiteto --target opex --mode inject --json
engramind subscriptions add --agent opex/arquiteto --target agentes/arquiteto --mode inject --json
engramind tokens list --agent opex/arquiteto --json
engramind tokens issue --agent opex/arquiteto --scopes read,write,delete --label arquiteto --json
```

1. `create_agent`: `201`, ou `200 created: false` na repetição.
2. A primeira assinatura alcança o canal do app; a segunda cria `agentes/arquiteto` (nome = slug, `created_by` = o app) e assina. Repetidas: `200 created: false`.
3. Emita o token só se `list_tokens` não mostrar um `active`; guarde o segredo, que aparece uma vez. Perdeu o segredo? `rotate_token` no token existente, nunca um segundo `issue_token`.

O token do agente lê `opex` e `agentes/arquiteto` no contexto e escreve em ambos (restrinja com `access` na assinatura do canal do app se ele só deve ler ali).

## Último admin

A rede nunca fica sem administração: revogar, expirar, reduzir escopos, desativar ou excluir o que deixaria a rede sem nenhum token `admin` ativo com `write` recebe `409 LAST_ADMIN_TOKEN`. Emita outro token admin antes.

## Bootstrap

O primeiro token `admin` nasce de `BOOTSTRAP_ADMIN_TOKEN` (no formato `eng.tk_<8 hex>.<segredo com 32+ caracteres>`), aceito só quando o banco não tem nenhum token. Ele cria o app `bootstrap` e um token `admin` `read+write+delete`. Com tokens existentes, o valor é ignorado com um aviso no log.

## Quem sou eu

`whoami` funciona com qualquer token e devolve o token, o tipo, os escopos, o app, o agente e as operações permitidas:

```bash
engramind whoami --json
```
