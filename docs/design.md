# Engramind — desenho do projeto

> Rede de memórias para agentes de IA. Um serviço em Rust, com SQLite (WAL), sqlite-vec e FTS5,
> replicado para S3 pelo Litestream. Guarda memórias em **canais**, identifica cada agente por um
> **token** com escopos e entrega tudo por **REST, MCP e CLI** (com skill), com documentação viva.
> Inspirado nos conceitos do Magic Context (CortexKit), adaptados para canais e acesso pela rede.

Status: desenho revisado (out/2026), pronto para planejamento. Nada implementado.

---

## 0. Glossário

| Termo | Significado |
|---|---|
| **Sistema consumidor** | Sistema que executa agentes e entrega a eles as memórias (no prompt, por MCP ou pela CLI). Também pode escrever memórias. |
| **Sistema gestor** | Sistema com tela de administração da rede: canais, memórias, identidades, tokens e configurações. Usa um token `admin`. Um mesmo sistema pode ser gestor e consumidor. |
| **App** | Registro de um sistema consumidor ou gestor na rede (ex.: `app-a`). Agrupa agentes e recebe webhooks. |
| **Agente** | Identidade de um agente dentro de um app (ex.: `app-a/desenvolvedor`). Cada agente tem o próprio token. |
| **Agente curador** | Agente cuja função é escrever e manter memórias **para outros agentes** (ex.: um mentor que transforma lições em regras). As memórias que ele escreve não são dele: pertencem ao canal. |
| **Canal** | Lista de memórias compartilhada (ex.: `eng/global`, `eng/projeto-atlas`, `review/revisor`). |
| **Assinatura** | Ligação entre agente e canal, com um modo (`inject`, `search` ou `manage`) e um acesso. |
| **Redator** | Subagente interno do serviço que gera o título e revisa o texto de cada memória. |
| **Contexto** | Bloco renderizado com as memórias dos canais assinados, pronto para entrar no prompt. |

---

## 1. Objetivo e princípios

- **Centralizar** as memórias de agentes que hoje ficam espalhadas, uma coleção por sistema e por
  papel, e deixar **qualquer sistema** consumir a mesma rede.
- **O canal é a unidade de compartilhamento.** Agentes que assinam o mesmo canal compartilham as
  memórias dele. Uma memória pertence ao canal, não ao agente que a escreveu. O autor fica
  registrado como proveniência.
- **O token identifica e delimita.** Ele diz *qual app* e *qual agente* está falando, e carrega os
  escopos `read`, `write` e `delete`. A política fina (que ferramentas o agente vê e quando ele
  escreve) é decidida pelo **sistema consumidor**. O Engramind fornece o MCP, as ferramentas e
  aplica os escopos.
- **Agentes escrevem direto**, sem fluxo de sugestão e aprovação. Toda escrita guarda
  proveniência e histórico de versões, então nada se perde e tudo pode ser revertido.
- **A curadoria textual é do serviço.** O Redator gera o título e revisa o texto, tirando essa
  função dos sistemas consumidores.
- **Tudo é configurável e consultável por qualquer via** (REST, MCP e CLI): memórias, canais,
  assinaturas, identidades, tokens e configurações, cada um com ferramentas específicas.
- **Documentação viva.** O contrato (OpenAPI) e os guias saem do binário em execução e também são
  servidos por MCP. Testes impedem que a documentação fique para trás do código.
- **Contexto estável para o cache.** O bloco de memórias de um agente é determinístico e só muda
  quando as memórias mudam, preservando o cache de prompt do provedor de LLM.
- **Desacoplado.** O serviço não conhece nenhum sistema consumidor específico. Toda integração
  passa pelos contratos públicos (REST, MCP, CLI e webhooks).

### Fora do escopo

- Não é um indexador RAG de código nem de documentos. Guarda fatos duráveis e curtos.
- Não executa agentes nem conversa com o usuário. Só armazena, busca, renderiza, redige e consolida.
- Não extrai memórias de conversas. Isso é papel dos sistemas consumidores (ex.: um agente curador).
- Uma instância só, que é o limite consciente do SQLite (ver §15).

---

## 2. Conceitos absorvidos do Magic Context

| Magic Context | Engramind |
|---|---|
| Memória de **projeto**, com identidade derivada do repositório | Memória de **canal**. Um agente assina vários canais. |
| **Workspaces** compartilham categorias entre repositórios, só leitura | **Assinatura** com modo e acesso, e escopos no token |
| `<project-memory>` injetado **a cada turno**, com **orçamento de tokens** | `get_context` com orçamento configurável e prioridade por canal e categoria |
| Renderização **em decaimento** (o antigo perde fidelidade em vez de sumir) | Duas faixas: texto completo até um orçamento, depois só título com `#id`, depois uma linha de excedente |
| Injeção **estável para o cache** | Renderização determinística, `version`/ETag e `304`. Classificação e contadores não mudam a ordem até a próxima mudança real. |
| `ctx_search` híbrido (embeddings + texto) | `search_memories`: sqlite-vec + FTS5 (BM25) com fusão RRF, só nos canais acessíveis |
| **Dicas automáticas** ("lembrança vaga") | `get_hints`: fragmentos curtos (`#id · título · canal`), nunca o texto todo |
| Ids visíveis no bloco para as ações continuarem funcionando | `#m_…` no contexto, usado por `update_memory`, `archive_memory` etc. |
| Categorias fixas (`PROJECT_RULES`, `ARCHITECTURE`, `CONSTRAINTS`, `CONFIG_VALUES`, `NAMING`) | Categorias **configuráveis** (padrão: as mesmas cinco), cada uma com prioridade |
| Status `active`, `permanent` e `archived` | Os mesmos três. Excluir de vez é uma ação separada, com escopo `delete`. |
| **Contagem de recuperação** (memória útil sobrevive à curadoria) | `retrieval_count`, `injected_count` e `last_retrieved_at` |
| **Dreamer**: curate, classify e arquivamento de memórias inúteis | **Consolidador** opcional: duplicatas, importância e candidatas por desuso |
| Editar uma memória zera a classificação e volta o compartilhamento a privado | Editar zera `importance` e reprocessa título e embedding |
| Historiador extrai fatos da conversa | Fica nos sistemas consumidores |
| `verify` confere a memória contra o código | Fica nos consumidores. O serviço oferece `mark_verified` para registrar a conferência (fase 4). |
| SQLite local compartilhado pelos harnesses da máquina | SQLite **atrás do serviço**. Os clientes acessam pela rede. |

---

## 3. Arquitetura

