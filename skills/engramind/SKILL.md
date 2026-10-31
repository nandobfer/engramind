---
name: engramind
description: Lê, busca e grava memórias duráveis do projeto no Engramind pela CLI `engramind`. Use quando precisar conferir uma regra, restrição, convenção ou valor de configuração já registrado, ou quando descobrir um fato durável que outros agentes devem seguir.
---

# Engramind

O Engramind guarda **memórias**: fatos duráveis e curtos (regras do projeto, restrições, convenções, valores de configuração) organizados em **canais**. Outros agentes vão seguir o que você gravar, então escreva com cuidado.

A CLI já sabe a URL, o token e os canais pela configuração (`ENGRAMIND_URL`, `ENGRAMIND_TOKEN`, `ENGRAMIND_CHANNELS` ou `--profile`). Use sempre `--json` e leia o resultado como JSON.

## Antes de começar

Confira quem você é e quais canais alcança:

```bash
engramind whoami --json
engramind channels list --json
```

Cada canal traz `reach.mode` (`inject`, `search` ou `manage`) e `reach.access` (o que você pode fazer nele: `read`, `write`, `delete`).

Para ler de uma vez as memórias que valem para você (os canais em modo `inject`), peça o contexto. Sem `--json`, sai o bloco pronto, `#id: texto` por canal e categoria, seguido de `#id: título` das que não couberam:

```bash
engramind context
engramind context --json
```

## Quando buscar

Ao ter dúvida sobre uma regra, uma convenção ou um valor, busque antes de decidir:

```bash
engramind memories search testes locais --json
engramind memories search testes locais --exclude-context --json
engramind memories get m_8f2a1c3d --json
```

`--exclude-context` deixa de fora o que você já leu com texto completo no contexto.

A busca é por sentido e por palavras: "rodar todos os testes" encontra "suíte completa". O campo `mode` diz se o ramo por sentido rodou (`hybrid`) ou se só o texto foi usado (`text`). Para uma lembrança rápida, só com ids e títulos:

```bash
engramind hints como rodo os testes --json
```

A busca ignora acentos e caixa. Para ver só os títulos de um canal:

```bash
engramind memories list --fields title --channels eng/global --json
```

## Quando escrever

Grave quando descobrir um fato que continuará verdadeiro por semanas e que outro agente precisaria saber. Uma boa memória:

- tem **um** fato, curto e autossuficiente (quem lê não tem o contexto da sua conversa);
- é genérica: sem dados sensíveis (segredos, tokens, dados pessoais), sem códigos de tarefa e sem URLs temporárias;
- está na categoria certa: `PROJECT_RULES`, `ARCHITECTURE`, `CONSTRAINTS`, `CONFIG_VALUES` ou `NAMING`.

Busque antes de criar. Depois de criar, confira `similar` na resposta: se aparecer uma memória parecida, prefira editá-la e excluir a nova.

```bash
engramind memories create --channel eng/global --category CONSTRAINTS --text "Rode só os testes afetados com cargo nextest; a suíte completa fica no CI." --json
```

- `duplicate: true` na resposta: o texto já existia e nada foi gravado.
- Para texto com várias linhas, passe `--text -` e mande o texto pela entrada padrão.

## Editar

Leia a memória, guarde a `version` e edite com `--expected-version`, para não sobrescrever a mudança de outro agente:

```bash
engramind memories update m_8f2a1c3d --text "Texto revisado" --expected-version 3 --json
engramind memories history m_8f2a1c3d --json
engramind memories revert m_8f2a1c3d --version 2 --json
```

Memória obsoleta: arquive em vez de excluir (`engramind memories archive m_8f2a1c3d`); ela sai do contexto e continua no histórico.

Conferiu uma memória contra o código e ela continua certa? Registre: `engramind memories verify m_8f2a1c3d` grava `verified_at` sem mudar o contexto e tira a marca `stale_candidate` (memória sem uso, que o consolidador pode arquivar). Uma memória com `merged_into` foi fundida em outra: use o id de `merged_into`.

## Erros comuns

Em erro, a saída é `{ "error": { "code", "message", "details" } }` e o código de saída não é 0.

| Código | O que fazer |
|---|---|
| `MEMORY_VERSION_CONFLICT` | Outro agente mudou a memória. Releia com `engramind memories get`, refaça a edição e use o novo `version`. |
| `MEMORY_DUPLICATE` | O texto já existe em `details.existing_id`; use ou edite aquela memória. |
| `CATEGORY_NOT_FOUND` | Use uma categoria de `details.valid`. |
| `VALIDATION_ERROR` com `details.field: "text"` | O texto está curto ou longo demais; veja `details.min` e `details.max`. |
| `SCOPE_REQUIRED` | Seu token ou sua assinatura não permitem a operação nesse canal; não insista. |
| `CHANNEL_NOT_SUBSCRIBED` | Você não alcança esse canal; escolha outro de `engramind channels list`. |
| `CHANNEL_ARCHIVED` | O canal foi arquivado; escreva em outro canal. |
| `MEMORY_NOT_FOUND` | O id não existe ou está num canal que você não alcança. |

Mais detalhes: `engramind docs guide escrever-memorias`.

Importar memórias em lote, configurar webhooks, rodar a consolidação e ver uso e métricas são tarefas do gestor, com token `admin` (guia `integracao-gestor`); um token `agent` recebe `SCOPE_REQUIRED` nelas.
