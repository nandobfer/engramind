# Memórias

Uma memória é um **fato durável e curto** que outros agentes vão seguir: uma regra do projeto, uma restrição, um valor de configuração, uma convenção de nomes. Ela pertence a um **canal**, não ao agente que a escreveu; o autor (`author.app`, `author.agent`) e a `origin` ficam como proveniência.

## Como escrever uma boa memória

- Um fato por memória, que continue verdadeiro por semanas ou meses.
- Genérico e autossuficiente: quem lê não tem o contexto da sua conversa.
- Sem dados sensíveis (segredos, tokens, dados pessoais), sem códigos de tarefa e sem URLs temporárias.
- Na categoria certa (`categories.list`; as padrão são `PROJECT_RULES`, `ARCHITECTURE`, `CONSTRAINTS`, `CONFIG_VALUES` e `NAMING`). Sem categoria, vale a primeira da lista.
- Antes de criar, busque. Depois de criar, olhe `similar`: se já existe uma memória parecida, prefira `update_memory` a ter duas quase iguais.

## Escrever

`create_memory { channel, text, category?, origin?, status? }` exige `write` efetivo no canal. A escrita nunca espera o LLM: a memória nasce na hora com um **título provisório** (a primeira frase, até 100 caracteres, `title_state: pending`), a revisão 1 e a auditoria, e o Redator e o embedding entram numa fila. O Redator troca depois o título provisório por um título de uma linha (`title_state: ready`) e pode revisar o texto numa revisão nova de autor `system/redator`, que o histórico e `revert_memory` preservam; sem Redator configurado, o título provisório fica (`title_state: fallback`).

- `text` precisa ter entre `memory.text_min` e `memory.text_max` caracteres (o máximo pode ser sobreposto por canal); fora disso, `400 VALIDATION_ERROR` com `details.field: "text"`.
- Categoria fora de `categories.list` → `400 CATEGORY_NOT_FOUND` com as válidas em `details.valid`.
- `status` inicial: `active` (padrão) ou `permanent`.
- Canal arquivado → `409 CHANNEL_ARCHIVED`.
- **Duplicata exata**: um texto igual a outra memória do canal (comparado sem diferença de caixa, espaços ou forma Unicode) devolve a memória existente com `duplicate: true` e status `200`, sem gravar nada (`memory.dedupe_exact`).
- **`similar`**: até três memórias do mesmo canal com similaridade de pelo menos `memory.similar_threshold` (de 0 a 1). Até haver embeddings, a nota é a sobreposição dos termos (Jaccard), então só aparecem textos muito parecidos; baixe o limiar se quiser mais sugestões.

```bash
engramind memories create --channel eng/global --category CONSTRAINTS --text "Rode só os testes afetados com cargo nextest; a suíte completa fica no CI." --origin-source curator --json
echo "Texto longo, com várias linhas" | engramind memories create --channel eng/global --text - --json
```

## Editar e reverter

- `update_memory { memory, text?, category?, expected_version? }`. Com `expected_version`, uma versão diferente da atual dá `409 MEMORY_VERSION_CONFLICT` com `details.current_version`: releia e decida, em vez de sobrescrever às cegas.
- Texto novo volta o título e o embedding para `pending`, zera `importance` e reenfileira o Redator. Texto igual a outra memória do canal dá `409 MEMORY_DUPLICATE` com `details.existing_id`. Mudar só a categoria grava uma revisão sem reprocessar. Sem mudança efetiva, nada é gravado.
- `revert_memory { memory, version }` restaura o texto, o título e a categoria de uma versão do histórico numa **revisão nova**; o histórico nunca é reescrito. Versão inexistente → `404 REVISION_NOT_FOUND`. O Redator só refaz o título, sem reescrever o texto restaurado.
- `memory_history` lista as revisões, da mais nova à mais antiga, com autor e motivo (`create`, `update`, `archive`, `restore`, `pin`, `unpin`, `move`, `revert`).

```bash
engramind memories update m_8f2a1c3d --text "Texto revisado" --expected-version 1
engramind memories history m_8f2a1c3d
engramind memories revert m_8f2a1c3d --version 1
```

## Status, mover e excluir

| Operação | Escopo | Efeito |
|---|---|---|
| `archive_memory` | `write` | Sai do contexto e das dicas; continua buscável com `include_archived`. |
| `restore_memory` | `write` | A arquivada volta a `active`. |
| `pin_memory` / `unpin_memory` | `write` | `permanent` entra primeiro no contexto e o consolidador nunca toca; `unpin` volta a `active`. |
| `move_memory` | `write` na origem e no destino | Troca de canal; duplicata no destino → `409 MEMORY_DUPLICATE`. |
| `delete_memory` | `delete` | Apaga a memória e o índice de busca; a auditoria e a última revisão guardam a proveniência. |

Repetir um status que já vale não muda nada. Num canal arquivado, só a leitura e `delete_memory` funcionam (o resto dá `409 CHANNEL_ARCHIVED`).

```bash
engramind memories pin m_8f2a1c3d
engramind memories move m_8f2a1c3d --to eng/projeto-atlas
engramind memories delete m_8f2a1c3d
```

Toda mudança grava a revisão (menos a exclusão), a auditoria (só metadados e o hash do texto, nunca o texto), incrementa a `version` da memória e a do canal, na mesma transação. Mutações aceitam `Idempotency-Key`.

## Ler e listar

- `get_memory` traz o texto, o título, a autoria, a `origin`, os contadores (`retrieval_count`, gravado em lote a cada poucos segundos), os estados e a `version`.
- `list_memories` lista as memórias dos canais que o token alcança, em qualquer modo, da alteração mais recente à mais antiga. Filtros: `channels` (e o cabeçalho `Engramind-Channels`), `categories`, `status` (padrão `active,permanent`), `author` (app ou agente) e `since` (`updated_at`). `fields=title` devolve só `id`, `title`, `channel`, `category` e `status`, para economizar tokens.
- Uma memória num canal que o token não alcança, ou fora do `Engramind-Channels`, responde `404 MEMORY_NOT_FOUND`, como se não existisse.

```bash
engramind memories list --fields title --channels eng/global --json
engramind memories get m_8f2a1c3d --json
```

## Buscar

`search_memories { query, channels?, categories?, limit?, include_archived?, exclude_ids?, exclude_context?, fields? }` faz busca textual sem acentos e sem caixa ("configuracao" encontra "configuração") nos canais em modo `inject` e `search`. Canais `manage` e arquivados só entram quando citados pelo slug exato em `channels` ou no `Engramind-Channels`.

- Padrão `limit=10` (máximo 50); só memórias ativas e fixadas, a menos que `include_archived`.
- `exclude_ids` tira ids escolhidos por você; `exclude_context: true` tira as que já estão com texto completo no contexto do mesmo token e da mesma seleção (os títulos da segunda faixa continuam buscáveis).
- A resposta traz `context_version`, a `version` de `get_context` sem `format` para a mesma seleção: se ela mudou, o bloco do prompt está desatualizado (guia `contexto-e-cache`).
- Cada resultado traz `rank` e `score`; `score` só serve para comparar resultados da mesma busca.
- A busca conta uma recuperação de cada resultado (`retrieval_count`, `last_retrieved_at`).

```bash
engramind memories search testes locais --json
engramind memories search configuracao --fields title --channels eng/global
engramind memories search porta preview --exclude-context
```