```
 ┌────────────────────┐  ┌──────────────────────┐  ┌────────────────────┐
 │ Sistema gestor     │  │ Sistemas consumidores│  │ Agente com shell   │
 │ (tela de admin)    │  │ (agentes via MCP     │  │ (CLI `engramind` +     │
 │                    │  │  ou via REST)        │  │  skill)            │
 └─────────┬──────────┘  └──────────┬───────────┘  └─────────┬──────────┘
           │ REST + webhooks        │ MCP / REST             │ REST
           ▼                        ▼                        ▼
 ┌─────────────────────────────────────────────────────────────────────┐
 │ engramind (binário Rust: axum + tokio)                            │
 │  ├─ auth: Bearer token → Principal { app, agent?, kind, scopes }    │
 │  ├─ adaptador REST (utoipa → OpenAPI + Swagger UI)                  │
 │  ├─ adaptador MCP (rmcp, Streamable HTTP, stateless)                │
 │  ├─ core: memories · channels · subscriptions · identities ·       │
 │  │        settings · context · search · audit                       │
 │  ├─ workers (tokio): redator · embeddings · consolidador · webhooks │
 │  └─ docs: guias embutidos + índice do OpenAPI                       │
 └───────────────────────────────┬─────────────────────────────────────┘
                                 │ rusqlite (WAL) + sqlite-vec + FTS5
                                 ▼
                     /data/engramind.db ──Litestream──▶ S3
```

- **Uma única camada de domínio** (`core`). REST, MCP e CLI são adaptadores finos sobre ela. As três
  vias têm as mesmas regras, validações, escopos e códigos de erro.
- O armazenamento fica atrás de uma interface (`Store`). Hoje ela tem uma implementação em SQLite.
  Uma migração futura para Postgres + pgvector troca só essa implementação.
- A **CLI é o mesmo binário** (`engramind <comando>`) agindo como cliente HTTP da API. Ela nunca abre o
  banco, exceto nos comandos de operação do próprio servidor (`engramind serve`, `engramind migrate` e
  `engramind doctor`).

### Stack

| Função | Escolha |
|---|---|
| HTTP | `axum` 0.8 + `tokio` + `tower-http` (timeout, limite de corpo, tracing, request id) |
| Banco | `rusqlite` (`bundled`) + crate `sqlite-vec` (registrada com `sqlite3_auto_extension`) + FTS5. Uma thread escritora dedicada e um pool de leitores. |
| Migrações | SQL versionado em `migrations/`, embutido no binário (`rusqlite_migration`) |
| Vetores | `sqlite-vec` (ver §9) |
| Embeddings | Configurável: local (`fastembed`, ONNX) ou remoto (OpenAI-compatível ou Ollama). Ver §11. |
| LLM do Redator | Cliente `reqwest` próprio para provedores OpenAI-compatíveis (OpenAI, DeepSeek, OpenRouter, Ollama…), Anthropic e Gemini |
| Contagem de tokens | `tiktoken-rs` (`o200k_base`) como estimador padrão, configurável |
| OpenAPI | `utoipa` 5 + `utoipa-swagger-ui` (vendored) |
| MCP | `rmcp` (servidor Streamable HTTP, stateless) |
| CLI | `clap`, com saída `--json` em todo comando |
| Erros e logs | `thiserror`, `tracing` + `tracing-subscriber` (JSON ou compacto) |
| Métricas | `metrics` + `metrics-exporter-prometheus` |
| Segredos | `zeroize` para os segredos em memória. Os segredos de configuração são cifrados em repouso (AES-GCM). |
| Backup | Litestream na própria imagem → S3 |

Lints iguais aos do sap-middleman (`print_stdout`/`print_stderr`/`dbg_macro` negados). Uma exceção:
`unsafe_code` passa de `forbid` para `deny`. O registro do `sqlite-vec` exige um único bloco
`unsafe`, isolado num módulo com `#[allow(unsafe_code)]` e comentário de justificativa.

---

## 4. Identidade, tokens e escopos

### Tipos de token

| `kind` | Para quê | Pertence a |
|---|---|---|
| `agent` | Operar memórias nos canais assinados pelo agente | um agente |
| `admin` | Gerir a rede pelas ferramentas administrativas: apps, agentes, tokens, canais, assinaturas, configurações, jobs e auditoria. Também acessa todas as memórias. | um app (o sistema gestor) e, opcionalmente, um agente |

Um token `admin` pode ser vinculado a um agente. É assim que um agente administrativo (ex.: o
assistente de um sistema gestor) cria canais e assinaturas pelas ferramentas administrativas, com a
autoria registrada no próprio agente. Um token `agent` nunca alcança essas ferramentas.

### Escopos

Todo token, de qualquer tipo, tem um conjunto de escopos:

| Escopo | Permite |
|---|---|
| `read` | Ler o contexto, listar, buscar, consultar histórico e documentação. Num token `admin`, também ler configurações, identidades e auditoria. |
| `write` | Criar, editar, arquivar, restaurar, fixar, mover e reverter. Num token `admin`, também mudar configurações, canais, assinaturas e identidades. |
| `delete` | Excluir de vez memórias. Num token `admin`, também excluir canais, apps e agentes e revogar tokens. |

- `write` e `delete` exigem `read`. Combinações válidas: `read`, `read+write`, `read+delete` e
  `read+write+delete`.
- Arquivar é `write`, porque é reversível. Excluir é `delete`, porque é definitivo.
- Um token `admin` só de leitura serve para painéis e auditoria.

### Formato e guarda

- Formato: `eng.<token_id>.<segredo>`, no mesmo esquema do sap-middleman. O `token_id` localiza o
  registro, e o segredo é comparado em tempo constante com o hash (SHA-256 com o pepper
  `TOKEN_PEPPER`).
- O segredo aparece **uma única vez**, na emissão. Depois só o prefixo é mostrado.
- Os tokens têm `expires_at` opcional, `last_used_at` (atualizado em lote) e revogação imediata.
- O primeiro token `admin` nasce de `BOOTSTRAP_ADMIN_TOKEN`, aceito só com o banco vazio.

### Acesso efetivo

```
acesso efetivo num canal = escopos do token ∩ acesso da assinatura
```

- Um token `agent` só alcança os canais que o agente assina (`CHANNEL_NOT_SUBSCRIBED`, 403).
- A assinatura pode **restringir** o acesso. Exemplo: o revisor assina `eng/global` só com `read`,
  mesmo que o token dele tenha `write`. A assinatura nunca amplia o que o token permite.
- Um token `admin` alcança todos os canais, limitado pelos próprios escopos.

### Ferramentas visíveis por token

O MCP lista em `tools/list` **só as ferramentas que o token pode usar**:
- um token `read` não vê `create_memory`;
- um token `agent` não vê as ferramentas de gestão.

Isso reduz o esquema carregado no contexto do agente e torna o escopo autoexplicativo. O sistema
consumidor ainda pode filtrar mais (ex.: expor só `search_memories`).

---

## 5. Canais e assinaturas

### Canal

- `slug` hierárquico com `/` (ex.: `eng/global`, `eng/projeto-atlas`), nome, descrição, prioridade
  padrão, sobreposições de configuração (§12) e `version`, que cresce a cada mudança.
- Canais são criados e mantidos só pelas **ferramentas administrativas** (token `admin`): pela tela
  do sistema gestor ou por um agente com token `admin` vinculado a ele. Um token `agent` não cria
  canais (`SCOPE_REQUIRED`). Escrever num slug inexistente devolve `CHANNEL_NOT_FOUND`; o canal não
  nasce implicitamente.

