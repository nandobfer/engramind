# 0028 — Token de app e idempotência natural do provisionamento

- Status: aceita (0.11.0)
- Contexto: desenho §4, §5 e §13; decisões 0002 e 0023.

## Contexto

Até a 0.10.0, só um token `admin` criava agentes, emitia tokens e assinava canais. Um app consumidor
que provisiona os próprios agentes (um agente por cliente, por exemplo) precisava guardar um token
`admin`, que alcança a rede inteira, configurações e segredos. E repetir o provisionamento a cada
inicialização dava `409` em `create_agent` e `subscribe`, que o consumidor precisava tratar como sucesso.

## Decisão

- Novo `TokenKind::App` (`kind: app`), vinculado a um app e nunca a um agente (CHECK no banco,
  migração `0010`, que recria `tokens` preservando os dados). Só o admin o emite.
- O catálogo de operações ganha dois tipos exigidos: `AdminOrApp` (identidades, assinaturas,
  `archive_channel`/`restore_channel`, `import_memories`, `rotate_token`, `update_app_webhook`) e
  `AgentOrAdmin` (todas as mutações individuais de memória, que o token de app não faz). O `core`
  confere que o token de app só opera o próprio app (`403 FOREIGN_APP`), só emite `kind: agent`, nunca
  concede escopos acima dos seus e nunca mexe em token que não seja de agente
  (`403 SCOPE_REQUIRED` com `required_kind: admin`).
- O alcance do token de app é a união das melhores assinaturas dos agentes **ativos** do app:
  escopos = token ∩ união dos acessos; modo = o mais amplo (`inject` > `search` > `manage`);
  prioridade = a maior. A rede em cache leva o app e o estado do agente de cada assinatura.
- `rotate_token` emite um token igual e revoga o antigo numa única transação, para que um segredo
  perdido nunca deixe um token órfão.
- **Idempotência natural**: `create_agent` e `subscribe` com os mesmos campos devolvem o registro
  existente com `200` e `created: false`; com campos diferentes, o `409` de antes com
  `details.existing`. O status da resposta passa a ser decidido dentro da mutação.
- `update_app_webhook` (admin ou o próprio app) emite o novo evento `identities.updated`, entregue
  ao próprio app e aos admins.
- `list_channels` e `get_channel` mostram ao token de app todos os canais (para poder assinar), com
  `reach` só onde os agentes dele chegam.
- Desvios conscientes da especificação da tarefa: não existe `KIND_REQUIRED` (o código é o
  `SCOPE_REQUIRED` de sempre, com `required_kind`); canal existente fora do alcance na importação é
  `403 CHANNEL_NOT_SUBSCRIBED`, como em toda escrita.

## Consequências

- O consumidor provisiona sem token admin, e o provisionamento é idempotente.
- Quem tratava `409 SLUG_ALREADY_EXISTS`/`SUBSCRIPTION_EXISTS` como "já existe" continua
  funcionando no caso divergente; no caso igual, passa a receber `200`.
- A matriz tipo × escopo × operação cobre os três tipos.
