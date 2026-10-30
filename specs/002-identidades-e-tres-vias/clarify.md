# Clarify - identidades e as três vias

**Atualizado em**: 2026-10-30

## Perguntas

| ID | Tópico | Pergunta | Status | Resolução |
|----|--------|----------|--------|-----------|
| Q-001 | Formato do token | O desenho usa `tk_…` como id público e o `.env.example` mostra um UUID no `token_id` do bootstrap. Qual vale dentro de `eng.<token_id>.<segredo>`? | aberta | Recomendação: o id público `tk_<hex>` (ex.: `eng.tk_3f9e1a7c.<segredo>`), um identificador só; o `.env.example` e o desenho §16 passam a usar `eng.tk_00000001.<segredo>`. |
| Q-002 | Hash do token | O desenho fala em SHA-256 com pepper; o sap-middleman usa HMAC-SHA256 com o pepper como chave. | aberta | Recomendação: HMAC-SHA256 (D-001) e ajuste do §4 do desenho. |
| Q-003 | Remoção de app/agente | O que acontece com memórias de autoria de um app ou agente excluído? | aberta | Recomendação: D-006. |
| Q-004 | Operações de documentação no REST | O sap-middleman só expõe documentação pelo MCP; o desenho exige as três vias. | resolvida | Seguir o desenho: `/v1/docs/*` e `engramind docs …`. |

## Decisões

| ID | Decisão | Fonte | Impacto |
|----|---------|-------|---------|
| D-001 | `TokenService` portado do sap-middleman: HMAC-SHA256 com `TOKEN_PEPPER`, segredo de 32 bytes em base64url sem padding, parser estrito (prefixo `hk`, segredo ≥ 32 caracteres `[A-Za-z0-9_-]`), pepper check. | sap-middleman `auth/token.rs` | `docs/decisions/0007-tokens-hmac.md`. |
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

## Bloqueios

Q-001 a Q-003 precisam de resposta antes da migração `0002`.
