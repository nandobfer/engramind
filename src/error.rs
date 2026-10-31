use axum::{
    Json,
    http::{
        HeaderValue, StatusCode,
        header::{RETRY_AFTER, WWW_AUTHENTICATE},
    },
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::Value;
use std::{error::Error, fmt};
use utoipa::ToSchema;

use crate::store::StoreError;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub details: Option<Value>,
    /// Segundos para o cabeçalho `Retry-After` (`429`).
    pub retry_after: Option<u64>,
}

/// Envelope único de erro: `{ error: { code, message, details? } }`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    /// Código estável, em inglês, documentado no guia de erros.
    #[schema(example = "VALIDATION_ERROR")]
    pub code: String,
    /// Mensagem em pt-BR para quem lê; não é contrato.
    #[schema(example = "Body JSON ausente ou invalido")]
    pub message: String,
    /// Campos de diagnóstico específicos da operação.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

/// Entrada do catálogo de códigos de erro (fonte do guia de erros, dos códigos de saída da CLI
/// e do teste de contrato).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorCodeDoc {
    pub code: &'static str,
    pub status: u16,
    /// Código de saída da CLI (D-011): 2 uso inválido, 3 autenticação, 4 escopo, 5 inexistente,
    /// 6 conflito, 7 limite, 8 servidor indisponível, 1 qualquer outro.
    pub exit_code: u8,
    pub description: &'static str,
}

