# Operação

Ferramentas para quem opera o Engramind. Todas exigem um token `admin` com `read`.

## Status

`get_status` mostra a versão em execução, o estado do banco e das migrações, a idade da última réplica do Litestream (quando `LITESTREAM_ENABLED=true`) e as contagens de apps, agentes e tokens.

```bash
engramind status --json
```

`backup.status` vale `ok`, `fail` ou `skip` (sem backup configurado ou sem o binário `litestream`).

## Auditoria

Toda mutação de identidade, token e configuração grava um evento na mesma transação, com autor (`app_id`, `agent_id`, `token_id`), ação, entidade e os estados antes e depois. Segredos e digests nunca entram na auditoria. Excluir um app, agente ou token não apaga os eventos dele.

`get_audit` lista do mais recente ao mais antigo, filtra por `entity`, `entity_id` e `action` e pagina por `cursor`.

```bash
engramind audit list --entity token --action token.revoke
```

| Ação | Entidade |
|---|---|
| `app.create`, `app.update`, `app.delete` | `app` |
| `agent.create`, `agent.update`, `agent.delete` | `agent` |
| `token.issue`, `token.update`, `token.revoke`, `token.bootstrap` | `token` |
| `settings.update` | `settings` (com a chave em `entity_id`) |

## Primeiro acesso

Com o banco vazio, defina `BOOTSTRAP_ADMIN_TOKEN` no `.env` e suba o serviço: o log registra `security.bootstrap_created`. Depois, emita os tokens dos sistemas gestores e guarde o do bootstrap num cofre. Com tokens existentes, o valor é ignorado (`security.bootstrap_ignored`); sem nenhum token e sem o valor, o log avisa `security.bootstrap_missing`.

## Logs

Cada requisição gera um único evento `http.request.completed` com `request_id`, rota parametrizada, `app`, `agent`, `token_id`, `operation`, status e duração. Tokens, segredos e o cabeçalho `Authorization` nunca aparecem. As chamadas MCP geram `mcp.tool.completed` em `DEBUG`, com o nome da ferramenta e o resultado, sem os argumentos.
