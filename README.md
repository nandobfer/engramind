# Engramind

Rede de memórias para agentes de IA. Um serviço em Rust, com SQLite (WAL), sqlite-vec e FTS5, replicado para S3 pelo Litestream.

As memórias ficam em **canais** (ex.: `eng/global`, `eng/projeto-atlas`). Cada agente é identificado por um **token** com escopos (`read`, `write`, `delete`) e recebe as memórias por **REST, MCP ou CLI** (com skill), com documentação viva servida pelo próprio binário.

> **Status:** a versão vigente está no [`Cargo.toml`](Cargo.toml) (e em `get_status`), e o que mudou em cada uma, no [`CHANGELOG.md`](CHANGELOG.md). Estão implementados: identidades e tokens com escopos, canais e assinaturas com acesso efetivo, memórias (escrita, edição versionada, status, histórico, `move`, `revert` e importação), contexto determinístico com `version`/ETag, o Redator, os embeddings com busca híbrida e dicas, webhooks, métricas Prometheus, retenção e consolidação, nas três vias (REST, MCP e CLI), com a skill da CLI em [`skills/engramind/SKILL.md`](skills/engramind/SKILL.md). Falta só a escala (índice `vec0` particionado por canal e o `Store` Postgres + pgvector), que ganha spec quando uma medição justificar. O desenho completo está em [`docs/design.md`](docs/design.md); o plano de cada etapa, em [`specs/`](specs/README.md).

## Por que existe

Memórias de agentes costumam ficar espalhadas, uma coleção por sistema e por papel. O Engramind centraliza essas memórias e deixa qualquer sistema consumir a mesma rede.