pub const ERROR_CODES: &[ErrorCodeDoc] = &[
    ErrorCodeDoc {
        code: "VALIDATION_ERROR",
        status: 400,
        exit_code: 2,
        description: "Path, query, header ou body inválido.",
    },
    ErrorCodeDoc {
        code: "AUTH_INVALID",
        status: 401,
        exit_code: 3,
        description: "Token ausente, malformado, desconhecido, expirado, revogado ou de app/agente desativado.",
    },
    ErrorCodeDoc {
        code: "SCOPE_REQUIRED",
        status: 403,
        exit_code: 4,
        description: "O tipo ou os escopos do token (ou o acesso efetivo no canal) não permitem a operação; `details` traz `required_kind`, `required_scope` e, no canal, `channel`.",
    },
    ErrorCodeDoc {
        code: "CHANNEL_NOT_SUBSCRIBED",
        status: 403,
        exit_code: 4,
        description: "O token não alcança o canal (sem assinatura ou fora da seleção `channels`/`Engramind-Channels`); `details.channel` traz o slug.",
    },
    ErrorCodeDoc {
        code: "ROUTE_NOT_FOUND",
        status: 404,
        exit_code: 5,
        description: "Rota inexistente.",
    },
    ErrorCodeDoc {
        code: "APP_NOT_FOUND",
        status: 404,
        exit_code: 5,
        description: "App inexistente.",
    },
    ErrorCodeDoc {
        code: "AGENT_NOT_FOUND",
        status: 404,
        exit_code: 5,
        description: "Agente inexistente.",
    },
    ErrorCodeDoc {
        code: "TOKEN_NOT_FOUND",
        status: 404,
        exit_code: 5,
        description: "Token inexistente.",
    },
    ErrorCodeDoc {
        code: "CHANNEL_NOT_FOUND",
        status: 404,
        exit_code: 5,
        description: "Canal inexistente; canais nunca nascem implicitamente.",
    },
    ErrorCodeDoc {
        code: "SUBSCRIPTION_NOT_FOUND",
        status: 404,
        exit_code: 5,
        description: "Assinatura inexistente.",
    },
    ErrorCodeDoc {
        code: "MEMORY_NOT_FOUND",
        status: 404,
        exit_code: 5,
        description: "Memória inexistente ou num canal fora do alcance do token (ou do `Engramind-Channels`).",
    },
    ErrorCodeDoc {
        code: "REVISION_NOT_FOUND",
        status: 404,
        exit_code: 5,
        description: "A memória não tem a versão pedida em `revert_memory`.",
    },
    ErrorCodeDoc {
        code: "CATEGORY_NOT_FOUND",
        status: 400,
        exit_code: 2,
        description: "Categoria fora de `categories.list`; `details.valid` lista as aceitas.",
    },
    ErrorCodeDoc {
        code: "GUIDE_NOT_FOUND",
        status: 404,
        exit_code: 5,
        description: "Guia inexistente ou oculto para o token; `details.valid` lista os visíveis.",
    },
    ErrorCodeDoc {
        code: "SECTION_NOT_FOUND",
        status: 404,
        exit_code: 5,
        description: "Seção inexistente no guia; `details.valid` lista as seções.",
    },
    ErrorCodeDoc {
        code: "OPERATION_NOT_FOUND",
        status: 404,
        exit_code: 5,
        description: "Operação inexistente ou oculta para o token; `details.valid` lista as visíveis.",
    },
    ErrorCodeDoc {
        code: "SCHEMA_NOT_FOUND",
        status: 404,
        exit_code: 5,
        description: "Esquema inexistente ou oculto para o token; `details.valid` lista os visíveis.",
    },
    ErrorCodeDoc {
        code: "TAG_NOT_FOUND",
        status: 404,
        exit_code: 5,
        description: "Tag OpenAPI inexistente ou oculta para o token; `details.valid` lista as visíveis.",
    },
    ErrorCodeDoc {
        code: "RESOURCE_NOT_FOUND",
        status: 404,
        exit_code: 5,
        description: "URI de resource MCP desconhecida (só no MCP, como erro `-32002`).",
    },
    ErrorCodeDoc {
        code: "PROMPT_NOT_FOUND",
        status: 404,
        exit_code: 5,
        description: "Prompt MCP inexistente (só no MCP, como `-32002`).",
    },
    ErrorCodeDoc {
        code: "METHOD_NOT_ALLOWED",
        status: 405,
        exit_code: 1,
        description: "Método não suportado pela rota.",
    },
    ErrorCodeDoc {
        code: "SLUG_ALREADY_EXISTS",
        status: 409,
        exit_code: 6,
        description: "Já existe um app, um agente no mesmo app ou um canal com esse slug.",
    },
    ErrorCodeDoc {
        code: "SUBSCRIPTION_EXISTS",
        status: 409,
        exit_code: 6,
        description: "O agente já assina esse alvo (mesmo canal exato ou mesmo prefixo).",
    },
    ErrorCodeDoc {
        code: "CHANNEL_NOT_ARCHIVED",
        status: 409,
        exit_code: 6,
        description: "Só um canal arquivado pode ser excluído; arquive antes.",
    },
    ErrorCodeDoc {
        code: "CHANNEL_ARCHIVED",
        status: 409,
        exit_code: 6,
        description: "O canal está arquivado e não recebe escrita; restaure-o antes.",
    },
    ErrorCodeDoc {
        code: "MEMORY_VERSION_CONFLICT",
        status: 409,
        exit_code: 6,
        description: "A memória mudou desde `expected_version`; `details.current_version` traz a atual.",
    },
    ErrorCodeDoc {
        code: "MEMORY_DUPLICATE",
        status: 409,
        exit_code: 6,
        description: "Já existe uma memória com o mesmo texto no canal; `details.existing_id` aponta para ela.",
    },
    ErrorCodeDoc {
        code: "CATEGORY_IN_USE",
        status: 409,
        exit_code: 6,
        description: "A mudança de `categories.list` retira categorias usadas; `details.in_use` traz a contagem.",
    },
    ErrorCodeDoc {
        code: "IDEMPOTENCY_KEY_CONFLICT",
        status: 409,
        exit_code: 6,
        description: "A mesma `Idempotency-Key` já foi usada com outra operação ou outro corpo.",
    },
    ErrorCodeDoc {
        code: "LAST_ADMIN_TOKEN",
        status: 409,
        exit_code: 6,
        description: "A mudança deixaria a rede sem nenhum token `admin` ativo com `write`.",
    },
    ErrorCodeDoc {
        code: "SETTING_UNKNOWN",
        status: 400,
        exit_code: 2,
        description: "Chave de configuração desconhecida; `details.valid` lista as chaves.",
    },
    ErrorCodeDoc {
        code: "SETTING_INVALID",
        status: 400,
        exit_code: 2,
        description: "Valor fora do tipo ou do intervalo da configuração; `details` traz a regra.",
    },
    ErrorCodeDoc {
        code: "PAYLOAD_TOO_LARGE",
        status: 413,
        exit_code: 1,
        description: "Corpo acima de MAX_BODY_BYTES.",
    },
    ErrorCodeDoc {
        code: "MCP_BAD_REQUEST",
        status: 400,
        exit_code: 1,
        description: "Cabeçalhos ou mensagem MCP inválidos.",
    },
    ErrorCodeDoc {
        code: "MCP_HOST_FORBIDDEN",
        status: 403,
        exit_code: 1,
        description: "Cabeçalho `Host` fora de MCP_ALLOWED_HOSTS.",
    },
    ErrorCodeDoc {
        code: "MCP_NOT_ACCEPTABLE",
        status: 406,
        exit_code: 1,
        description: "`Accept` precisa incluir application/json e text/event-stream.",
    },
    ErrorCodeDoc {
        code: "MCP_UNSUPPORTED_MEDIA_TYPE",
        status: 415,
        exit_code: 1,
        description: "`Content-Type` do MCP precisa ser application/json.",
    },
    ErrorCodeDoc {
        code: "MCP_INVALID_MESSAGE",
        status: 422,
        exit_code: 1,
        description: "Mensagem JSON-RPC inesperada.",
    },
    ErrorCodeDoc {
        code: "RATE_LIMITED",
        status: 429,
        exit_code: 7,
        description: "Token acima de `limits.rate_per_minute`; espere `Retry-After` segundos.",
    },
    ErrorCodeDoc {
        code: "INTERNAL_ERROR",
        status: 500,
        exit_code: 1,
        description: "Falha inesperada.",
    },
    ErrorCodeDoc {
        code: "DATABASE_UNAVAILABLE",
        status: 503,
        exit_code: 8,
        description: "Escritor ou leitores do banco indisponíveis.",
    },
    ErrorCodeDoc {
        code: "MIGRATIONS_PENDING",
        status: 503,
        exit_code: 8,
        description: "Há migração pendente; o serviço ainda não recebe tráfego.",
    },
    ErrorCodeDoc {
        code: "REQUEST_TIMEOUT",
        status: 504,
        exit_code: 8,
        description: "Tempo limite da requisição (REQUEST_TIMEOUT_SECS) excedido.",
    },
];

