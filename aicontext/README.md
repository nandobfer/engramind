# Contexto para desenvolvimento assistido

Este diretório contém regras operacionais concisas para qualquer trabalho no Engramind. O desenho completo, com justificativas, está em [`../docs/design.md`](../docs/design.md).

## Estado atual

Versão **0.1.0**: etapa **001 (Fundação operável)** implementada. O plano está em [`../specs/`](../specs/README.md); a próxima etapa é a **002 (Identidades e as três vias)**, depois de resolver as perguntas abertas do `clarify.md` dela.

Existe de fato:

- binário `engramind` (também `eng`) com `serve`, `migrate` e `doctor` (`--json`);
- configuração de infraestrutura validada no startup (`src/config.rs`), logs `compact`/`json` com `x-request-id` e um evento `http.request.completed` por requisição;
- envelope de erro `{ error: { code, message, details? } }` com o catálogo `ERROR_CODES` (`src/error.rs`);
- `Store` com a implementação SQLite (escritor único, pool de leitores, WAL, FTS5, `sqlite-vec`), migração `0001_init.sql` (`schema_meta`) e o verificador do `TOKEN_PEPPER` (`src/core/pepper.rs`);
- rotas públicas `/health/live`, `/health/ready`, `/openapi.json`, `/docs` e `/`, com o snapshot `docs/openapi.json`;
- imagem Docker com Litestream, compose de um serviço, `scripts/check-compose.sh`, `scripts/test.sh`;
- testes com `cargo-nextest` (unitários nos módulos e o binário de integração `tests/it/`) e CI/CD em `.github/workflows/ci-cd.yml`.

Ainda não existe nenhuma rota de domínio, autenticação, MCP, guias em `docs/guides/` nem skill.

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
- `docs/guides/`: guias de uso compilados no binário (a partir da 002).
- `docs/decisions/`: decisões arquiteturais posteriores ao desenho (`0001` a `0006` na 001).
- `specs/NNN-etapa/`: plano de cada etapa entregável (`spec`, `clarify`, `contracts`, `plan`, `tasks`; `implement` e `report` na execução).

## Regra de atualização

Quando a implementação alterar comportamento documentado, atualize no mesmo trabalho o guia normativo, o desenho, os guias de uso, os testes, o contrato OpenAPI e a versão aplicáveis.