### Assinatura

| Campo | Significado |
|---|---|
| `target` | Um canal exato (`eng/global`) ou um **prefixo** (`eng/*`). O prefixo vale para canais criados depois. |
| `mode` | `inject`: entra no contexto, na busca e nas dicas. `search`: entra na busca e nas dicas, não no contexto. `manage`: fica fora do contexto, das dicas e da busca padrão, e só é alcançado quando o canal é citado de forma explícita. |
| `access` | Subconjunto de `read`/`write`/`delete` (padrão: o do token) |
| `priority` | Opcional. Sobrepõe a prioridade do canal na ordem do contexto. |

Quando um canal é alcançado por mais de uma assinatura (ex.: `eng/*` e `eng/global`), vale a mais
específica.

### Seleção por requisição

Um mesmo agente pode atender a vários projetos. Um token de desenvolvedor, por exemplo, assina
`eng/*`, mas cada sessão dele deve receber só `eng/global` e o canal do projeto em que está.

- Toda operação aceita `channels` (lista de slugs ou prefixos) para restringir aos canais desejados,
  sempre dentro do que o token já alcança.
- No MCP e na CLI, o cabeçalho `Engramind-Channels: eng/global,eng/projeto-atlas` (ou
  `ENGRAMIND_CHANNELS`) define o filtro padrão de toda a conexão. O sistema consumidor registra o MCP
  por sessão com esse cabeçalho, e o agente não precisa saber quais canais escolher.

### Agente curador

O agente curador escreve memórias **para outros agentes**. A recomendação é que ele **assine os
canais-alvo em modo `manage`** com `read+write`.

| Alternativa | Problema |
|---|---|
| Manipular sem assinar, com token `admin` | Daria ao curador poder sobre identidades, tokens e configurações, muito além da função dele |
| Uma flag "escreve em qualquer canal" | Perde o mapa explícito de quem alcança cada canal e não permite restringir canal a canal |
| **Assinar em modo `manage`** ✅ | Fica visível e revogável na tela de assinaturas. O acesso pode ser limitado por canal. As memórias dos canais-alvo não poluem o contexto do próprio curador. |

Com isso, o fluxo de um curador fica assim:
1. `list_channels`, para saber os destinos possíveis;
2. `list_memories { channel, fields: "full" }` no canal escolhido, e
   `list_memories { channels, fields: "title" }` nos demais, para comparar sem gastar contexto;
3. `create_memory` ou `update_memory` com `origin` (ex.: `{ source: "curator", label: "caso 123" }`).

O autor registrado é o curador, mas a memória pertence ao canal.

---

## 6. Modelo de dados (SQLite)

```sql
apps          (id, slug, name, description, webhook_url, webhook_secret_enc,
               created_at, disabled_at)
agents        (id, app_id, slug, name, description, created_at, disabled_at)
tokens        (id, app_id, agent_id NULL, kind, scopes, label, secret_hash,
               expires_at NULL, created_at, last_used_at, revoked_at)

channels      (id, slug UNIQUE, name, description, priority, settings_json,
               version, created_at, archived_at)
subscriptions (id, agent_id, target, is_prefix, mode, access, priority NULL, created_at)

memories      (id, channel_id, category, title, text, status, importance NULL,
               content_hash, version, title_state, embedding_state,
               author_app_id, author_agent_id, origin_source, origin_label,
               retrieval_count, injected_count, last_retrieved_at, verified_at NULL,
               merged_into NULL, stale_candidate, created_at, updated_at, archived_at)
memory_revisions (id, memory_id, version, title, text, category, status,
                  app_id, agent_id, reason, created_at)
memory_fts    -- FTS5 externa (title, text), tokenizer unicode61 remove_diacritics 2,
              -- sincronizada por trigger
memory_vectors (memory_id PK, model_id, dims, embedding BLOB)  -- formato float32 do sqlite-vec

settings      (key PK, value_json, updated_by_app, updated_at)
secrets       (key PK, value_enc, updated_at)          -- api keys do Redator e dos embeddings
jobs          (id, kind, payload_json, state, attempts, run_after, locked_until,
               last_error, created_at, finished_at)
idempotency   (token_id, key, request_hash, response_json, created_at)
audit_log     (id, at, app_id, agent_id, token_id, action, entity, entity_id,
               before_json, after_json)
llm_calls     (id, at, purpose, provider, model, input_tokens, output_tokens,
               cost_usd NULL, duration_ms, outcome, error)
usage_log     (id, at, agent_id, op, channel_ids, memory_ids, rendered_tokens)
```

Regras gerais:
- Os ids públicos são curtos, estáveis e com prefixo de tipo: `m_8f2a1c`, `ch_…`, `ag_…`, `app_…`, `tk_…`.
- Todo `create`, `update`, `archive`, `restore`, `pin`, `move`, `revert` e `delete` faz quatro coisas:
  - grava `memory_revisions` (exceto `delete`, que deixa a última revisão);
  - grava `audit_log`;
  - incrementa `memories.version` e `channels.version`;
  - enfileira o webhook.
- `jobs` é uma fila em tabela, sem broker. Os workers tokio pegam trabalho com
  `UPDATE … RETURNING` e `locked_until`, com backoff exponencial.
- Os contadores (`retrieval_count`, `injected_count`, `last_used_at`) são acumulados em memória e
  gravados em lote a cada poucos segundos, para não disputar o escritor.
- Retenção configurável: `usage_log` e `llm_calls` (padrão de 90 dias), `idempotency` (24 h).

---

## 7. Ciclo de vida das memórias

### Escrita (`create_memory`)

1. Entrada: `{ channel, text, category?, origin?, status? }` e o cabeçalho opcional `Idempotency-Key`.
2. Validações:
   - escopo `write` no canal;
   - tamanho do texto (`memory.text_min`/`text_max`);
   - a categoria precisa existir.
3. **Duplicata exata** (`content_hash` normalizado) no canal devolve a memória existente com
   `duplicate: true`, sem criar outra.
4. Persiste com **título provisório** (a primeira frase, truncada) e `title_state = pending`, grava
   a revisão 1 com o texto original, gera auditoria, sobe a versão do canal e enfileira os jobs
   `redact` e `embed`.
5. Responde na hora, porque a escrita nunca espera o LLM. A resposta traz `similar`: até três
   memórias do mesmo canal com similaridade acima de `memory.similar_threshold`, pelo FTS na hora e
   pelo vetor quando já existir. Com isso o agente pode preferir `update_memory` a uma quase-duplicata.

### Redator

O job `redact` envia o texto ao Redator, que devolve `{ title, text }`:
- o **título** é uma linha (`redator.title_max`, padrão de 100 caracteres);
- o **texto revisado** é mais claro e conciso, com o mesmo sentido. A revisão do texto pode ser
  desligada com `redator.rewrite_text = false`; nesse caso o Redator só gera o título.

