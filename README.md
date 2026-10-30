# Haikocampus

Rede de memórias para agentes de IA. Um serviço em Rust, com SQLite (WAL), sqlite-vec e FTS5, replicado para S3 pelo Litestream.

As memórias ficam em **canais** (ex.: `eng/global`, `eng/projeto-atlas`). Cada agente é identificado por um **token** com escopos (`read`, `write`, `delete`) e recebe as memórias por **REST, MCP ou CLI** (com skill), com documentação viva servida pelo próprio binário.

> **Status:** desenho revisado, pronto para planejamento. Ainda não há implementação. O desenho completo está em [`docs/design.md`](docs/design.md).

## Por que existe

Memórias de agentes costumam ficar espalhadas, uma coleção por sistema e por papel. O Haikocampus centraliza essas memórias e deixa qualquer sistema consumir a mesma rede.

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
Sistemas consumidores ──MCP/REST──┼──▶ haikocampus (Rust: axum + tokio)
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
                              /data/haikocampus.db ──Litestream──▶ S3
```

- **Uma única camada de domínio** (`core`). REST, MCP e CLI são adaptadores finos sobre ela.
- **O armazenamento fica atrás de uma interface (`Store`).** Hoje há uma implementação em SQLite. Uma migração futura para Postgres + pgvector troca só essa implementação.
- **A CLI `haiko` é o mesmo binário**, agindo como cliente HTTP da API. Ela não abre o banco, exceto nos comandos de operação do servidor (`haiko serve`, `haiko migrate`, `haiko doctor`).

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

O formato do token é `hk.<token_id>.<segredo>`. O segredo aparece uma única vez, na emissão, e é guardado só como hash com pepper.

## Contexto

O formato padrão (`tagged`) agrupa as memórias por canal e categoria, com uma linha por memória:

```
<memories version="cv_3f9e1a">
<channel slug="eng/global">
<CONSTRAINTS>
#m_8f2a1c: Nunca rodar a suíte completa de testes localmente; só os testes afetados.
</CONSTRAINTS>
</channel>
<overflow count="12">use search_memories para o restante</overflow>
</memories>
```

- **Duas faixas de orçamento:** texto completo até `context.budget_tokens` (padrão 4.000); depois só `#id: título` até `context.titles_budget_tokens` (padrão 800); o restante vira uma linha de excedente.
- **Cache:** `version` e `ETag` permitem responder `304` sem corpo quando nada mudou.
- **Dicas (`get_hints`):** fragmentos `#id · título · canal`, sem o texto completo, para a "lembrança vaga" antes do turno.

## Busca

`search_memories` combina duas buscas com fusão RRF (Reciprocal Rank Fusion):

1. **Por sentido:** embeddings comparados com sqlite-vec.
2. **Textual:** FTS5 com BM25, sem acentos.

Sem embeddings (desligados ou pendentes), a busca usa só o texto.

## Redator e embeddings

- **Redator:** gera um título e, opcionalmente, revisa o texto. Tem guarda de sentido: se a revisão se afastar do original, o texto original é mantido. Se o Redator falhar ou não estiver configurado, a memória é salva como foi escrita.
- **Embeddings:** provedor local (`fastembed`, com o modelo multilíngue `multilingual-e5-small` como padrão) ou remoto (OpenAI-compatível, Ollama ou Gemini). Trocar de modelo reindexa todas as memórias sem misturar dimensões.

## Stack

| Função | Escolha |
|---|---|
| HTTP | `axum` 0.8 + `tokio` + `tower-http` |
| Banco | `rusqlite` (bundled) + `sqlite-vec` + FTS5 |
| Migrações | SQL versionado embutido no binário |
| OpenAPI | `utoipa` + `utoipa-swagger-ui` |
| MCP | `rmcp` (Streamable HTTP, stateless) |
| CLI | `clap`, com saída `--json` |
| Backup | Litestream → S3 |

## Documentação viva

- O contrato OpenAPI é gerado do código e servido em `/openapi.json`, com Swagger UI em `/docs`.
- Os guias ficam em `docs/guides/` e são compilados no binário. O MCP serve a documentação da versão em execução.
- Testes de contrato falham no CI se a documentação ficar para trás: o OpenAPI versionado precisa bater com o gerado, toda rota precisa de anotação, toda ferramenta MCP precisa de operação REST correspondente, e todo comando citado na skill precisa existir.

## Docker e backup

- Imagem de produção multi-stage, com o binário, o Litestream e o entrypoint `docker/entrypoint.sh`, que restaura o banco do S3 na primeira subida e replica depois.
- `docker-compose.yml` com um único serviço, sem perfis:

```bash
cp .env.example .env   # preencha as variáveis obrigatórias
docker compose up -d --build
```

O banco fica em `/data/haikocampus.db`, num volume nomeado (`haiko-data`). Nunca monte o banco em NFS ou em bind mount compartilhado entre hosts.

## Desenvolvimento

O desenvolvimento acontece fora do Docker:

```bash
cargo run -- serve
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

Os testes usam provedores falsos e determinísticos (LLM roteirizado, embeddings por hash e servidor de webhook de teste).

## Roadmap

1. **Núcleo:** tokens e escopos, apps, agentes, canais, assinaturas, memórias com revisões, contexto, FTS5, REST + OpenAPI, CLI, Litestream e testes de contrato.
2. **Inteligência:** fila de jobs, Redator, embeddings + sqlite-vec, busca híbrida, dicas, configurações completas e MCP completo com skill.
3. **Integração:** webhooks, importação de memórias, métricas e guias de integração de consumidor e gestor.
4. **Consolidação:** duplicatas, importância, desuso e `mark_verified`.
5. **Escala, se necessário:** índice `vec0` particionado por canal e, só se precisar de várias instâncias, Postgres + pgvector.

## Licença

A definir.
