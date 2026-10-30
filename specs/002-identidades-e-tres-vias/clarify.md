# Clarify - identidades e as três vias

**Atualizado em**: 2026-10-30

## Perguntas

| ID | Tópico | Pergunta | Status | Resolução |
|----|--------|----------|--------|-----------|
| Q-001 | Formato do token | O desenho usa `tk_…` como id público e o desenho §16 mostrava um UUID no `token_id` do bootstrap. Qual vale dentro de `eng.<token_id>.<segredo>`? | resolvida | O id público `tk_<8 hex>` (ex.: `eng.tk_3f9e1a7c.<segredo>`), um identificador só; o desenho §16 e o `.env.example` usam `eng.tk_00000001.<segredo>` (D-001). |
| Q-002 | Hash do token | O desenho fala em SHA-256 com pepper; o sap-middleman usa HMAC-SHA256 com o pepper como chave. | resolvida | HMAC-SHA256 (D-001); desenho §4, `AGENTS.md` e decisão 0007 ajustados. |
| Q-003 | Remoção de app/agente | O que acontece com memórias de autoria de um app ou agente excluído? | resolvida | D-006. |
| Q-005 | Último admin | `LAST_ADMIN_TOKEN` vale só para revogar/remover o token, ou para toda mudança que derrube o último admin ativo com `write`? | resolvida | Toda mudança: revogar, reduzir escopos, expirar, desativar ou excluir app e agente (D-015). |
| Q-006 | Entrega | Um PR único ou PRs incrementais? | resolvida | Um PR único, como na 001. |
| Q-004 | Operações de documentação no REST | O sap-middleman só expõe documentação pelo MCP; o desenho exige as três vias. | resolvida | Seguir o desenho: `/v1/docs/*` e `engramind docs …`. |

## Decisões

| ID | Decisão | Fonte | Impacto |
|----|---------|-------|---------|
| D-001 | `TokenService` portado do sap-middleman: HMAC-SHA256 com `TOKEN_PEPPER`, segredo de 32 bytes em base64url sem padding, parser estrito (esquema `eng`, id `tk_` + 8 hex, segredo ≥ 32 caracteres `[A-Za-z0-9_-]`). O verificador do pepper continua em `core/pepper.rs` (001). | sap-middleman `auth/token.rs` | `docs/decisions/0007-tokens-hmac.md`. |
| D-002 | `core::operations::OPERATIONS` é a fonte única de cada operação: nome base, grupo, tag OpenAPI, tipo exigido (`any`/`admin`), escopo, hints MCP, método/rota REST e caminho da CLI. Autorização, MCP, CLI e testes de contrato leem dela. | desenho §13, §15 | `docs/decisions/0008-catalogo-de-operacoes.md`. |
| D-003 | `operationId` = nome base em snake_case (`issue_token`), igual ao nome da ferramenta MCP. | `aicontext/use-quando-desenvolver-api.md` | Diverge do camelCase do sap-middleman. |
| D-004 | MCP: um `EngramindServer` (`rmcp` `ServerHandler`) com `list_tools` sobrescrito para filtrar o `ToolRouter` pelo `Principal` lido de `RequestContext.extensions` (mesmo mecanismo do `caller()` do sap-middleman). Transporte, `NeverSessionManager`, `MCP_ALLOWED_HOSTS` e `normalize_transport_errors` portados. | sap-middleman `mcp/` | Ferramentas de documentação viram parte do mesmo servidor. |
| D-005 | Erro de domínio no MCP: `CallToolResult` com `isError: true` e `structuredContent` = envelope `{ error: { code, message, details } }`, para o agente ler o mesmo `code` do REST. Erros de protocolo continuam JSON-RPC. | desenho §13 | Documentado em `integracao-mcp`. |
| D-006 | `delete_app`/`delete_agent` removem o registro, os agentes, tokens e (a partir da 003) assinaturas; as colunas de autoria das memórias guardam o id sem chave estrangeira, e a auditoria preserva `slug`/`name` no `before_json`. Para tirar de uso sem perder o vínculo, `update_app`/`update_agent` com `disabled: true`. | desenho §6 (proveniência) | Proveniência sobrevive à exclusão. |
| D-007 | `last_used_at` e o rate limit vivem em memória (`Mutex<HashMap>` com janela fixa de 60 s); `last_used_at` é gravado em lote a cada 10 s pela thread escritora. | desenho §6 | Rate limit é por instância (há só uma). |
| D-008 | Idempotência: `request_hash` = SHA-256 de `operação + corpo canônico`; a reserva e a gravação da resposta acontecem na mesma transação da mutação; retenção de 24 h aplicada por tarefa periódica. | desenho §6, §12 | Tabela `idempotency` nasce nesta etapa. |
| D-009 | Paginação por cursor opaco (base64url de `(created_at, id)`), ordem estável `created_at, id`. | desenho §13 | — |
| D-010 | CLI: perfis em `~/.config/engramind/config.toml` (`[profiles.<nome>] url, token, channels` e `default_profile`); o arquivo é criado com permissão `0600` por `engramind profiles add`. | desenho §13 | — |
| D-011 | Códigos de saída da CLI: 0 sucesso; 2 uso inválido; 3 `AUTH_INVALID`; 4 `SCOPE_REQUIRED`/`CHANNEL_NOT_SUBSCRIBED`; 5 `*_NOT_FOUND`; 6 conflitos (`409`); 7 `RATE_LIMITED`; 8 servidor indisponível ou rede; 1 qualquer outro. A tabela vive no catálogo de erros. | desenho §13 | Estável; mudança é quebra de contrato. |
| D-012 | Guias em `docs/guides/<id>.md` com front matter mínimo no catálogo Rust (id, público, tags), como `mcp/catalog.rs` do sap-middleman. | sap-middleman | — |
| D-013 | Configurações: tabela `settings(key, value_json, updated_by_app, updated_at)`, um catálogo Rust `SETTINGS` com chave, tipo, padrão, validação, se aceita sobreposição por canal e a etapa em que entra. `update_settings` aceita um mapa parcial de chaves; chave desconhecida → `SETTING_UNKNOWN`. | desenho §12 | O catálogo é a base do teste de contrato 6. |