Proteções:
- O texto vai como **dado**, entre delimitadores, com a instrução de não seguir nada do conteúdo.
- A resposta é validada por esquema JSON e por limites de tamanho.
- **Guarda de sentido.** Se a similaridade de embedding entre o original e o revisado ficar abaixo
  de `redator.min_similarity`, o texto original é mantido e só o título é aplicado.

Resultado:
- Se o texto mudar, nasce uma nova revisão com autor `system/redator`, e o original continua no
  histórico.
- Se o Redator estiver desligado, falhar todas as tentativas ou não estiver configurado, a memória
  recebe `title_state = fallback` e mantém o texto como foi escrito.

Toda chamada fica registrada em `llm_calls`, com tokens, custo e duração.

### Embeddings

- O job `embed` gera o vetor de `title + "\n" + text` e grava `memory_vectors` com o `model_id`.
- Trocar o modelo enfileira `reembed_all`, com progresso em `get_status`. Enquanto isso, a busca
  usa só os vetores do modelo atual e o FTS.

### Edição e concorrência

- `update_memory { id, text?, category?, expected_version? }`. Com `expected_version`, uma versão
  diferente devolve `409 MEMORY_VERSION_CONFLICT` com a versão atual, para dois agentes não se
  sobrescreverem às cegas.
- A edição refaz os passos 3–5, zera `importance` e reprocessa título e embedding.
- `move_memory` exige `write` nos dois canais.
- `revert_memory { id, version }` cria uma nova revisão com o conteúdo antigo. Nunca reescreve o
  histórico.

### Status

| Ação | Escopo | Efeito |
|---|---|---|
| `archive_memory` | `write` | Sai do contexto e das dicas. Continua buscável com `include_archived`. |
| `restore_memory` | `write` | Volta a `active` |
| `pin_memory` / `unpin_memory` | `write` | `permanent`: entra primeiro no contexto, e o consolidador nunca toca |
| `delete_memory` | `delete` | Apaga a memória, o vetor e o FTS. A auditoria e a última revisão guardam a proveniência. |

---

## 8. Contexto (`get_context`)

### Por que esse formato

O formato padrão é **linhas compactas `#id: texto`, agrupadas por canal e categoria em tags curtas**,
o mesmo princípio do `<project-memory>` do Magic Context. É o mais eficiente para os agentes e para
o serviço:

| Formato | Tokens | Ids para agir | Leitura pelo modelo | Uso |
|---|---|---|---|---|
| **`tagged`** (padrão) | Mínimo: a tag aparece uma vez por grupo, e cada linha tem só `#id: texto` | Sim | Ótima: as tags dão as fronteiras | Prompt de agentes |
| `markdown` | Parecido, sem ids | Não | Boa | Agentes sem ferramentas de escrita |
| `json` | 2 a 3 vezes mais: aspas, chaves e nomes de campo em toda memória | Sim | Pior | Sistemas que montam o próprio bloco |

- **Autoria fora do contexto.** App, agente e origem custariam tokens em toda linha e quase nunca
  mudam a decisão do agente. Ficam disponíveis em `get_memory`.
- **Para o serviço**, renderizar é concatenar strings. O bloco fica num **cache em memória**
  (LRU) com a chave `(token, canais selecionados, versões dos canais, hash das configurações de
  renderização)`. Sem mudança, `get_context` não toca o banco além de conferir as versões. Com
  `If-None-Match`, responde `304` sem corpo.

Exemplo (`tagged`):

```
<memories version="cv_3f9e1a">
<channel slug="eng/global">
<CONSTRAINTS>
#m_8f2a1c: Nunca rodar a suíte completa de testes localmente; só os testes afetados.
#m_1b7d0e: Segredos de serviços externos nunca ficam no código nem no repositório.
</CONSTRAINTS>
<NAMING>
#m_44c2aa: Nomes de comandos da CLI usam kebab-case.
</NAMING>
</channel>
<channel slug="eng/projeto-atlas">
<ARCHITECTURE>
#m_90ee12: Toda chamada externa passa pelo cliente central com timeout e retry.
</ARCHITECTURE>
</channel>
<titles>
#m_77aa01: Prefixo de versão nos commits de release
#m_77aa02: Porta padrão do ambiente de preview
</titles>
<overflow count="12">use search_memories para o restante</overflow>
</memories>
```

### Montagem

1. Junta as memórias `active` e `permanent` dos canais em modo `inject` alcançados pelo token,
   filtrados por `channels`/`Engramind-Channels`.
2. Ordena de forma **determinística**:
   1. prioridade da assinatura ou do canal;
   2. `permanent` primeiro;
   3. prioridade da categoria;
   4. `importance`, só se já classificada;
   5. `created_at`;
   6. `id`.
3. **Decaimento em duas faixas**, inspirado no Magic Context:
   - texto completo até `context.budget_tokens` (padrão 4.000);
   - em seguida, só `#id: título` até `context.titles_budget_tokens` (padrão 800);
   - o que sobrar vira a linha `overflow` com a contagem.
4. Devolve `{ content, version, counts: { full, titles, overflow }, rendered_tokens }`.
   `injected_count` é incrementado em lote.

### Como o sistema consumidor deve usar

- Coloque o contexto na **parte estável do prompt** (instruções de sistema ou entrada durável da
  sessão) e só o regrave quando `version` mudar. Assim o prefixo em cache sobrevive entre turnos.
- Coloque as **dicas** (§9) no fim do turno, junto da mensagem do usuário, nunca no prefixo.
- Consulte `get_context` com `If-None-Match` no início de cada turno, ou reaja ao webhook (§16).

---

## 9. Busca e dicas

### `search_memories`

Parâmetros: `{ query, channels?, categories?, limit?, include_archived?, exclude_ids?, fields? }`.

1. **Vetorial**: o embedding da consulta é comparado com `vec_distance_cosine` do sqlite-vec sobre
   as linhas filtradas por canal, categoria e status.
   - No volume esperado (milhares a dezenas de milhares de memórias), a varredura exata com filtro
     leva poucos milissegundos e aceita qualquer filtro.
   - Se o volume crescer, a etapa troca para uma tabela `vec0` com `channel_id` como chave de
     partição (índice KNN), sem mudar o contrato.
2. **Textual**: FTS5 com BM25 sobre título e texto (sem acentos, `unicode61 remove_diacritics 2`).
3. **Fusão RRF**: `score = Σ peso / (rrf_k + posição)`, com pesos configuráveis. Sem embeddings
   (desligados ou ainda pendentes), a busca usa só o FTS.
4. `exclude_ids` tira o que já está no contexto. A resposta traz o `version` do contexto, e o
   consumidor pode passar `exclude_context: true` para o serviço excluir sozinho.
5. Incrementa `retrieval_count` e `last_retrieved_at` (em lote).

### `get_hints`

Parâmetros: `{ text, channels?, limit? (3) }`.

- Faz a mesma busca com um limiar alto (`search.hint_threshold`), só em canais `inject` e `search`,
  e exclui o que já está no contexto.
- Devolve só `#id · título · canal`. É a "lembrança vaga": o agente busca o texto completo se quiser.
- Feita para ser chamada pelo sistema consumidor antes do turno, de forma barata e sem LLM.

