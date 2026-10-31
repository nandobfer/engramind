# Operação

Ferramentas para quem opera o Engramind. Todas exigem um token `admin` com `read`.

## Status

`get_status` mostra a versão em execução, o estado do banco e das migrações, a idade da última réplica do Litestream (quando `LITESTREAM_ENABLED=true`), as contagens de apps, agentes e tokens, a fila (`queue`: pendentes, falhos e jobs por tipo e estado) e o Redator (`redator`: ligado, configurado, chaves que faltam em `missing`, provedor, modelo e o último sucesso e a última falha das chamadas do job `redact`).

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
| `secret.set`, `secret.delete` | `secret` (com a chave em `entity_id`; só `configured` e `updated_at`) |
| `job.retry` | `job` |
| `memory.redact` | `memory` (autor `system/redator`) |

## Fila de jobs

O trabalho assíncrono roda numa fila em tabela (`jobs`), sem broker. A escrita de uma memória grava os jobs na mesma transação e responde na hora; um runner por tipo os executa depois do commit. Nesta versão só o `redact` (Redator) tem runner, com 2 jobs em paralelo; os jobs `embed` ficam em `queued` até os embeddings chegarem.

- **Captura**: o runner pega o job pronto mais antigo do tipo com um único `UPDATE … RETURNING`, marca `running`, conta a tentativa e reserva um lease (`locked_until`). Ele acorda a cada commit que enfileira e, sem aviso, a cada segundo.
- **Lease vencido**: um job `running` cujo lease venceu (o processo caiu no meio) volta a ser capturável.
- **Falha temporária**: o job volta para `queued` com backoff de 2 s × 2ⁿ (teto de 10 min) e `last_error`, até `redator.max_attempts`. Esgotadas as tentativas, fica `failed`, com um único `ERROR` `jobs.failed`.
- **Superado**: um job de uma memória excluída, ou com um job do mesmo tipo mais novo para a mesma memória (o texto mudou de novo), termina `done` com `outcome = superseded`, sem efeito.

`list_jobs` lista do mais recente ao mais antigo, filtra por `state` e `kind` e pagina por `cursor`; o `payload` só tem ids e versões. `retry_job` recoloca um job `failed` na fila, com as tentativas zeradas; outro estado dá `409 JOB_NOT_RETRYABLE` e um id inexistente, `404 JOB_NOT_FOUND`. Exigem `read` e `write`.

```bash
engramind jobs list --state failed
engramind jobs retry 42
```

| `outcome` | Quando |
|---|---|
| `applied` | Título e texto revisado aplicados. |
| `title_only` | Só o título (sem revisão de texto, texto igual, duplicata no canal ou guarda de sentido). |
| `superseded` | Memória excluída ou conteúdo mudou depois do job. |
| `skipped` | Redator desligado ou sem configuração: `title_state = fallback`. |

## Redator

O job `redact` envia o texto ao LLM como **dado**, entre marcas com um código aleatório e com a instrução de não seguir nada do conteúdo; `redator.instructions` (global ou do canal) vai no fim do prompt do sistema. A resposta `{ title, text }` é validada pelo serviço (esquema, `redator.title_max`, `memory.text_min`/`text_max`), independente do modo de saída estruturada do provedor; resposta inválida conta como falha da tentativa.

- **Título**: aplicado com `title_state = ready`; não cria revisão nem muda a `version` da memória, mas incrementa a `version` do canal e muda a `version` do contexto (o título aparece na faixa de títulos).
- **Texto revisado** (com `redator.rewrite_text`): só vale se mudou, não repete outra memória do canal e passa na **guarda de sentido** (similaridade de embedding acima de `redator.min_similarity`). Aplicado, nasce uma revisão com autor `system/redator` (`reason = redact`), a `version` da memória sobe e o job `embed` é enfileirado; o original continua no histórico e `revert_memory` o restaura. Até os embeddings existirem, a guarda não tem como medir e falha fechada: só o título é aplicado.
- **Desligado ou sem configuração** (`redator.enabled = false`, ou sem provedor, modelo, chave ou URL): `title_state = fallback`, sem chamar o LLM e sem `ERROR`.
- **Modelos**: o principal e, em ordem, os de `redator.fallback_models` quando o anterior falha por indisponibilidade, `429` ou tempo limite (`WARN redator.fallback`).
- **Conflito**: se a memória mudou enquanto o LLM respondia, o resultado é descartado (`superseded`); a edição nova já enfileirou outro job.

Os logs do Redator (`redator.completed` em `INFO`, `redator.retry` e `redator.fallback` em `WARN`) trazem provedor, modelo, resultado, tentativa e duração, nunca o texto, o prompt ou a chave.

## Chamadas ao LLM

Toda chamada ao LLM (job `redact` ou `test_redator`) grava uma linha em `llm_calls`: finalidade (`purpose`), provedor, modelo, tokens de entrada e saída, custo (`cost_usd`, só quando o provedor informa, como o `usage.cost` do OpenRouter), duração, resultado (`success`, `timeout`, `rate_limited`, `unavailable`, `rejected` ou `invalid_response`) e um erro curto. O texto, o prompt e a chave nunca são gravados. `list_llm_calls` lista da mais recente à mais antiga e filtra por `since` (RFC 3339) e `purpose`.

```bash
engramind llm-calls list --purpose redact --since 2026-10-31T00:00:00Z
```

## Primeiro acesso

Com o banco vazio, defina `BOOTSTRAP_ADMIN_TOKEN` no `.env` e suba o serviço: o log registra `security.bootstrap_created`. Depois, emita os tokens dos sistemas gestores e guarde o do bootstrap num cofre. Com tokens existentes, o valor é ignorado (`security.bootstrap_ignored`); sem nenhum token e sem o valor, o log avisa `security.bootstrap_missing`.

## Logs

Cada requisição gera um único evento `http.request.completed` com `request_id`, rota parametrizada, `app`, `agent`, `token_id`, `operation`, status e duração. Tokens, segredos e o cabeçalho `Authorization` nunca aparecem. As chamadas MCP geram `mcp.tool.completed` em `DEBUG`, com o nome da ferramenta e o resultado, sem os argumentos.
