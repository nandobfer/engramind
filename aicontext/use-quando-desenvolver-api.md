# Use quando desenvolver API (REST, MCP e CLI)

## Uma operação, três vias

- Toda operação é definida uma vez no `core` e exposta em REST, MCP e CLI com o mesmo nome base, parâmetros, escopo e erros (§13 do desenho).
- O adaptador só traduz entrada e saída. Validação, escopo, acesso efetivo e auditoria ficam no `core`.
- Operações de lista são paginadas (`cursor`, `limit`) e aceitam `fields` (`title`/`full`).
- Toda operação aceita `channels` para restringir o alcance, sempre dentro do que o token já alcança.

## Erros

```json
{
  "error": {
    "code": "SCOPE_REQUIRED",
    "message": "Esta operação exige o escopo write.",
    "details": { "required": "write" }
  }
}
```

- `code` é estável, em inglês e parte do contrato; `message` é em pt-BR.
- `details` nunca contém segredos, tokens ou texto de memória de canais não alcançados.
- Diferencie validação (`400`/`422`), autenticação (`401`), escopo/canal (`403`), inexistente (`404`), conflito (`409`), limite (`429`), indisponibilidade de provedor e erro interno.
- Todo código novo entra no guia de erros e no mapeamento de códigos de saída da CLI.

## REST

- Rotas de domínio em `/v1`. Públicas fora de `/v1`: `/health/live`, `/health/ready`, `/openapi.json`, `/docs`. `/mcp` exige token; `/metrics` exige token `admin`·`read` (fora do catálogo, `operationId` `get_metrics`, decisão 0022).
- `get_context` responde `ETag` com a `version` e `304` para `If-None-Match` igual.
- Mutações aceitam `Idempotency-Key`; edições aceitam `expected_version`.
- Gere ou preserve `x-request-id` em toda resposta.

## OpenAPI e Swagger

- Toda rota tem `#[utoipa::path]` com `operationId` único e estável (o nome base da operação), tag, resumo, descrição, parâmetros, body, todos os status e schemas.
- Schemas derivam dos mesmos tipos Rust da validação; não duplique contratos equivalentes.
- Security scheme HTTP Bearer; operações protegidas o declaram, rotas públicas declaram a ausência. O Swagger em `/docs` oferece `Authorize`.
- `servers: [{ url: "." }]` e redirects relativos, para funcionar atrás de proxy com prefixo.
- `docs/openapi.json` versionado igual ao gerado; regenere com `UPDATE_SNAPSHOTS=1`.
- Exemplos sanitizados, sem segredos nem dados pessoais.

## MCP

- `POST /mcp`, Streamable HTTP stateless, JSON-RPC com resposta `application/json`.
- `tools/list` mostra **só as ferramentas que o token pode usar** (tipo e escopos).
- Ferramentas declaram `read_only_hint`, `destructive_hint` e `idempotent_hint`.
- O cabeçalho `Engramind-Channels` define o filtro padrão de canais da conexão.
- Resources `engramind://context`, `engramind://channels/{slug}`, `engramind://guides/{id}`; prompt `memory-writing`.
- Ferramentas de documentação usam o índice do OpenAPI e os guias embutidos; guias `admin` ficam ocultos para tokens `agent`.
- `MCP_ALLOWED_HOSTS` valida o `Host`.

## CLI

- `engramind <grupo> <ação>`, cliente HTTP da API; configuração por `ENGRAMIND_URL`, `ENGRAMIND_TOKEN`, `ENGRAMIND_CHANNELS` ou perfis em `~/.config/engramind/config.toml`.
- `--json` em todo comando; códigos de saída estáveis mapeados dos códigos de erro.
- `engramind help --json` descreve todos os comandos e é a base dos testes da skill.
- Toda escrita no terminal passa pelo módulo de saída da CLI.

## Documentação viva

- Toda operação nova atualiza o guia correspondente em `docs/guides/`, a skill (quando a CLI muda) e o snapshot do OpenAPI no mesmo trabalho.
- Os testes de contrato do §15 precisam continuar verdes.

## Revisão obrigatória

Antes de concluir uma operação, confirme:

- A regra está no `core` e as três vias se comportam igual.
- Escopo e acesso efetivo estão cobertos pela matriz de testes.
- Entrada e saída são tipadas; erros documentados.
- OpenAPI, `/docs`, MCP, CLI, guia e skill representam o comportamento real.
- Logs e métricas não expõem tokens, segredos ou texto de memórias.