---

## 10. Consolidação (opcional)

O consolidador é agendado por configuração (ex.: diário, de madrugada), roda por canal e usa a
mesma fila de jobs. Tarefas:

- **Duplicatas.** Pares com similaridade acima de `consolidation.duplicate_threshold` vão ao Redator,
  que decide a fusão e o texto final. A memória mais antiga recebe o texto fundido, e a outra vira
  `archived` com `merged_into`. Tudo gera revisão e auditoria.
- **Importância.** O Redator dá uma nota de 0 a 1 em lote. A nota só afeta a ordem de corte do
  orçamento.
- **Desuso.** Memórias sem recuperação e sem injeção recente há mais de
  `consolidation.stale_after_days` recebem `stale_candidate = true`. Elas só são arquivadas
  automaticamente se `consolidation.auto_archive_stale = true`.
- Nunca mexe em memórias `permanent`, nem em canais com `consolidation.enabled = false`.
- `run_consolidation { channel? , tasks? , dry_run? }` executa sob demanda. O `dry_run` devolve o
  que seria feito.

---

## 11. Embeddings: local ou remoto

Para buscar por **sentido**, e não só por palavras, cada memória vira um vetor de números. Isso é o
*embedding*, e quem gera o vetor é um modelo. Memórias parecidas ficam com vetores próximos, e por
isso "rodar todos os testes" encontra "suíte completa" mesmo sem palavras em comum. O modelo pode
rodar em dois lugares:

| | Local, dentro do contêiner | Remoto, por API |
|---|---|---|
| Como | `fastembed` (ONNX) roda o modelo na CPU do serviço | O serviço chama uma API (OpenAI `text-embedding-3-small`, Ollama…) |
| Custo | Zero por chamada | Centavos por milhão de tokens (memórias são curtas: quase nada) |
| Dependência | Nenhuma externa; funciona offline | Rede, chave de API e disponibilidade do provedor |
| Peso | Imagem maior e um modelo em volume. O `multilingual-e5-small` ocupa de ~120 MB a ~470 MB conforme a quantização, e usa ~200–500 MB de RAM. | Imagem leve |
| Latência | ~5–30 ms por memória na CPU | ~100–300 ms por chamada |
| Privacidade | O texto não sai do serviço | O texto vai para o provedor |
| Qualidade em pt-BR | Boa com um modelo **multilíngue** (modelos só em inglês, como o `all-MiniLM-L6-v2` do Magic Context, são ruins em português) | Muito boa |

- **Inicialização:** o serviço nasce com `embedding.provider = "local"` e o modelo
  `multilingual-e5-small` quantizado. As memórias não saem do serviço, não há custo, e a qualidade
  basta para frases curtas.
- **Troca pela configuração** (REST, MCP ou CLI, token `admin`), a qualquer momento:

  | `embedding.provider` | Campos usados | Exemplos |
  |---|---|---|
  | `local` | `model` (um dos modelos suportados pelo `fastembed`) | `multilingual-e5-small`, `multilingual-e5-base`, `bge-m3` |
  | `openai_compatible` | `base_url`, `model`, `api_key`🔒, `dimensions` opcional | Qualquer serviço com `POST /v1/embeddings` no formato da OpenAI: OpenAI, OpenRouter (vários modelos de embedding de vários provedores numa chave só), Mistral, Jina, Together, LM Studio, vLLM |
  | `ollama` | `base_url`, `model` | `nomic-embed-text`, `bge-m3` |
  | `gemini` | `model`, `api_key`🔒 | `gemini-embedding-001` |
  | `off` | — | desliga a busca por sentido |

- **Só modelos de embedding servem.** Modelos de chat (Claude, GPT, DeepSeek Chat…) geram texto, não
  vetores, e não podem ser usados aqui. Provedores que não oferecem endpoint de embeddings (ex.:
  Anthropic, DeepSeek) ficam de fora, a não ser por um roteador como o OpenRouter. O provedor e o
  modelo de embedding são independentes do Redator: é normal o Redator usar um modelo de chat de um
  provedor e os embeddings usarem outro.
- **Prefixos por modelo.** Alguns modelos esperam marcar o que é consulta e o que é documento
  (`query:`/`passage:` na família e5, `input_type` em outros). A configuração tem
  `query_prefix`/`document_prefix` (preenchidos automaticamente para os modelos locais conhecidos).
- `list_embedding_models { provider, base_url?, api_key? }` lista os modelos disponíveis quando o
  provedor oferece a listagem (local, Ollama, OpenRouter), para a tela do sistema gestor montar o seletor.
- `test_embedding` valida a configuração proposta antes de salvar: chama o provedor, devolve a
  dimensão detectada e a latência, e recusa salvar se falhar.
- Cada modelo tem uma dimensão fixa. Salvar outro provedor ou modelo enfileira `reembed_all`
  (reindexação de todas as memórias, com progresso em `get_status`). Até terminar, a busca usa os
  vetores do modelo anterior ainda válidos e o FTS, sem misturar dimensões.
- Com o provedor local, o modelo é baixado no primeiro uso para `MODELS_PATH` (volume `/data`), e um
  modelo local que não estiver em uso é removido do disco depois da reindexação.
- Com `off`, a busca textual e o contexto continuam funcionando normalmente.

---

## 12. Configurações

Todas as configurações são lidas e escritas por REST, MCP e CLI (token `admin`), têm valor padrão
global e, quando indicado, sobreposição **por canal**.

| Grupo | Chaves (padrão) | Por canal |
|---|---|---|
| `context` | `budget_tokens` (4000), `titles_budget_tokens` (800), `default_format` (`tagged`), `token_estimator` (`o200k`), `overflow_line` (true) | budget, titles_budget |
| `categories` | lista `{ key, label, priority }` (as 5 do Magic Context) | — |
| `memory` | `text_min` (3), `text_max` (4000), `dedupe_exact` (true), `similar_threshold` (0.85) | text_max |
| `redator` | `enabled` (true), `provider`, `model`, `api_key`🔒, `base_url`, `instructions`, `rewrite_text` (true), `title_max` (100), `min_similarity` (0.80), `timeout_ms` (30000), `max_attempts` (3), `fallback_models[]` | instructions, rewrite_text |
| `embedding` | `provider` (`local`; ou `openai_compatible`, `ollama`, `gemini`, `off`), `model` (`multilingual-e5-small`), `base_url`, `api_key`🔒, `dimensions`, `query_prefix`, `document_prefix`, `batch_size` (32), `timeout_ms` (15000) | — |
| `search` | `vector_weight` (1.0), `text_weight` (1.0), `rrf_k` (60), `default_limit` (10), `hint_threshold` (0.80), `hint_limit` (3) | — |
| `consolidation` | `enabled` (false), `schedule` (cron, `0 3 * * *`), `tasks[]`, `duplicate_threshold` (0.92), `stale_after_days` (90), `auto_archive_stale` (false) | enabled |
| `webhooks` | `enabled` (true), `timeout_ms` (5000), `max_attempts` (5) | — |
| `retention` | `usage_days` (90), `llm_calls_days` (90), `audit_days` (null = sempre) | — |
| `limits` | `rate_per_minute` por token (600), `max_batch` (100) | — |

