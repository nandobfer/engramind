# Canais e assinaturas

As memórias pertencem a **canais**. Um agente só alcança um canal quando o **assina**; o token `admin` alcança todos.

## Canais

- O `slug` é hierárquico, com `/`: `eng/global`, `eng/projeto-atlas`. Cada segmento usa minúsculas, dígitos e hífens simples (sem hífen no início ou no fim); são até 8 segmentos e 120 caracteres.
- O slug é único e **imutável**. `update_channel` muda só `name`, `description` e `priority`.
- `priority` (de -1000 a 1000, padrão 0) é a prioridade padrão na ordem do contexto.
- `version` cresce a cada mudança do canal.
- Só token `admin` cria e mantém canais. Um token `agent` recebe `403 SCOPE_REQUIRED`. Um slug inexistente recebe `404 CHANNEL_NOT_FOUND`: canais nunca nascem implicitamente.
- No caminho REST, `{channel}` aceita o id (`ch_…`) ou o slug com `/` codificada (`/v1/channels/eng%2Fglobal`). A CLI codifica sozinha.

```bash
engramind channels create --slug eng/global --name "Engenharia global"
engramind channels update eng/global --priority 10
engramind channels get eng/global
```

### Arquivar, restaurar e excluir

- `archive_channel` tira o canal do contexto, da busca e das dicas padrão, e também de `list_channels` sem `include_archived`. O canal continua legível quando citado pelo slug.
- `restore_channel` devolve o canal ao uso.
- Um canal arquivado não recebe escrita de memórias (`409 CHANNEL_ARCHIVED`); a leitura e `delete_memory` continuam.
- `delete_channel` só exclui um canal arquivado; um canal ativo recebe `409 CHANNEL_NOT_ARCHIVED`. A exclusão remove o canal, as memórias dele (com as revisões) e as assinaturas **exatas**; as por prefixo continuam. A auditoria guarda o canal e quantas assinaturas (`subscriptions_removed`) e memórias (`memories_removed`) saíram.

```bash
engramind channels archive eng/legado
engramind channels delete eng/legado
```

### Estatísticas

`channel_stats` mostra, para quem tem `read` efetivo no canal, as memórias por status e por categoria, quantas esperam título ou embedding, a soma de `retrieval_count`, a última mudança e a `version` do canal (que cresce a cada mudança de memória).

```bash
engramind channels stats eng/global --json
```

## Assinaturas

| Campo | Significado |
|---|---|
| `agent` | O agente (id ou `app/agente`). |
| `target` | Um canal exato existente (`eng/global`) ou um prefixo terminado em `*`. |
| `mode` | `inject`: entra no contexto, na busca e nas dicas. `search`: entra na busca e nas dicas, não no contexto. `manage`: só é alcançado quando o canal é citado pelo slug. |
| `access` | Restringe os escopos do token (`read` obrigatório). Omitido ou `null`: os escopos do token. |
| `priority` | Opcional. Sobrepõe a prioridade do canal. |

```bash
engramind subscriptions add --agent app-a/desenvolvedor --target "eng/projeto-*" --mode inject --access read
engramind subscriptions list --agent app-a/desenvolvedor
engramind subscriptions update sub_7c2e9a10 --mode search
engramind subscriptions remove sub_7c2e9a10
```

- Assinar o mesmo alvo duas vezes recebe `409 SUBSCRIPTION_EXISTS`. O agente e o alvo não mudam: para trocá-los, cancele e assine de novo.
- Um alvo exato precisa existir (`404 CHANNEL_NOT_FOUND`), mesmo arquivado. Um prefixo não precisa: ele vale também para canais criados depois.
- Cada assinatura traz `reaches`, os canais ativos que o alvo alcança agora. Confira esse campo depois de assinar um prefixo.
- `update_subscription` aceita `access: null` (volta aos escopos do token) e `priority: null` (volta à prioridade do canal).
- Mudanças valem já na requisição seguinte.
- `delete_agent` e `delete_app` removem as assinaturas dos agentes.

### Prefixos

- `eng/*` alcança `eng/x` e `eng/x/y`, mas não `eng`.
- `eng/projeto-*` alcança `eng/projeto-atlas` e `eng/projeto-atlas/docs`, mas não `eng/projetos` nem `eng/global`.
- O prefixo precisa de ao menos um segmento completo e da `/` antes do `*`: `*` e `eng*` recebem `400 VALIDATION_ERROR`. Para alcançar tudo, use um token `admin`.

### Especificidade

Quando várias assinaturas do agente alcançam o mesmo canal, vale a mais específica:

1. a exata vence qualquer prefixo;
2. entre prefixos, o mais longo vence (`eng/projeto-*` vence `eng/*`).

O `mode`, o `access` e a `priority` vêm só dessa assinatura.

## Acesso efetivo

```
acesso efetivo num canal = escopos do token ∩ acesso da assinatura
```

- A assinatura **restringe** e nunca amplia. Um token `read+write` com `access: ["read"]` só lê o canal.
- Um token `agent` sem assinatura no canal recebe `403 CHANNEL_NOT_SUBSCRIBED`, com `details.channel`.
- Quando a assinatura existe mas não dá o escopo, a resposta é `403 SCOPE_REQUIRED`, com `details: { required_kind, required_scope, channel }`.
- Um token `admin` alcança todos os canais em modo `inject`, com os próprios escopos e a prioridade do canal.

`list_channels` e `get_channel` mostram o alcance do token em `reach`: `mode`, `access`, `priority` e a assinatura (`subscription`, `target`) que o define. Para um token `agent`, essas operações mostram só os canais alcançados, inclusive os de modo `manage`. `list_subscriptions` mostra só as assinaturas do próprio agente.

## Seleção por requisição

Um agente que assina `eng/*` pode receber, em cada sessão, só os canais do projeto atual.

- O parâmetro `channels` aceita uma lista de slugs e prefixos separados por vírgula: `channels=eng/global,eng/projeto-atlas`.
- O cabeçalho `Engramind-Channels` define o filtro padrão da conexão no REST e no MCP. Na CLI, o filtro vem de `--channels` ou `ENGRAMIND_CHANNELS`.
- Com os dois presentes, vale a **interseção**: o agente não escapa da política da sessão.
- A seleção só restringe o que o token já alcança, nunca amplia. Um slug inexistente na seleção não dá erro, só não casa com nada.
- Um canal citado fora da seleção recebe `403 CHANNEL_NOT_SUBSCRIBED`.

```bash
ENGRAMIND_CHANNELS=eng/global,eng/projeto-atlas engramind channels list
```

### Modo `manage` e citação explícita

Um canal `manage` só entra quando é **citado pelo slug exato**, no parâmetro `channel` ou numa lista de seleção. Um prefixo não cita. Assim, o agente curador assina os canais-alvo em `manage` com `read+write`. Ele os encontra em `list_channels` e lê cada um quando o cita, mas as memórias desses canais nunca entram no contexto dele.

## Exemplo

| Canal | Assinantes |
|---|---|
| `assistente/geral` | `app-a/assistente` (`inject`, `read+write`) |
| `eng/global` | `app-a/desenvolvedor` (`inject`, `read`), `app-a/revisor` (`inject`, `read`), `app-a/curador` (`manage`, `read+write`) |
| `eng/projeto-*` (prefixo) | `app-a/desenvolvedor` (`inject`, `read`), `app-a/curador` (`manage`, `read+write`) |
| `review/revisor` | `app-a/revisor` (`inject`, `read`), `app-a/curador` (`manage`, `read+write`) |