| D-014 | Idempotência de `issue_token`: o segredo nunca é persistido, então a repetição com a mesma chave devolve o token sem `secret`. | FR-002, D-008 | Documentado no guia `tokens-e-escopos` e no OpenAPI. |
| D-015 | `LAST_ADMIN_TOKEN` é conferido na transação de toda mudança que pode desativar um token `admin` com `write`: se havia algum antes e não sobra nenhum, a transação é desfeita. | Q-005 | Decisão 0008. |
| D-016 | A repetição idempotente é conferida antes de resolver os alvos: repetir uma exclusão devolve a resposta gravada, não `404`. | D-008 | `Core::mutate`. |
| D-017 | MCP: argumentos planos (caminho + query + corpo), mais `idempotency_key`; `update_settings` recebe `settings`. Argumentos fora do esquema viram `VALIDATION_ERROR` com `isError: true`. | D-004, D-005 | Decisão 0009; teste de contrato de paridade de parâmetros. |
| D-018 | `issue_token` aceita `app` omitido quando `agent` é informado (o desenho §13 usa `tokens issue --agent app-a/curador`). | desenho §13 | Corpo `IssueTokenBody.app` opcional. |
| D-019 | CLI: `--url`, `--token`, `--channels` e `--profile` são globais; `profiles add` grava os valores dessas mesmas flags (ou das variáveis). Erros locais da CLI (`CLI_CONFIG_INVALID` 2, `CLI_CONNECTION_FAILED` 8, `CLI_UNEXPECTED_RESPONSE` 1) ficam fora do catálogo do servidor e são documentados no guia `cli`. | D-010, D-011 | Teste de contrato confere o guia. |
| D-020 | O rate limit e o `last_used_at` vivem em `core::usage` (não em `src/auth/rate_limit.rs`), porque a autenticação é regra do `core`; o middleware só extrai o Bearer. | D-007 | — |
| D-021 | Tags OpenAPI: `Authentication` (`whoami`), `Identities`, `Settings`, `Status`, `Audit`, `Documentation` e `MCP`. Guias: `overview` (Health, Operations, Documentation), `tokens-e-escopos` (Authentication, Identities), `integracao-mcp` (MCP), `configuracoes` (Settings, admin), `operacao` (Status, Audit, admin), `cli` e `erros` (sem tags). | D-012 | — |

## Bloqueios

Nenhum. Q-001 a Q-003, Q-005 e Q-006 foram resolvidas seguindo as recomendações.