- Os segredos (🔒) só são gravados. A leitura mostra `{ configured: true, updated_at }`.
- `test_redator { text }` e `test_embedding { text }` usam a configuração atual (ou uma proposta,
  ainda não salva) e devolvem o resultado e a latência.
- Toda mudança de configuração gera auditoria (sem segredos) e webhook `settings.updated`.
- As **variáveis de ambiente** cuidam só da infraestrutura: porta, caminho do banco, pepper, chave de
  cifra, Litestream e hosts do MCP. Todo o comportamento funcional (Redator, embeddings, orçamento)
  vive nas configurações acima, gerenciadas pela API.

---

## 13. Superfície: REST, MCP e CLI

Cada operação existe nas três vias com o mesmo nome base, os mesmos parâmetros e os mesmos erros.

| Grupo | Operação | Token / escopo |
|---|---|---|
| **Contexto** | `get_context`, `get_hints` | agent·read |
| **Memórias** | `list_memories`, `get_memory`, `search_memories`, `memory_history` | agent·read |
| | `create_memory`, `update_memory`, `archive_memory`, `restore_memory`, `pin_memory`, `unpin_memory`, `move_memory`, `revert_memory` | agent·write |
| | `delete_memory` | agent·delete |
| **Canais** | `list_channels`, `get_channel` (agent: só os alcançados), `channel_stats` | agent·read |
| | `create_channel`, `update_channel`, `archive_channel`, `restore_channel` | admin·write |
| | `delete_channel` | admin·delete |
| **Assinaturas** | `list_subscriptions` (agent: as próprias) | agent·read |
| | `subscribe`, `update_subscription`, `unsubscribe` | admin·write |
| **Identidades** | `whoami` | qualquer token |
| | `list_apps`, `get_app`, `list_agents`, `get_agent`, `list_tokens` | admin·read |
| | `create_app`, `update_app`, `create_agent`, `update_agent`, `issue_token`, `update_token` | admin·write |
| | `revoke_token`, `delete_app`, `delete_agent` | admin·delete |
| **Configurações** | `get_settings`, `get_channel_settings` | admin·read |
| | `update_settings`, `update_channel_settings`, `set_secret`, `test_redator`, `test_embedding`, `list_embedding_models` | admin·write |
| **Operação** | `get_status`, `list_jobs`, `get_audit`, `get_usage`, `list_llm_calls` | admin·read |
| | `retry_job`, `run_consolidation`, `reembed_all`, `import_memories` | admin·write |
| **Documentação** | `get_overview`, `list_guides`, `get_guide`, `search_docs`, `list_operations`, `get_operation`, `get_schema` | qualquer token (guias de admin ocultos para agent) |

Observações:
- `import_memories` faz importação em lote, preservando `created_at`, autor e `origin`, com
  `Idempotency-Key`. Serve para trazer memórias de sistemas existentes.
- Operações de lista são paginadas (`cursor`, `limit`) e aceitam `fields` (`title` ou `full`) para
  economizar tokens.
- Erros seguem um envelope único, `{ error: { code, message, details? } }`, com `code` estável em
  inglês (`CHANNEL_NOT_SUBSCRIBED`, `SCOPE_REQUIRED`, `MEMORY_VERSION_CONFLICT`…) e `message` em pt-BR.

### REST

- Rotas versionadas em `/v1`: `GET /v1/context`, `POST /v1/memories`, `POST /v1/memories/search`,
  `GET /v1/channels`, `PUT /v1/settings`…
- Fora de `/v1`: `GET /health/live`, `GET /health/ready` (banco e migrações), `GET /metrics`,
  `GET /openapi.json`, `GET /docs` e `POST /mcp`.

### MCP

- `POST /mcp`, Streamable HTTP **stateless**: cada POST leva uma mensagem JSON-RPC e recebe
  `application/json`, como no sap-middleman.
- As ferramentas levam anotações `read_only_hint`, `destructive_hint` e `idempotent_hint` e são
  **filtradas pelo token** (§4).
- **Resources**:
  - `engramind://context`, que é o contexto do token com o filtro da conexão;
  - `engramind://channels/{slug}`, que lista as memórias do canal;
  - `engramind://guides/{id}`, que são os guias.
- **Prompts**: `memory-writing`, um guia curto de como escrever uma boa memória, para o consumidor
  anexar ao agente.
- Proteção contra DNS rebinding por `MCP_ALLOWED_HOSTS`, como no sap-middleman.

### CLI

- `engramind context --format tagged`, `engramind memories search "testes locais"`,
  `engramind memories create --channel eng/global --category CONSTRAINTS --text "…"`,
  `engramind settings set redator.model …`, `engramind tokens issue --agent app-a/curador --scopes read,write`.
- Configuração: `ENGRAMIND_URL`, `ENGRAMIND_TOKEN`, `ENGRAMIND_CHANNELS` ou `~/.config/engramind/config.toml`
  com perfis de conexão (`--profile`: URL + token, ex.: um por agente; nada a ver com o compose).
- `--json` em todos os comandos. Os códigos de saída são estáveis e mapeados dos códigos de erro.
- `engramind help --json` descreve todos os comandos. É a base da skill e dos testes de contrato.

---

## 14. Skill

`skills/engramind/SKILL.md` ensina um agente com shell a usar a CLI:
- quando ler o contexto (início da tarefa) e quando buscar (dúvida sobre uma regra);
- como escrever uma boa memória: um fato durável, genérico, sem dados sensíveis, códigos de tarefa
  ou URLs temporárias, na categoria certa e checando `similar` antes de criar outra;
- os comandos exatos com `--json`, e como tratar os erros comuns.

Para agentes com shell, a skill costuma sair mais barata que o MCP: carrega algumas centenas de
tokens em vez do esquema de dezenas de ferramentas. A skill é **verificada por teste**: todo comando
citado nela precisa existir em `engramind help --json` (§15).

---

## 15. Documentação viva (padrão sap-middleman)

- **OpenAPI gerado do código.** Toda rota tem `#[utoipa::path]`, e os esquemas derivam dos mesmos
  tipos Rust usados na validação. O documento é servido em `/openapi.json`, com Swagger UI em
  `/docs`. A raiz redireciona de forma relativa, o que funciona atrás de proxy com prefixo.
- **Guias** em `docs/guides/*.md`: visão geral, conceitos, tokens e escopos, canais e assinaturas,
  escrever memórias, contexto e cache, busca e dicas, configurações, CLI, integração MCP, integração
  de um sistema consumidor, integração de um sistema gestor, erros e operação.
  - Os guias são **compilados no binário** (`include_str!`), com metadados de público (`all`/`admin`)
    e tags do OpenAPI.
  - O MCP sempre serve a documentação **da versão em execução**.
