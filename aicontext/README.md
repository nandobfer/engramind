# Contexto para desenvolvimento assistido

Este diretório contém regras operacionais concisas para qualquer trabalho no Engramind. O desenho completo, com justificativas, está em [`../docs/design.md`](../docs/design.md).

## Estado atual

Versão **0.8.0**: etapas **001 (Fundação operável)**, **002 (Identidades e as três vias)**, **003 (Canais e assinaturas)**, **004 (Memórias)**, **005 (Contexto)**, **006 (Redator)**, **007 (Embeddings e busca híbrida)** e **008 (Integração)** implementadas. O plano está em [`../specs/`](../specs/README.md); a próxima etapa é a **009 (Consolidação)**.

Existe de fato:

- binário `engramind` (também `eng`) com `serve`, `migrate`, `doctor`, `help --json`, `profiles` e um comando cliente por operação (`--json` em todos);
- configuração de infraestrutura validada no startup (`src/config.rs`), logs `compact`/`json` com `x-request-id` e um evento `http.request.completed` por requisição, com `app`, `agent`, `token_id` e `operation`;
- envelope de erro `{ error: { code, message, details? } }` com o catálogo `ERROR_CODES` (status e código de saída da CLI) em `src/error.rs`;
- `core` com `Core<S: Store>`: catálogo `OPERATIONS`, autorização por tipo e escopo, autenticação Bearer (`eng.tk_….<segredo>`, HMAC-SHA256), rate limit por token, idempotência genérica, auditoria, bootstrap e documentação viva;
- identidades (apps, agentes, tokens), configurações (`limits.*`), `get_status` básico e `get_audit` — ver [`identities.md`](identities.md) e [`settings.md`](settings.md);
- canais, assinaturas (exatas e por prefixo, modos `inject`/`search`/`manage`) e o resolvedor de acesso efetivo `core::access`, com o cabeçalho `Engramind-Channels` nas três vias — ver [`channels.md`](channels.md);
- memórias (criação, edição versionada, status, `move`, `revert`, `delete`, histórico), busca textual FTS5, `similar`, `channel_stats`, configurações por canal, a fila `jobs` (só enfileiramento) e a skill da CLI — ver [`memories.md`](memories.md);
- contexto `get_context` determinístico (duas faixas, `tagged`/`markdown`/`json`, `version`/ETag, `304` sem banco, cache LRU), `injected_count`, `usage_log`, `exclude_context` na busca e, no MCP, os resources `engramind://context`, `engramind://channels/{slug}`, `engramind://guides/{id}` e o prompt `memory-writing` — ver [`context.md`](context.md);
- fila `jobs` com runner do `redact` (captura com lease, backoff, superados, `list_jobs`, `retry_job`), cofre de segredos AES-256-GCM (`set_secret`), cliente LLM próprio (OpenAI-compatível, Anthropic, Gemini, `fallback_models`), Redator (título; texto revisado só com a guarda de sentido), `llm_calls`, `test_redator` e `get_status` com fila e Redator — ver [`jobs.md`](jobs.md) e [`redator.md`](redator.md);
- embeddings: provedores `local` (fastembed + ONNX Runtime por `ORT_DYLIB_PATH`), `openai_compatible`, `ollama`, `gemini` e `off`; job `embed` (lotes, compare-and-set, `model_id` vigente em `schema_meta`), troca de modelo com reindexação atômica, `reembed_all`, `test_embedding`, `list_embedding_models`, `get_status.embedding` e a guarda de sentido real — ver [`embeddings.md`](embeddings.md);
- busca híbrida (`search_memories` com ramo vetorial, RRF ponderado e `mode`) e `get_hints` — ver [`search.md`](search.md);
- integração: eventos em toda mutação de memória, canal, assinatura e configuração (inclusive o Redator), webhooks por app (URL e segredo cifrado, HMAC-SHA256, job `webhook` com backoff próprio), `import_memories`, `get_usage`, `get_status` completo, `/metrics` (token `admin`·`read`) e a retenção horária — ver [`webhooks.md`](webhooks.md);
- REST em `/v1` com `#[utoipa::path]` em toda rota e o snapshot `docs/openapi.json`; MCP em `POST /mcp` com uma ferramenta por operação, filtrada pelo token; guias em `docs/guides/` compilados no binário;
- `Store` SQLite com as migrações `0001_init.sql`, `0002_identities.sql`, `0003_channels.sql`, `0004_memories.sql`, `0005_usage.sql`, `0006_redator.sql`, `0007_vectors.sql` e `0008_integration.sql`;
- imagem Docker com Litestream e a biblioteca do ONNX Runtime, compose de um serviço, `scripts/check-compose.sh`, `scripts/test.sh`;
- testes com `cargo-nextest` (unitários e o binário `tests/it/`, com a matriz tipo × escopo × operação nas três vias, o cenário de canais do §19, as invariantes das memórias e os testes de contrato do §15, inclusive a skill) e CI/CD em `.github/workflows/ci-cd.yml`.

Ainda não existe: a consolidação (`run_consolidation`, `mark_verified`, com o cosseno em `similar`).

Atualize esta seção a cada fase concluída, listando as capacidades que de fato existem.

## Ordem de leitura

Leia sempre:

1. [`use-sempre-que-desenvolver.md`](use-sempre-que-desenvolver.md)
2. [`use-para-entender-o-proposito-e-arquitetura.md`](use-para-entender-o-proposito-e-arquitetura.md)

Depois leia o guia aplicável:

- REST, OpenAPI, MCP, CLI e skill: [`use-quando-desenvolver-api.md`](use-quando-desenvolver-api.md)
- Banco, `Store`, migrações, FTS5 e sqlite-vec: [`use-quando-desenvolver-persistencia.md`](use-quando-desenvolver-persistencia.md)
- Redator, embeddings, webhooks e workers: [`use-quando-integrar-provedores.md`](use-quando-integrar-provedores.md)
- Docker, Litestream e reverse proxy: [`use-quando-configurar-deploy.md`](use-quando-configurar-deploy.md)

Por fim, consulte as seções relacionadas de [`../docs/design.md`](../docs/design.md).

## Guias por módulo

Quando um módulo do domínio for implementado (ex.: memórias, canais, contexto, busca), crie aqui um arquivo curto com o nome do módulo (ex.: `memories.md`) contendo:

- onde o código vive;
- operações, `operationId` e tags;
- escopos exigidos e regras de acesso;
- invariantes que não podem ser quebradas;
- referência ao guia em `docs/guides/` e à seção do desenho.

Não crie guias de módulo antes do código existir.

## Papel de cada área

- `aicontext/`: regras normativas, curtas e acionáveis.
- `docs/design.md`: desenho e decisões tomadas; fonte de verdade do que ainda não foi implementado.
- `docs/guides/`: guias de uso compilados no binário e servidos por `get_guide`.
- `docs/decisions/`: decisões arquiteturais posteriores ao desenho (`0001` a `0006` na 001; `0007` a `0009` na 002; `0010` na 003; `0011` e `0012` na 004; `0013` e `0014` na 005; `0015` a `0017` na 006; `0018` a `0021` na 007; `0022` a `0024` na 008).
- `specs/NNN-etapa/`: plano de cada etapa entregável (`spec`, `clarify`, `contracts`, `plan`, `tasks`; `implement` e `report` na execução).

## Regra de atualização

Quando a implementação alterar comportamento documentado, atualize no mesmo trabalho o guia normativo, o desenho, os guias de uso, os testes, o contrato OpenAPI e a versão aplicáveis.