Inspirado nos conceitos do [Magic Context](https://github.com/cortexkit), adaptado para canais e para acesso pela rede.

## Princípios

- **O canal é a unidade de compartilhamento.** Uma memória pertence ao canal, não ao agente que a escreveu. O autor fica registrado como proveniência.
- **O token identifica e delimita.** Ele diz qual app e qual agente está falando, e carrega os escopos. A política fina (quais ferramentas o agente vê e quando ele escreve) é decidida pelo sistema consumidor.
- **Agentes escrevem direto**, sem fluxo de sugestão e aprovação. Toda escrita guarda proveniência e histórico de versões, então tudo pode ser revertido.
- **A curadoria textual é do serviço.** Um subagente interno, o Redator, gera o título e revisa o texto de cada memória.
- **Tudo é consultável por qualquer via** (REST, MCP e CLI), com as mesmas regras, validações, escopos e códigos de erro.
- **Contexto estável para o cache.** O bloco de memórias de um agente é determinístico e só muda quando as memórias mudam, preservando o cache de prompt do provedor de LLM.
- **Desacoplado.** O serviço não conhece nenhum sistema consumidor específico. A integração passa só pelos contratos públicos.

### Fora do escopo

- Não é um indexador RAG de código nem de documentos. Guarda fatos duráveis e curtos.
- Não executa agentes nem conversa com o usuário.
- Não extrai memórias de conversas. Isso é papel dos sistemas consumidores.
- Uma instância por banco, que é o limite consciente do SQLite.

## Arquitetura

```
Sistema gestor ──REST + webhooks──┐
Sistemas consumidores ──MCP/REST──┼──▶ engramind (Rust: axum + tokio)
Agente com shell (CLI + skill) ───┘        │
                                           ├─ auth: Bearer token → Principal
                                           ├─ REST (OpenAPI + Swagger UI)
                                           ├─ MCP (Streamable HTTP, stateless)
                                           ├─ core: memórias, canais, assinaturas,
                                           │        identidades, configurações,
                                           │        contexto, busca, auditoria
                                           └─ workers: redator, embeddings,
                                                       consolidador, webhooks
                                           │
                                           ▼ rusqlite (WAL) + sqlite-vec + FTS5
                              /data/engramind.db ──Litestream──▶ S3
```

- **Uma única camada de domínio** (`core`). REST, MCP e CLI são adaptadores finos sobre ela.
- **O armazenamento fica atrás de uma interface (`Store`).** Hoje há uma implementação em SQLite. Uma migração futura para Postgres + pgvector troca só essa implementação.
- **A CLI `engramind` é o mesmo binário**, agindo como cliente HTTP da API. Ela não abre o banco, exceto nos comandos de operação do servidor (`engramind serve`, `engramind migrate`, `engramind doctor`).
- **Workers `tokio`** consomem a fila em tabela `jobs` (sem broker): `redact` (Redator), `embed` (embeddings), `webhook` (entregas) e `consolidate` (consolidador), com lease, backoff e `list_jobs`/`retry_job`.
- **Uma instância por banco.** Não há coordenação entre instâncias.

### Rotas

| Rota | Acesso | Uso |
|---|---|---|
| `/v1/...` | Bearer token | API REST (uma rota por operação) |
| `POST /mcp` | Bearer token | MCP (Streamable HTTP, stateless), uma ferramenta por operação |
| `GET /metrics` | token `admin` com `read` | Métricas Prometheus |
| `GET /health/live`, `GET /health/ready` | pública | Healthchecks |
| `GET /openapi.json`, `GET /docs` | pública | Contrato OpenAPI e Swagger UI |

## Conceitos principais

| Termo | Significado |
|---|---|
| **Sistema consumidor** | Sistema que executa agentes e entrega as memórias a eles (no prompt, por MCP ou pela CLI). |
| **Sistema gestor** | Sistema com tela de administração da rede. Usa um token `admin`. |
| **App** | Registro de um sistema na rede (ex.: `app-a`). Agrupa agentes e recebe webhooks. |
| **Agente** | Identidade dentro de um app (ex.: `app-a/desenvolvedor`), com token próprio. |
| **Agente curador** | Agente que escreve e mantém memórias para outros agentes. |
| **Canal** | Lista de memórias compartilhada. |
| **Assinatura** | Ligação entre agente e canal, com um modo (`inject`, `search` ou `manage`) e um acesso. |
| **Contexto** | Bloco renderizado com as memórias dos canais assinados, pronto para o prompt. |

## Tokens e escopos

Há dois tipos de token:

- **`agent`**: opera memórias nos canais assinados pelo agente.
- **`admin`**: gere a rede (apps, agentes, tokens, canais, assinaturas, configurações, jobs e auditoria). Pode ser vinculado a um agente administrativo.

Escopos:

| Escopo | Permite |
|---|---|
| `read` | Ler contexto, listar, buscar e consultar histórico e documentação. |
| `write` | Criar, editar, arquivar, restaurar, fixar, mover e reverter. |
| `delete` | Excluir de vez memórias (e, no token `admin`, canais, apps e agentes). |

Combinações válidas: `read`, `read+write`, `read+delete` e `read+write+delete`.

**Acesso efetivo** = escopos do token ∩ acesso da assinatura. A assinatura pode restringir o acesso, mas nunca ampliar o que o token permite. O MCP lista em `tools/list` apenas as ferramentas que o token pode usar.

O formato do token é `eng.<token_id>.<segredo>`. O segredo aparece uma única vez, na emissão, e é guardado só como HMAC-SHA256 com o `TOKEN_PEPPER`, comparado em tempo constante.

## Contexto

O formato padrão (`tagged`) agrupa as memórias por canal e categoria, com uma linha por memória:

```
<memories version="cv_3f9e1a7c4b2d">
<channel slug="eng/global">
<CONSTRAINTS>
#m_8f2a1c: Nunca rodar a suíte completa de testes localmente; só os testes afetados.
</CONSTRAINTS>
</channel>
<overflow count="12">use search_memories para o restante</overflow>
</memories>
```

- **Formatos:** `tagged` (padrão), `markdown` e `json`.
- **Duas faixas de orçamento:** texto completo até `context.budget_tokens` (padrão 4.000); depois só `#id: título` até `context.titles_budget_tokens` (padrão 800); o restante vira uma linha de excedente.
- **Cache:** `version` (`cv_` + 12 hex) e `ETag` permitem responder `304` sem corpo, e sem tocar no banco, quando nada mudou.
- **MCP:** o mesmo bloco está no resource `engramind://context`; há também `engramind://channels/{slug}`, `engramind://guides/{id}` e o prompt `memory-writing`.
- **Dicas (`get_hints`):** fragmentos `#id · título · canal`, sem o texto completo, para a "lembrança vaga" antes do turno.

Detalhes no guia [`contexto-e-cache`](docs/guides/contexto-e-cache.md).

## Busca

`search_memories` combina duas buscas com fusão RRF (Reciprocal Rank Fusion):

1. **Por sentido:** embeddings do modelo vigente comparados por cosseno com sqlite-vec.
2. **Textual:** FTS5 com BM25, sem acentos.

A resposta traz `mode: hybrid` quando a consulta foi vetorizada. Sem embeddings (desligados, pendentes ou com o provedor fora), a busca usa só o texto e responde `mode: text`, sem erro. Detalhes no guia [`busca-e-dicas`](docs/guides/busca-e-dicas.md).

## Redator e embeddings

- **Redator:** gera um título e, opcionalmente (`redator.rewrite_text`), revisa o texto. Provedores `openai_compatible`, `anthropic` e `gemini`, com `redator.fallback_models` e `redator.temperature` (vazio usa o padrão do modelo). Tem guarda de sentido: se a revisão se afastar do original (`redator.min_similarity`), o texto original é mantido; com os embeddings desligados, o texto revisado nunca é aplicado. Se o Redator falhar ou não estiver configurado, a memória é salva como foi escrita (`title_state = fallback`). Cada chamada fica em `llm_calls`; `test_redator` testa a configuração.
- **Embeddings:** provedor `local` (`fastembed` + ONNX Runtime, com o modelo multilíngue `multilingual-e5-small` como padrão, baixado no primeiro uso em `MODELS_PATH`), `openai_compatible`, `ollama`, `gemini` ou `off`. Trocar de modelo reindexa todas as memórias sem misturar dimensões (`reembed_all`, `test_embedding`, `list_embedding_models`). Enquanto o modelo local baixa ou carrega, as chamadas que precisam dele recebem `409 EMBEDDING_MODEL_LOADING`.
- **Recusas do provedor** (HTTP 4xx) respondem `422 PROVIDER_REJECTED` com o motivo; indisponibilidade ou resposta inválida respondem `502 PROVIDER_FAILED`.
- **Segredos** (chaves de API) são gravados com `set_secret`, cifrados com `SECRETS_KEY` e nunca lidos de volta: a leitura mostra só `{ configured, updated_at }`.

A escrita nunca espera o LLM nem o embedding: persiste, enfileira e responde. Todo o comportamento funcional fica nas configurações gerenciadas pela API (`get_settings`/`update_settings`), descritas no guia [`configuracoes`](docs/guides/configuracoes.md).

## Integração

- **Eventos:** `memory.created`, `memory.updated`, `memory.archived`, `memory.deleted`, `channel.updated`, `subscription.updated` e `settings.updated`, sem o texto das memórias.
- **Webhooks por app:** `webhook_url` e `webhook_secret` em `create_app`/`update_app`. Cada entrega é um `POST` com `Engramind-Event-Id`, `Engramind-Event-Type` e `Engramind-Signature: t=<unix>,v1=<hex(HMAC-SHA256(segredo, t.corpo))>`, com backoff até `webhooks.max_attempts`.
- **Importação:** `import_memories` grava um lote numa transação, preservando `created_at`, autor e origem (a CLI aceita JSON e JSONL).
- **Operação:** `get_status` (banco, backup, fila, Redator, embeddings, webhooks, retenção e consolidação), `get_usage`, `get_audit`, `list_jobs` e `/metrics`.

Detalhes nos guias [`integracao-consumidor`](docs/guides/integracao-consumidor.md), [`integracao-gestor`](docs/guides/integracao-gestor.md), [`integracao-mcp`](docs/guides/integracao-mcp.md) e [`operacao`](docs/guides/operacao.md).

## Consolidação

Um job `consolidate` por canal, agendado por `consolidation.schedule` (cron de 5 campos em UTC) ou pedido com `run_consolidation` (`dry_run` mostra o plano sem chamar o LLM). Tarefas:

- **`duplicates`:** pares parecidos vão ao Redator, que decide fundir; a fusão passa pela guarda de sentido e a memória absorvida é arquivada com `merged_into`.
- **`importance`:** nota de 0 a 1, aplicada à ordem do contexto só quando a `version` do canal muda.
- **`stale`:** marca `stale_candidate` nas memórias sem uso há `consolidation.stale_after_days` e, com `consolidation.auto_archive_stale`, arquiva.

`mark_verified` confirma que uma memória continua válida. Detalhes no guia [`consolidacao`](docs/guides/consolidacao.md).

## Stack

| Função | Escolha |
|---|---|
| Linguagem | Rust 1.98 (edition 2024, `rust-toolchain.toml`) |
| HTTP | `axum` 0.8 + `tokio` + `tower-http` |
| Banco | `rusqlite` (bundled, WAL) + `sqlite-vec` + FTS5 |
| Migrações | SQL versionado em `migrations/`, embutido no binário |
| OpenAPI | `utoipa` 5 + `utoipa-swagger-ui` |
| MCP | `rmcp` (Streamable HTTP, stateless) |
| CLI | `clap`, com perfis e saída `--json` |
| LLM e embeddings remotos | cliente próprio sobre `reqwest` |
| Embeddings locais | `fastembed` + ONNX Runtime (`ORT_DYLIB_PATH`) |
| Segredos | AES-256-GCM (`SECRETS_KEY`) e tokens com HMAC-SHA256 (`TOKEN_PEPPER`) |
| Agendamento | `croner` (cron em UTC) |
| Observabilidade | `tracing` (logs `compact`/`json`) e Prometheus (`metrics`) |
| Backup | Litestream → S3 |

## Documentação viva

- O contrato OpenAPI é gerado do código e servido em `/openapi.json`, com Swagger UI em `/docs`. O snapshot fica em [`docs/openapi.json`](docs/openapi.json).
- Os guias ficam em [`docs/guides/`](docs/guides/) e são compilados no binário. O MCP serve a documentação da versão em execução (`get_overview`, `list_guides`, `get_guide`).
- Testes de contrato falham no CI se a documentação ficar para trás: o OpenAPI versionado precisa bater com o gerado, toda rota precisa de anotação, toda ferramenta MCP precisa de operação REST correspondente e de guia, todo comando citado nos guias e na skill precisa existir em `engramind help --json`, e todo código de erro e chave de configuração precisa estar documentado.

## Configuração

O serviço lê as variáveis do ambiente e de um `.env` no diretório atual. Elas cuidam só da infraestrutura; o comportamento funcional fica nas configurações gerenciadas pela API. O modelo completo, sem valores sensíveis, está em [`.env.example`](.env.example). Valores inválidos impedem o startup.

| Variável | Obrigatória | Padrão | Uso |
|---|---|---|---|
| `APP_ENV` | não | `development` | `development`, `staging` ou `production`. |
| `APP_HOST` | não | `0.0.0.0` | Endereço de escuta. |
| `APP_PORT` | sim | — | Porta HTTP (também nomeia o projeto Compose `engramind-${APP_PORT}`). |
| `DATABASE_PATH` | não | `./data/engramind.db` (imagem: `/data/engramind.db`) | Banco SQLite; o diretório é criado se não existir. |
| `MODELS_PATH` | não | `./data/models` (imagem: `/data/models`) | Cache dos modelos locais de embedding. |
| `ORT_DYLIB_PATH` | não | definida na imagem | Biblioteca do ONNX Runtime. Sem ela, `embedding.provider = local` fica indisponível e a busca usa só o FTS. |
| `READ_POOL_SIZE` | não | `4` | Conexões de leitura do SQLite. |
| `BOOTSTRAP_ADMIN_TOKEN` | não | — | Primeiro token `admin`, aceito só com o banco sem tokens (ver [Primeiro acesso](#primeiro-acesso)). |
| `TOKEN_PEPPER` | sim | — | Chave do HMAC-SHA256 dos tokens, com pelo menos 32 caracteres. |
| `SECRETS_KEY` | sim | — | Chave em base64 de 32 bytes que cifra os segredos de configuração. |
| `REQUEST_TIMEOUT_SECS` | não | `30` | Tempo limite de cada requisição. |
| `MAX_BODY_BYTES` | não | `1048576` | Tamanho máximo do corpo. |
| `MCP_ALLOWED_HOSTS` | em `staging` e `production` | `localhost,127.0.0.1` fora de `staging`/`production` | Hosts aceitos em `/mcp` (separados por vírgula, ou `*`), contra DNS rebinding. |
| `LITESTREAM_ENABLED` | não | `false` | Liga a restauração e a replicação do Litestream na imagem. |
| `ENGRAMIND_S3_BUCKET`, `ENGRAMIND_S3_PATH`, `ENGRAMIND_S3_ENDPOINT`, `LITESTREAM_ACCESS_KEY_ID`, `LITESTREAM_SECRET_ACCESS_KEY` | com `LITESTREAM_ENABLED=true` | `ENGRAMIND_S3_PATH=engramind` | Réplica no S3 (MinIO ou compatível). |
| `RUST_LOG` | não | `engramind=info,tower_http=warn` | Filtro de logs. |
| `LOG_FORMAT` | não | `compact` | `compact` ou `json`. |
| `LOG_ANSI` | não | `true` | Cores no log. |
| `TZ` | não | — | Fuso do contêiner. |
| `CONTAINER_NAME` | sim (Compose) | — | Nome do contêiner. |
| `API_IMAGE` | não (Compose) | `engramind-api:latest` | Imagem usada pelo Compose; obrigatória no override de deploy. |
| `ENGRAMIND_NETWORK`, `S3_NETWORK` | não (Compose) | `engramind-network`, `minio-network` | Redes Docker externas. |

## Primeiro acesso

Com o banco vazio, defina `BOOTSTRAP_ADMIN_TOKEN` no formato `eng.tk_<8 hex>.<segredo com 32+ caracteres [A-Za-z0-9_-]>`. Uma forma de gerá-lo:

```bash
echo "eng.tk_$(openssl rand -hex 4).$(openssl rand -base64 32 | tr '+/' '-_' | tr -d '=\n')"
```

Na subida, o serviço cria o app `bootstrap` com um token `admin` `read+write+delete` e registra `security.bootstrap_created`. Use esse token para emitir os tokens dos sistemas gestores e consumidores (`issue_token`) e guarde-o num cofre. Com tokens existentes, o valor é ignorado (`security.bootstrap_ignored`). Detalhes no guia [`tokens-e-escopos`](docs/guides/tokens-e-escopos.md).

## CLI

O binário `engramind` (também `eng`) é o servidor e o cliente. `serve`, `migrate` e `doctor` abrem o banco; os demais comandos são clientes HTTP da API, um por operação, com os mesmos parâmetros e códigos de erro do REST.

A conexão é lida nesta ordem: flag, variável de ambiente e perfil (`~/.config/engramind/config.toml`, gravado com permissão `0600`).

| Flag | Variável |
|---|---|
| `--url` | `ENGRAMIND_URL` |
| `--token` | `ENGRAMIND_TOKEN` |
| `--channels` | `ENGRAMIND_CHANNELS` (vira o cabeçalho `Engramind-Channels`) |

```bash
engramind profiles add curador --url https://engramind.exemplo --token "$TOKEN" --default
engramind whoami
engramind context
engramind memories list --channels eng/global --json
engramind help --json        # catálogo de comandos para agentes
```

Detalhes no guia [`cli`](docs/guides/cli.md) e na skill [`skills/engramind/SKILL.md`](skills/engramind/SKILL.md).

## Docker e backup

- Imagem de produção multi-stage ([`Dockerfile`](Dockerfile)), com o binário (`engramind` e o atalho `eng`), o Litestream, a biblioteca do ONNX Runtime e o entrypoint [`docker/entrypoint.sh`](docker/entrypoint.sh). Com `LITESTREAM_ENABLED=true`, o entrypoint restaura o banco do S3 quando ele não existe e sobe o servidor sob `litestream replicate`; com argumentos, executa o comando informado.
- [`docker-compose.yml`](docker-compose.yml) com um único serviço (`api`), sem perfis, e duas redes externas.

```bash
docker network create engramind-network   # uma vez por host (ENGRAMIND_NETWORK)
docker network create minio-network       # ou aponte S3_NETWORK para a rede do S3 existente
cp .env.example .env                       # preencha APP_PORT, CONTAINER_NAME, TOKEN_PEPPER, SECRETS_KEY e MCP_ALLOWED_HOSTS
docker compose up -d --build
docker compose exec api eng doctor
curl "http://localhost:${APP_PORT}/health/ready"
```

O banco fica em `/data/engramind.db`, num volume nomeado (`engramind-data`). Nunca monte o banco em NFS ou em bind mount compartilhado entre hosts. `./scripts/check-compose.sh` valida o Compose e o override de deploy sem subir nada.

## Desenvolvimento

O desenvolvimento acontece fora do Docker. Pré-requisitos: o toolchain fixado em [`rust-toolchain.toml`](rust-toolchain.toml) (instalado pelo `rustup`) e o [`cargo-nextest`](https://nexte.st/); o `./scripts/test.sh` também exige o `cargo-llvm-cov` e o Docker.

```bash
cargo run -- serve                          # .env com APP_PORT, TOKEN_PEPPER e SECRETS_KEY
cargo nextest run -E 'test(/<modulo>::/)'   # só os testes afetados
cargo nextest run --locked                  # suíte completa sem cobertura
cargo clippy --all-targets -- -D warnings
cargo fmt --check
UPDATE_SNAPSHOTS=1 cargo nextest run -E 'test(/contract::/)'   # regenera docs/openapi.json
./scripts/test.sh                           # compose, lint e suíte com cobertura (como no CI)
```

Em `development`, o banco e os modelos ficam em `./data/` e o `MCP_ALLOWED_HOSTS` é opcional. Sem `ORT_DYLIB_PATH`, o provedor local de embeddings fica indisponível e a busca usa só o texto.

Os testes de integração ficam num único binário, [`tests/it/`](tests/it/), e usam SQLite temporário e provedores falsos e determinísticos (LLM roteirizado, embeddings por hash e servidor de webhook de teste); nunca dependem de rede.

O CI ([`.github/workflows/ci-cd.yml`](.github/workflows/ci-cd.yml)) roda lint, Compose, a suíte em shards e a cobertura em todo pull request e push na `main`. Na `main`, o CD constrói a imagem no runner, transfere por SCP e recria o serviço com [`docker-compose.deploy.yml`](docker-compose.deploy.yml), sem build no servidor.

As regras para quem desenvolve (humano ou agente) estão em [`AGENTS.md`](AGENTS.md) e [`aicontext/`](aicontext/README.md); as decisões arquiteturais, em [`docs/decisions/`](docs/decisions/).

## Estrutura do repositório

```
src/
  core/        domínio: operações, acesso, memórias, canais, contexto, busca,
               Redator, embeddings, jobs, eventos, consolidação, auditoria
  rest/        adaptador REST (/v1) com #[utoipa::path]
  mcp/         adaptador MCP: ferramentas, resources e prompt
  cli/         CLI (cliente HTTP), perfis e saída
  auth/        Bearer token e middleware
  store/       interface Store e a implementação SQLite (único lugar com SQL)
  providers/   clientes de LLM e de embeddings
  app.rs, config.rs, observability.rs, metrics.rs, health.rs, doctor.rs, main.rs
migrations/    0001_init.sql … 0009_consolidation.sql
tests/it/      binário único de integração, um módulo por domínio
docs/          design.md, guides/, decisions/, prompts/, openapi.json
aicontext/     regras operacionais para desenvolvimento assistido
specs/         plano de cada etapa (001 a 009)
skills/        skill da CLI
docker/        entrypoint da imagem
scripts/       test.sh, check-compose.sh, deploy-production.ts
```

## Roadmap

1. **Núcleo** (concluído): tokens e escopos, apps, agentes, canais, assinaturas, memórias com revisões, contexto, FTS5, REST + OpenAPI, CLI, Litestream e testes de contrato.
2. **Inteligência** (concluído): fila de jobs, Redator, embeddings + sqlite-vec, busca híbrida, dicas, configurações completas e MCP completo com skill.
3. **Integração** (concluído): webhooks, importação de memórias, métricas e guias de integração de consumidor e gestor.
4. **Consolidação** (concluído): duplicatas, importância, desuso e `mark_verified`.
5. **Escala, se necessário** (pendente): índice `vec0` particionado por canal e, só se precisar de várias instâncias, Postgres + pgvector.

## Licença

A definir.
