# Specs — plano de implementação

**Criado em**: 2026-10-30
**Fonte**: [`../docs/design.md`](../docs/design.md) (desenho) e o sap-middleman (`/home/burgos/code/sipal/sap-middleman`) como referência de código, arquitetura e operação.

O desenho descreve a ideia. Estas specs transformam o desenho em **etapas entregáveis**: cada etapa termina com uma capacidade utilizável, implantável e testada, nas três vias (REST + OpenAPI, MCP e CLI), com documentação viva e testes de contrato verdes. As etapas não seguem as fases do §22 do desenho.

## Formato (igual ao sap-middleman)

Cada etapa tem um diretório `specs/NNN-nome/`:

| Arquivo | Quando | Conteúdo |
|---|---|---|
| `spec.md` | planejamento | objetivo, escopo, requisitos (`FR-xxx`), fora do escopo e critérios de sucesso |
| `clarify.md` | planejamento | perguntas (`Q-xxx`), decisões (`D-xxx`) e bloqueios |
| `contracts.md` | planejamento | interfaces Rust, rotas REST, ferramentas MCP, comandos da CLI, códigos de erro e migração |
| `plan.md` | planejamento | contexto técnico, áreas afetadas, estratégia de implementação, testes e riscos |
| `tasks.md` | planejamento e execução | tarefas `Txxx` marcadas conforme concluídas |
| `implement.md` | execução | ordem real, mudanças de código, comandos de teste e rascunho de release notes |
| `report.md` | conclusão | entrega, verificação executada e pendências |

Uma etapa só começa com o `clarify.md` sem perguntas abertas.

## Etapas

| # | Etapa | Versão | Entregável |
|---|---|---|---|
| 001 | [Fundação operável](001-fundacao/spec.md) | 0.1.0 | Contêiner sobe, migra, responde health e faz backup no MinIO; testes rápidos em shards e CI/CD com deploy na `main` |
| 002 | [Identidades e as três vias](002-identidades-e-tres-vias/spec.md) | 0.2.0 | Apps, agentes, tokens com escopos, auditoria e documentação por REST, MCP e CLI |
| 003 | [Canais e assinaturas](003-canais-e-assinaturas/spec.md) | 0.3.0 | Rede de canais e assinaturas com acesso efetivo |
| 004 | [Memórias](004-memorias/spec.md) | 0.4.0 | CRUD versionado, busca textual, configurações base e skill |
| 005 | [Contexto](005-contexto/spec.md) | 0.5.0 | `get_context` determinístico com ETag — **marco: MVP do primeiro consumidor** |
| 006 | [Redator](006-redator/spec.md) | 0.6.0 | Fila de jobs, workers, segredos e Redator (título e texto) |
| 007 | [Embeddings e busca híbrida](007-embeddings-e-busca-hibrida/spec.md) | 0.7.0 | Embeddings local/remoto, sqlite-vec, RRF, `similar`, dicas e guarda de sentido |
| 008 | [Integração](008-integracao/spec.md) | 0.8.0 | Webhooks, importação, métricas, status completo e retenção — **marco: migração de consumidor e gestor** |
| 009 | [Consolidação](009-consolidacao/spec.md) | 0.9.0 | Duplicatas, importância, desuso e `mark_verified` |

Escala (índice `vec0` particionado e `Store` Postgres + pgvector) é condicional e só ganha spec quando uma medição justificar.

## Regras que valem para todas as etapas

- **Três vias desde a 002.** Toda operação nasce no `core` e é exposta em REST (com `#[utoipa::path]`), MCP e CLI na mesma etapa, com guia em `docs/guides/` e códigos de erro documentados.
- **Testes rápidos desde a 001** (mesmas estratégias do `opex` e do `dndicas`, traduzidas para Rust; detalhes em [`001-fundacao/clarify.md`](001-fundacao/clarify.md) D-011 a D-018):
  - `cargo-nextest`, um processo por teste (isolamento mantido) e `test-threads` = número de CPUs;
  - um único binário de integração `tests/it/`, com um módulo amplo por domínio; nunca um arquivo novo direto em `tests/`;
  - inicializações pesadas preguiçosas, para só o teste que usa pagar o custo;
  - CI compila os testes uma vez e roda em 4 shards (`Test (i/4)`), com lint e compose em paralelo;
  - localmente, só os testes afetados; suíte completa só a pedido.
- **CD como no `opex`** (skill `setup-ci-cd`): na `main`, imagem construída no runner, transferida por SCP e recriada com Compose sem build nem pull no servidor.
- **Definição de pronto**: `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings`, testes afetados durante o trabalho (`cargo nextest run <filtro>`) e os checks do CI verdes no PR; `docs/openapi.json` regenerado; `CHANGELOG.md` e versão MINOR; `aicontext/README.md` (estado atual) e guias de módulo em `aicontext/`; decisões em `docs/decisions/`; `docs/design.md` ajustado quando a implementação divergir.
- **Migrações**: uma por etapa (`migrations/NNNN_nome.sql`), criando só as tabelas que a etapa usa. Nunca editar migração aplicada.
- **Referência**: quando o desenho não decide um detalhe e o sap-middleman já resolveu, segue-se o sap-middleman literalmente, ajustando só os nomes. Quando o desenho decide, vale o desenho.

## Decisões transversais

Registradas em [`001-fundacao/clarify.md`](001-fundacao/clarify.md) e [`002-identidades-e-tres-vias/clarify.md`](002-identidades-e-tres-vias/clarify.md), e promovidas a `docs/decisions/` na etapa em que entram no código.
