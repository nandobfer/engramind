# Spec - identidades e as três vias

**Criado em**: 2026-10-30
**Status**: Implementada
**Versão alvo**: 0.2.0
**Input**: Desenho §4, §12 (grupo `limits`), §13 (Identidades, Configurações, Operação e Documentação), §15, §18, §20.

## Objetivo

Permitir que um sistema gestor provisione a rede de identidades (apps, agentes e tokens com escopos) e que qualquer token consulte a documentação viva, por REST, MCP e CLI, com auditoria, idempotência, rate limit e os testes de contrato que travam as três vias e a documentação daqui em diante.

## Escopo

- Tokens `eng.<token_id>.<segredo>` com HMAC-SHA256 + `TOKEN_PEPPER`, tipos `agent`/`admin`, escopos `read`/`write`/`delete`, expiração, revogação e `last_used_at` em lote.
- Bootstrap do primeiro token `admin` por `BOOTSTRAP_ADMIN_TOKEN`, aceito só com o banco sem tokens.
- `Principal { app, agent?, kind, scopes, token_id }` e autorização no `core`.
- Apps, agentes, tokens, auditoria e status básico.
- Infraestrutura de configurações (`get_settings`, `update_settings`) com o grupo `limits`.
- Rate limit por token (`limits.rate_per_minute`).
- Idempotência genérica (`Idempotency-Key`).
- Catálogo único de operações (`core::operations`) que alimenta escopos, MCP, CLI e testes de contrato.
- MCP em `POST /mcp`: ferramentas de operação e de documentação, filtradas pelo token.
- CLI cliente (`engramind <grupo> <ação>`), perfis, `--json`, códigos de saída e `engramind help --json`.
- Guias embutidos e operações de documentação nas três vias.
- Testes de contrato do §15 (itens 1 a 6, com a skill a partir da 004).

## Requisitos

| ID | Requisito |
|----|-----------|
| FR-001 | Token ausente, malformado, desconhecido, expirado, revogado, de app/agente desativado ou com HMAC divergente → `401 AUTH_INVALID`; a comparação usa `Mac::verify_slice` (tempo constante). |
| FR-002 | O segredo aparece só na resposta de `issue_token` (e no `.env` do bootstrap); listagens mostram `token_id`, `label`, prefixo e metadados. |
| FR-003 | Escopos válidos: `read`, `read+write`, `read+delete`, `read+write+delete`; outras combinações → `400 VALIDATION_ERROR`. |
| FR-004 | Token `agent` exige `agent_id`; token `admin` aceita `agent_id` opcional do mesmo app. |
| FR-005 | Quem emite ou altera um token não pode conceder escopos que não tem (`403 SCOPE_REQUIRED`). |
| FR-006 | Operação de tipo `admin` chamada por token `agent`, ou escopo insuficiente → `403 SCOPE_REQUIRED` com `details: { required_kind, required_scope }`. |
| FR-007 | `BOOTSTRAP_ADMIN_TOKEN` cria app `bootstrap` e token `admin` `read+write+delete` só quando não existe nenhum token; com tokens existentes é ignorado com `WARN`. |
| FR-008 | Toda mutação de identidade, token e configuração grava `audit_log` na mesma transação, sem segredos. |
| FR-009 | Mutações aceitam `Idempotency-Key` (REST), `idempotency_key` (MCP) e `--idempotency-key` (CLI); mesma chave e mesmo corpo devolvem a resposta gravada; corpo diferente → `409 IDEMPOTENCY_KEY_CONFLICT`. |
| FR-010 | Requisições acima de `limits.rate_per_minute` por token → `429 RATE_LIMITED` com `Retry-After`. |
| FR-011 | `tools/list` mostra só as ferramentas permitidas ao tipo e aos escopos do token; chamar uma ferramenta oculta devolve o mesmo erro que o REST. |
| FR-012 | Toda ferramenta MCP declara `read_only_hint`, `destructive_hint` e `idempotent_hint` a partir do catálogo de operações. |
| FR-013 | `MCP_ALLOWED_HOSTS` valida o `Host`; erros do transporte MCP usam o envelope padrão. |
| FR-014 | Toda operação existe nas três vias com o mesmo nome base, parâmetros, escopo e códigos de erro. |
| FR-015 | Guias `admin` e operações/esquemas de tags administrativas ficam ocultos para tokens `agent`. |
| FR-016 | A CLI lê `--url`/`--token`/`--channels`/`--profile`, depois `ENGRAMIND_URL`/`ENGRAMIND_TOKEN`/`ENGRAMIND_CHANNELS`, depois `~/.config/engramind/config.toml`; `--json` em todo comando; códigos de saída estáveis. |
| FR-017 | Listas paginadas por `cursor`/`limit` (padrão 50, máximo 200). |
| FR-018 | Logs de requisição incluem `app`, `agent` e `token_id`; nunca o token, o segredo ou `Authorization`. |
| FR-019 | Não é possível revogar ou remover o último token `admin` ativo com escopo `write` (`409 LAST_ADMIN_TOKEN`). |

## Fora do escopo

- Canais, assinaturas e memórias (003 e 004).
- `webhook_url` e segredo de webhook do app (008).
- Resources e prompts do MCP (005).
- `get_status` completo (008); aqui só versão, banco, migrações, backup e contagens.

## Critérios de sucesso

- Com o banco vazio e o bootstrap no `.env`, o gestor cria um app, um agente e um token `agent` pela CLI e pelo MCP, e o novo token responde `whoami`.
- A matriz de tipo × escopo × operação cobre todas as operações da etapa.
- Os testes de contrato falham ao remover uma anotação utoipa, uma ferramenta MCP, um comando da CLI, um código de erro do guia ou uma chave de configuração do guia.
- `docs/openapi.json` atualizado; guias `overview`, `tokens-e-escopos`, `configuracoes`, `cli`, `integracao-mcp`, `erros` e `operacao` servidos por `get_guide`.