/// Código de saída da CLI para um código de erro do servidor.
pub fn exit_code_for(code: &str) -> u8 {
    ERROR_CODES
        .iter()
        .find(|doc| doc.code == code)
        .map_or(1, |doc| doc.exit_code)
}

impl ApiError {
    /// Cria um erro a partir de um código do catálogo; o status vem do catálogo.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        let status = ERROR_CODES
            .iter()
            .find(|doc| doc.code == code)
            .and_then(|doc| StatusCode::from_u16(doc.status).ok())
            .unwrap_or_else(|| panic!("codigo de erro fora do catalogo: {code}"));
        Self {
            status,
            code,
            message: message.into(),
            details: None,
            retry_after: None,
        }
    }

    /// O envelope `{ error: { code, message, details? } }` como JSON (MCP e idempotência).
    pub fn envelope(&self) -> Value {
        serde_json::to_value(ErrorResponse {
            error: ErrorBody {
                code: self.code.to_owned(),
                message: self.message.clone(),
                details: self.details.clone(),
            },
        })
        .expect("envelope serializa")
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new("VALIDATION_ERROR", message)
    }

    pub fn route_not_found() -> Self {
        Self::new("ROUTE_NOT_FOUND", "Rota nao encontrada")
    }

    pub fn method_not_allowed() -> Self {
        Self::new("METHOD_NOT_ALLOWED", "Metodo nao permitido para esta rota")
    }

    pub fn payload_too_large() -> Self {
        Self::new("PAYLOAD_TOO_LARGE", "Body excede o limite permitido")
    }

    pub fn internal() -> Self {
        Self::new("INTERNAL_ERROR", "Erro interno")
    }

    pub fn database_unavailable() -> Self {
        Self::new("DATABASE_UNAVAILABLE", "Banco de dados indisponivel")
    }

    pub fn migrations_pending() -> Self {
        Self::new("MIGRATIONS_PENDING", "Ha migracoes pendentes no banco")
    }

    pub fn timeout() -> Self {
        Self::new("REQUEST_TIMEOUT", "Tempo limite da requisicao excedido")
    }

    pub fn unauthorized() -> Self {
        Self::new("AUTH_INVALID", "Token ausente ou invalido")
    }

    pub fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(code, message)
    }

    pub fn rate_limited(retry_after_secs: u64) -> Self {
        let mut error = Self::new(
            "RATE_LIMITED",
            "Limite de requisicoes por minuto do token excedido",
        )
        .with_details(serde_json::json!({ "retry_after_secs": retry_after_secs }));
        error.retry_after = Some(retry_after_secs);
        error
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status;
        let body = ErrorResponse {
            error: ErrorBody {
                code: self.code.to_owned(),
                message: self.message,
                details: self.details,
            },
        };
        let mut response = (status, Json(body)).into_response();
        if let Some(seconds) = self.retry_after {
            response
                .headers_mut()
                .insert(RETRY_AFTER, HeaderValue::from(seconds));
        }
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl Error for ApiError {}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unavailable => Self::database_unavailable(),
            StoreError::Internal(detail) => {
                // A mensagem vem do SQLite e nunca carrega os valores dos parâmetros.
                tracing::error!(event = "store.failed", error = %detail, "falha interna do banco");
                Self::internal()
            }
            StoreError::Conflict(_) | StoreError::NotFound => Self::internal(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn catalog_codes_are_unique_and_upper_snake_case() {
        let mut seen = HashSet::new();
        for doc in ERROR_CODES {
            assert!(seen.insert(doc.code), "duplicado: {}", doc.code);
            assert!(
                doc.code.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
                "{}",
                doc.code
            );
            assert!(StatusCode::from_u16(doc.status).is_ok());
            assert!(!doc.description.is_empty());
            assert!((1..=8).contains(&doc.exit_code), "{}", doc.code);
        }
    }

    #[test]
    fn status_comes_from_the_catalog() {
        assert_eq!(ApiError::validation("x").status, StatusCode::BAD_REQUEST);
        assert_eq!(ApiError::timeout().status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(
            ApiError::migrations_pending().status,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            ApiError::from(StoreError::Unavailable).code,
            "DATABASE_UNAVAILABLE"
        );
        assert_eq!(
            ApiError::from(StoreError::Internal("falha".to_owned())).code,
            "INTERNAL_ERROR"
        );
    }

    #[test]
    fn serializes_the_single_envelope() {
        let body = ErrorResponse {
            error: ErrorBody {
                code: "VALIDATION_ERROR".to_owned(),
                message: "invalido".to_owned(),
                details: None,
            },
        };
        assert_eq!(
            serde_json::to_value(body).expect("json"),
            serde_json::json!({ "error": { "code": "VALIDATION_ERROR", "message": "invalido" } })
        );
    }
}