- **MCP de documentação**: as ferramentas do grupo "Documentação" usam um índice do OpenAPI, que
  liga cada operação aos parâmetros, ao corpo, às respostas por status, à segurança e aos esquemas
  referenciados.
- **Testes de contrato.** Falham no CI se algo ficar para trás:
  1. `docs/openapi.json`, versionado, é igual ao gerado (regenera com `UPDATE_SNAPSHOTS=1`);
  2. toda rota do router tem anotação `utoipa`;
  3. toda ferramenta MCP corresponde a uma operação REST e aparece em algum guia;
  4. todo comando da CLI corresponde a uma operação, e todo comando citado na skill e nos guias existe;
  5. todo código de erro está documentado no guia de erros;
  6. toda chave de configuração está documentada, com tipo e padrão.
- `CHANGELOG.md` e a versão do binário aparecem em `get_status` e `get_overview`.

---

## 16. Persistência, backup e Docker

### SQLite

- O banco é um arquivo único, `/data/engramind.db`, num volume nomeado.
- PRAGMAs: `journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout=5000` e `foreign_keys=ON`.
- Uma **thread escritora** recebe comandos por um canal tokio e serializa as escritas. Um pool de
  leitores atende as consultas. Memórias são uma carga de leitura, e um escritor dá folga.
- **Nunca** monte o `.db` em pasta de rede, NFS ou bind mount compartilhado entre hosts: o lock de
  arquivo falha e o banco corrompe.
- **Uma instância por banco.** O deploy tem uma parada curta: o compose recria o contêiner. Se um dia
  precisar de várias instâncias, o caminho é a implementação Postgres + pgvector do `Store`.

### Litestream

- O binário vem na imagem (`COPY --from=litestream/litestream:<versão fixa>`).
- O entrypoint `docker/entrypoint.sh` faz duas coisas:
  1. `litestream restore -if-db-not-exists -if-replica-exists /data/engramind.db`;
  2. `exec litestream replicate -exec "engramind serve"`.
- O destino S3 fica em `litestream.yml`, com credenciais por variável de ambiente
  (`LITESTREAM_ACCESS_KEY_ID`, `LITESTREAM_SECRET_ACCESS_KEY`, `ENGRAMIND_S3_BUCKET`…).
- Com `LITESTREAM_ENABLED=false` (ex.: uma instalação sem S3), o entrypoint roda
  `engramind serve` direto e `get_status` avisa que não há backup.
- `engramind doctor` confere integridade (`PRAGMA integrity_check`), migrações, extensões carregadas e,
  quando o Litestream existe, a idade da última réplica. A idade também aparece em `get_status`.

### Docker (mesmo padrão do sap-middleman)

**`Dockerfile`** multi-stage, só para a imagem de produção (sem estágio de desenvolvimento):
- `builder`: `rust:<versão>-bookworm` com `ca-certificates`, `curl` e `pkg-config`, rodando
  `cargo build --locked --release` com cache de registry e `target`.
- `runtime`: `debian:bookworm-slim` com `ca-certificates`, `curl`, o binário, o Litestream e o
  entrypoint. Roda com usuário de sistema sem privilégios, `EXPOSE ${APP_PORT}` e
  `HEALTHCHECK` em `/health/live`.
- O runtime inclui a biblioteca do ONNX Runtime, porque o provedor local de embeddings é o padrão.
  O modelo não entra na imagem: é baixado no primeiro uso para `/data/models`.
- O desenvolvimento acontece fora do Docker (`cargo run` / `cargo test`). O contêiner é sempre o
  artefato de produção.

**`docker-compose.yml`**, com um único serviço e **sem perfis** (`docker compose up -d --build`):
- `name: engramind-${APP_PORT}`.
- Serviço `api`: `build` do `Dockerfile`, `image: ${API_IMAGE:-engramind-api:latest}`,
  `container_name: ${CONTAINER_NAME}`, `init: true`, `restart: unless-stopped`, limites de CPU e
  memória e porta `${APP_PORT}`.
- Variáveis obrigatórias com `${VAR:?…}` (porta, pepper, chave de cifra).
- Rede externa configurável (padrão igual à do sap-middleman) e o volume nomeado `engramind-data:/data`.

**`.env.example`**, só com infraestrutura:

```env
APP_ENV=production
APP_HOST=0.0.0.0
APP_PORT=4300
CONTAINER_NAME=engramind-api
# API_IMAGE=engramind-api:4300

DATABASE_PATH=/data/engramind.db
MODELS_PATH=/data/models

# Aceito só com o banco vazio, para criar o primeiro token admin.
BOOTSTRAP_ADMIN_TOKEN=eng.00000000-0000-4000-8000-000000000001.substitua-por-32-ou-mais-caracteres
TOKEN_PEPPER=substitua-por-um-segredo-longo
# Chave (base64, 32 bytes) que cifra os segredos de configuração: openssl rand -base64 32
SECRETS_KEY=

REQUEST_TIMEOUT_SECS=30
MAX_BODY_BYTES=1048576
MCP_ALLOWED_HOSTS=localhost,127.0.0.1,engramind-api

LITESTREAM_ENABLED=false
ENGRAMIND_S3_BUCKET=
ENGRAMIND_S3_PATH=engramind
ENGRAMIND_S3_ENDPOINT=
LITESTREAM_ACCESS_KEY_ID=
LITESTREAM_SECRET_ACCESS_KEY=

RUST_LOG=engramind=info,tower_http=warn
LOG_FORMAT=compact
LOG_ANSI=true
TZ=America/Sao_Paulo
```

---

## 17. Notificações de mudança

- Toda mutação incrementa `channels.version` e enfileira um evento.
- **Webhooks por app.** O app informa `webhook_url`, e o segredo fica cifrado. O serviço envia `POST`
  assinado com HMAC-SHA256 (`Engramind-Signature: t=<ts>,v1=<hex>`) a todo app que tenha algum agente
  assinando o canal afetado. Apps com token `admin` recebem todos os eventos.
- Payload mínimo, sem textos:
  `{ id, type, channel, channel_version, memory_ids, at }`, com tipos `memory.created`,
  `memory.updated`, `memory.archived`, `memory.deleted`, `channel.updated`, `subscription.updated`
  e `settings.updated`.
- O envio é feito com retentativas e backoff (`webhooks.max_attempts`), e o `id` permite descartar
  repetições. O sistema gestor repassa os eventos à tela pelo mecanismo de tempo real dele.
- Clientes sem webhook usam `get_context` com `If-None-Match` no início do turno, um pull de poucos
  bytes.

---

## 18. Segurança

- **Escopos e acesso efetivo** (§4) são aplicados no `core`, valem para as três vias e são cobertos
  por testes de matriz (token × assinatura × operação).
- **Injeção de instruções.** Memória é uma instrução que outros agentes vão seguir. Sem fluxo de
  aprovação, as defesas são estas:
  - escopos mínimos por token e acesso restrito por assinatura (o revisor lê, o curador escreve);
  - proveniência, histórico, `revert_memory` e auditoria completos;
  - na tela do sistema gestor, o filtro "escritas recentes por agente" e o webhook `memory.*`;
  - o Redator trata o texto como dado e não segue nada do conteúdo.
