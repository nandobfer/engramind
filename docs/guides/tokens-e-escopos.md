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

Um token `agent` nunca alcança as operações administrativas: elas devolvem `403 SCOPE_REQUIRED` e nem aparecem no `tools/list` do MCP.

## Escopos

| Escopo | Permite |
|---|---|
| `read` | Ler e consultar. Num token `admin`, também ler identidades, configurações, status e auditoria. |
| `write` | Criar e alterar. Num token `admin`, também identidades, canais, assinaturas e configurações. |
| `delete` | Excluir de vez. Num token `admin`, também excluir canais, apps e agentes e revogar tokens. |

`write` e `delete` exigem `read`. As combinações válidas são `read`, `read+write`, `read+delete` e `read+write+delete`; qualquer outra recebe `400 VALIDATION_ERROR`.

Quando o tipo ou o escopo não bastam, a resposta é `403 SCOPE_REQUIRED` com `details: { required_kind, required_scope }`.

## Emissão

`issue_token` exige um token `admin` com `write`:

```bash
engramind tokens issue --agent app-a/curador --scopes read,write --label "curador producao"
```

- Token `agent` exige `agent`; `app` pode ser omitido quando `agent` é informado.
- Quem emite não pode conceder escopos que não tem (`403 SCOPE_REQUIRED`).
- `expires_at` (RFC 3339, no futuro) é opcional.
- O segredo aparece **uma única vez**, em `secret`. Listagens mostram só o prefixo público `eng.<token_id>`. Uma repetição com a mesma `Idempotency-Key` devolve o token sem `secret`.

## Ciclo de vida

- `update_token` altera `label`, `scopes` (sem escalada) e `expires_at` (`null` remove a expiração).
- `revoke_token` revoga na hora: a próxima requisição com o token recebe `401 AUTH_INVALID`.
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
- Slug repetido recebe `409 SLUG_ALREADY_EXISTS`.

## Último admin

A rede nunca fica sem administração: revogar, expirar, reduzir escopos, desativar ou excluir o que deixaria a rede sem nenhum token `admin` ativo com `write` recebe `409 LAST_ADMIN_TOKEN`. Emita outro token admin antes.

## Bootstrap

O primeiro token `admin` nasce de `BOOTSTRAP_ADMIN_TOKEN` (no formato `eng.tk_<8 hex>.<segredo com 32+ caracteres>`), aceito só quando o banco não tem nenhum token. Ele cria o app `bootstrap` e um token `admin` `read+write+delete`. Com tokens existentes, o valor é ignorado com um aviso no log.

## Quem sou eu

`whoami` funciona com qualquer token e devolve o token, o tipo, os escopos, o app, o agente e as operações permitidas:

```bash
engramind whoami --json
```
