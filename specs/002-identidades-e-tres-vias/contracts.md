# Contracts - identidades e as três vias

**Criado em**: 2026-10-30
**Status**: Draft
**Feature**: [`spec.md`](spec.md)

## Módulos

```text
src/core/mod.rs            # Core<S: Store>: ponto de entrada das operações
src/core/principal.rs      # Principal, TokenKind, Scope, ScopeSet
src/core/authz.rs          # require(principal, operation)
src/core/operations.rs     # OPERATIONS (catálogo único)
src/core/identities/       # apps, agents, tokens (model, service)
src/core/settings/         # catálogo SETTINGS, validação, service
src/core/audit.rs
src/core/idempotency.rs
src/core/status.rs
src/core/docs/             # catalog (guias), openapi_index, search (portados)
src/auth/token.rs          # TokenService (portado)
src/auth/middleware.rs     # require_auth → Principal nas extensions
src/auth/rate_limit.rs
src/rest/<grupo>.rs        # handlers finos + #[utoipa::path]
src/mcp/                   # server, tools, contract, transport
src/cli/                   # client, profiles, commands/<grupo>.rs, output.rs, exit_codes.rs
docs/guides/*.md
```

## Tipos centrais

```rust
pub enum TokenKind { Agent, Admin }
pub enum Scope { Read, Write, Delete }
pub struct ScopeSet(u8);               // invariante: write/delete ⇒ read

pub struct Principal {
    pub token_id: TokenId,
    pub app: AppRef,                   // id + slug
    pub agent: Option<AgentRef>,       // id + slug
    pub kind: TokenKind,
    pub scopes: ScopeSet,
}

pub struct OperationSpec {
    pub name: &'static str,            // = operationId = ferramenta MCP
    pub group: Group,
    pub tag: &'static str,
    pub kind: RequiredKind,            // Any | Admin
    pub scope: Option<Scope>,          // None = qualquer token (whoami, docs)
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
    pub method: Method,
    pub path: &'static str,
    pub cli: &'static [&'static str],  // ["tokens", "issue"]
}
```

## `Store` (extensão)

```rust
async fn find_auth_token(&self, id: &TokenId) -> Result<Option<AuthTokenRow>, StoreError>;
async fn count_tokens(&self) -> Result<u64, StoreError>;
async fn create_app(&self, new: NewApp, audit: AuditEntry, idem: Option<IdemRecord>) -> Result<App, StoreError>;
// … list/get/update/delete de apps, agents e tokens no mesmo formato
async fn touch_tokens(&self, batch: Vec<(TokenId, DateTime<Utc>)>) -> Result<(), StoreError>;
async fn find_idempotency(&self, token: &TokenId, key: &str) -> Result<Option<IdemRow>, StoreError>;
async fn list_audit(&self, filter: AuditFilter, page: PageRequest) -> Result<Page<AuditEvent>, StoreError>;
async fn get_settings(&self) -> Result<Vec<SettingRow>, StoreError>;
async fn update_settings(&self, changes: Vec<SettingChange>, audit: AuditEntry, idem: Option<IdemRecord>) -> Result<(), StoreError>;
```

Toda mutação recebe a entrada de auditoria e o registro de idempotência para gravar na mesma transação.

## Migração `0002_identities.sql`

```sql
apps        (id TEXT PK, slug TEXT UNIQUE, name, description, created_at, disabled_at)
agents      (id TEXT PK, app_id TEXT FK→apps ON DELETE CASCADE, slug, name, description,
             created_at, disabled_at, UNIQUE(app_id, slug))
tokens      (id TEXT PK, app_id FK ON DELETE CASCADE, agent_id NULL FK ON DELETE CASCADE,
             kind CHECK IN ('agent','admin'), scopes TEXT, label, secret_hash BLOB,
             expires_at NULL, created_at, last_used_at NULL, revoked_at NULL)
settings    (key TEXT PK, value_json TEXT, updated_by_app TEXT, updated_at)
idempotency (token_id, key, request_hash BLOB, response_json TEXT, status INTEGER, created_at,
             PRIMARY KEY (token_id, key))
audit_log   (id INTEGER PK, at, app_id, agent_id, token_id, action, entity, entity_id,
             before_json, after_json)
```

Todas `STRICT`. Ids públicos `app_`, `ag_`, `tk_` + 8 hex minúsculos, gerados com retentativa em colisão.

## Operações