- Segredos:
  - tokens só como hash com pepper;
  - chaves de API cifradas com `SECRETS_KEY`;
  - `zeroize` em memória;
  - nada de segredo em log, auditoria ou resposta.
- Limites:
  - corpo máximo e timeout por requisição;
  - rate limit por token;
  - `MCP_ALLOWED_HOSTS`;
  - CORS fechado: o uso é servidor-a-servidor, e a tela do sistema gestor chama pelo próprio servidor.
- Tokens expirados ou revogados recebem `401`. Escopo insuficiente recebe `403 SCOPE_REQUIRED`, que
  informa o escopo exigido.

---

## 19. Integração de sistemas

### Sistema consumidor (guia `integracao-consumidor`)

1. O gestor cria o app, os agentes, os tokens (com os escopos mínimos) e as assinaturas.
2. Por turno:
   1. `get_context` (com `If-None-Match` e `channels` do projeto) vai para a parte estável do prompt;
   2. opcionalmente, `get_hints` com a mensagem do turno, colocado no fim.
3. Ferramentas: registre o MCP com o token do agente e o cabeçalho `Engramind-Channels`, e repasse só as
   ferramentas que a política do sistema permite. A alternativa é a skill com a CLI.
4. Reaja aos webhooks ou ao `304` para saber quando reconstruir o prompt.
5. Para trazer memórias que já existem, use `import_memories` (token `admin`), um canal por lista de
   origem, preservando datas e autoria.

### Sistema gestor (guia `integracao-gestor`)

- A tela cobre canais, memórias (lista, busca, histórico, reverter, fixar, arquivar, mover e
  excluir), apps, agentes, tokens, assinaturas, configurações com testers, fila de jobs, consumo do
  Redator (`llm_calls`), auditoria e status do backup.
- Toda chamada sai do servidor do gestor com o token `admin`, que fica guardado como segredo do
  gestor. O tempo real vem dos webhooks.

### Exemplo de organização de canais

| Canal | Assinantes |
|---|---|
| `assistente/geral` | `app-a/assistente` (`inject`, `read+write`) |
| `eng/global` | `app-a/desenvolvedor` (`inject`, `read`), `app-a/revisor` (`inject`, `read`), `app-a/curador` (`manage`, `read+write`) |
| `eng/projeto-*` (prefixo) | `app-a/desenvolvedor` (`inject`, `read`), `app-a/curador` (`manage`, `read+write`) |
| `review/revisor` | `app-a/revisor` (`inject`, `read`), `app-a/curador` (`manage`, `read+write`) |
| `eng/global` | `app-b/agente-cli` (`inject`, `read+write`), compartilhado entre sistemas |

---

## 20. Observabilidade

- Logs `tracing` estruturados, com `request_id`, `app`, `agent`, operação e duração. Nunca o texto
  das memórias em nível `info`.
- Métricas Prometheus em `/metrics`, protegido por token `admin`·read ou por rede interna:
  - latência por operação;
  - taxa de `304` do contexto;
  - tamanho da fila de jobs e falhas por tipo;
  - chamadas e custo do Redator;
  - duração dos embeddings;
  - idade da réplica do Litestream.
- `get_status` resume tudo isso para a tela do gestor.

---

## 21. Testes

- `cargo test` com SQLite temporário por teste (arquivo em `tempdir`, com as extensões carregadas).
- Provedores **falsos** e determinísticos:
  - um LLM roteirizado para o Redator e o consolidador;
  - embeddings por hash, para a busca ser reprodutível;
  - um servidor de webhook de teste.
- O que é coberto:
  - matriz de escopos e acesso;
  - ordem e cortes do contexto;
  - estabilidade da `version` e `304`;
  - fusão RRF;
  - duplicatas, `similar` e conflito de versão;
  - idempotência;
  - guarda de sentido do Redator;
  - fila com retentativa;
  - assinatura HMAC dos webhooks;
  - importação.
- Os testes de contrato da §15 completam a cobertura.
- Clippy com `-D warnings` e `cargo fmt --check` no CI.

---

## 22. Fases

1. **Núcleo.** Repositório, Docker e compose, migrações, tokens com escopos e bootstrap admin, apps e
   agentes, canais, assinaturas (com prefixo e modos), CRUD de memórias com revisões, auditoria,
   idempotência e conflito de versão, `get_context` (tagged/markdown/json, duas faixas, ETag e cache),
   FTS5, REST + OpenAPI + Swagger, guias embutidos, CLI, Litestream e testes de contrato.
2. **Inteligência.** Fila de jobs, Redator (título + texto, guarda de sentido, fallback de modelos),
   embeddings (local e remoto) + sqlite-vec, busca híbrida, `similar`, `get_hints`, configurações
   completas, segredos e testers, `llm_calls`, MCP completo (ferramentas filtradas, resources,
   prompts e documentação) e skill.
3. **Integração.** Webhooks, `import_memories`, métricas, `get_status` completo e guias de integração
   de consumidor e gestor. A partir daqui, um primeiro sistema consumidor e gestor pode migrar.
4. **Consolidação.** Duplicatas, importância, desuso e `mark_verified`.
5. **Escala, se necessário.** Índice `vec0` com partição por canal e, só se precisar de várias
   instâncias, a implementação Postgres + pgvector do `Store`.

---

## 23. Decisões

### Tomadas

1. **Tokens `agent` e `admin`**, ambos com escopos `read`/`write`/`delete`.
2. **Agente curador assina os canais-alvo em modo `manage`**, com `read+write`. A memória pertence
   ao canal, e o curador fica registrado como autor.
3. **Contexto padrão `tagged`** (`#id: texto` agrupado em tags), em duas faixas e com cache em
   memória. `markdown` e `json` ficam como alternativas.
4. **Redator gera o título e revisa o texto**, com `rewrite_text` configurável e a guarda de sentido.
5. **Contêiner Docker próprio**, no padrão do sap-middleman, só de produção: um `Dockerfile` sem
   estágio de desenvolvimento e um compose com um único serviço, sem perfis. SQLite em volume e
   Litestream na imagem.
6. **Escopos no token**, que a assinatura pode restringir canal a canal.
7. **Sem fluxo de sugestão e aprovação.** A autonomia é definida pelo sistema consumidor.
8. **Embeddings configuráveis**: local (`fastembed`) ou remoto (`openai_compatible`, `ollama`,
   `gemini`), com o provedor e o modelo escolhidos pela configuração. O serviço inicializa com o
   local `multilingual-e5-small`.
9. **Canais só pelas ferramentas administrativas.** Um token `agent` nunca cria canais. Um agente
   que precisa criá-los recebe um token `admin` vinculado a ele (§4), com os escopos mínimos.
10. **Nomes**: serviço `engramind`, CLI `engramind`, tokens `eng.…`, cabeçalhos `Engramind-*`, variáveis
    `ENGRAMIND_*` e recursos MCP `engramind://`.

Não há decisões em aberto.
