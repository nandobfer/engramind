# 0033 — Canal criado ao assinar e ciclo de vida dos canais do app

- Status: aceita (0.11.0)
- Contexto: desenho §5; decisões 0003 e 0028.

## Decisão

- `subscribe` com alvo exato inexistente cria o canal na mesma transação (nome = slug, sem
  descrição, prioridade 0, auditoria `channel.create` e evento `channel.updated`), para o admin e o
  token de app. Prefixos nunca criam canal.
- `channels.created_by_app_id` (migração `0012`) guarda o app quando o criador é um token de app;
  é ciclo de vida, não dono. O DTO mostra `created_by` (slug).
- O token de app arquiva e restaura só os canais que criou. Qualquer assinatura (exata ou por
  prefixo) de agente **ativo** de outro app que alcance o canal → `409 CHANNEL_SHARED` com
  `details.apps`. `delete_channel` continua só do admin.

## Consequências

- Um prefixo amplo de outro app (`*`-like, como `dndicas/*`) bloqueia o arquivamento pelo app;
  o admin continua podendo.