| Operação | Tipo·escopo | REST | CLI |
|---|---|---|---|
| `whoami` | qualquer | `GET /v1/whoami` | `engramind whoami` |
| `list_apps` | admin·read | `GET /v1/apps` | `engramind apps list` |
| `get_app` | admin·read | `GET /v1/apps/{app}` | `engramind apps get <app>` |
| `create_app` | admin·write | `POST /v1/apps` | `engramind apps create` |
| `update_app` | admin·write | `PATCH /v1/apps/{app}` | `engramind apps update <app>` |
| `delete_app` | admin·delete | `DELETE /v1/apps/{app}` | `engramind apps delete <app>` |
| `list_agents` | admin·read | `GET /v1/agents?app=` | `engramind agents list` |
| `get_agent` | admin·read | `GET /v1/agents/{agent}` | `engramind agents get <agent>` |
| `create_agent` | admin·write | `POST /v1/agents` | `engramind agents create` |
| `update_agent` | admin·write | `PATCH /v1/agents/{agent}` | `engramind agents update <agent>` |
| `delete_agent` | admin·delete | `DELETE /v1/agents/{agent}` | `engramind agents delete <agent>` |
| `list_tokens` | admin·read | `GET /v1/tokens?app=&agent=` | `engramind tokens list` |
| `issue_token` | admin·write | `POST /v1/tokens` | `engramind tokens issue` |
| `update_token` | admin·write | `PATCH /v1/tokens/{token}` | `engramind tokens update <token>` |
| `revoke_token` | admin·delete | `POST /v1/tokens/{token}/revoke` | `engramind tokens revoke <token>` |
| `get_settings` | admin·read | `GET /v1/settings` | `engramind settings get [chave]` |
| `update_settings` | admin·write | `PUT /v1/settings` | `engramind settings set <chave> <valor>` |
| `get_status` | admin·read | `GET /v1/status` | `engramind status` |
| `get_audit` | admin·read | `GET /v1/audit` | `engramind audit list` |
| `get_overview` | qualquer | `GET /v1/docs/overview` | `engramind docs overview` |
| `list_guides` | qualquer | `GET /v1/docs/guides` | `engramind docs guides` |
| `get_guide` | qualquer | `GET /v1/docs/guides/{guide}?section=` | `engramind docs guide <id>` |
| `search_docs` | qualquer | `GET /v1/docs/search?q=&limit=` | `engramind docs search <termos>` |
| `list_operations` | qualquer | `GET /v1/docs/operations?guide=&tag=` | `engramind docs operations` |
| `get_operation` | qualquer | `GET /v1/docs/operations/{operation}` | `engramind docs operation <nome>` |
| `get_schema` | qualquer | `GET /v1/docs/schemas/{schema}` | `engramind docs schema <nome>` |

`{app}` e `{agent}` aceitam id (`app_…`, `ag_…`) ou slug (`app-a`, `app-a/desenvolvedor` codificado como `app-a%2Fdesenvolvedor`).

Comandos locais da CLI (sem operação): `serve`, `migrate`, `doctor`, `help`, `profiles add|list|remove|use`.

Rota técnica: `POST /mcp` (`mcp`, tag MCP, Bearer), contrato documentado como no sap-middleman.

## Corpos principais

```jsonc
// POST /v1/tokens
{ "app": "app-a", "agent": "app-a/curador", "kind": "agent", "scopes": ["read","write"],
  "label": "curador produção", "expires_at": null }
// 201
{ "token": { "id": "tk_3f9e1a7c", "kind": "agent", "scopes": ["read","write"], "app": {...}, "agent": {...},
             "label": "...", "expires_at": null, "created_at": "..." },
  "secret": "eng.tk_3f9e1a7c.<segredo>" }        // única vez

// GET /v1/whoami
{ "token_id": "tk_…", "kind": "agent", "scopes": ["read"], "app": {...}, "agent": {...},
  "operations": ["whoami", "get_overview", ...] }   // operações permitidas

// PUT /v1/settings
{ "limits.rate_per_minute": 600, "limits.max_batch": 100 }
// GET /v1/settings
{ "settings": [ { "key": "limits.rate_per_minute", "value": 600, "default": 600, "type": "integer",
                  "channel_override": false, "updated_at": null } ] }
```

## Configurações desta etapa

| Chave | Tipo | Padrão | Por canal |
|---|---|---|---|
| `limits.rate_per_minute` | inteiro 1–100000 | 600 | não |
| `limits.max_batch` | inteiro 1–1000 | 100 | não |

## Códigos de erro novos

| Código | Status |
|---|---|
| `AUTH_INVALID` | 401 |
| `SCOPE_REQUIRED` | 403 |
| `APP_NOT_FOUND`, `AGENT_NOT_FOUND`, `TOKEN_NOT_FOUND`, `GUIDE_NOT_FOUND`, `OPERATION_NOT_FOUND`, `SCHEMA_NOT_FOUND`, `SECTION_NOT_FOUND` | 404 |
| `SLUG_ALREADY_EXISTS`, `IDEMPOTENCY_KEY_CONFLICT`, `LAST_ADMIN_TOKEN` | 409 |
| `SETTING_UNKNOWN`, `SETTING_INVALID` | 400 |
| `RATE_LIMITED` | 429 |
| `MCP_BAD_REQUEST`, `MCP_INVALID_MESSAGE` | 400/422 |
| `MCP_HOST_FORBIDDEN` | 403 |
| `MCP_NOT_ACCEPTABLE` | 406 |
| `MCP_UNSUPPORTED_MEDIA_TYPE` | 415 |

## Testes de contrato (`tests/it/contract.rs`)

1. `docs/openapi.json` igual ao gerado (`UPDATE_SNAPSHOTS=1` regenera).
2. Toda rota do router tem `#[utoipa::path]`, e todo `operationId` está em `OPERATIONS` (exceto as técnicas).
3. Toda entrada de `OPERATIONS` tem rota REST, ferramenta MCP e comando CLI, e aparece em exatamente um guia (por tag).
4. Todo comando de `engramind help --json` corresponde a uma operação ou a um comando local; todo `engramind …` citado em `docs/guides/` existe.
5. Todo código de `ERROR_CODES` aparece em `docs/guides/erros.md` e no mapa de códigos de saída.
6. Toda chave de `SETTINGS` aparece em `docs/guides/configuracoes.md` com tipo e padrão.
