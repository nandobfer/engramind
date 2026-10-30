use axum::{
    Json,
    http::{HeaderValue, StatusCode, header::WWW_AUTHENTICATE},
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

/// Entrada do catálogo de códigos de erro (fonte do guia de erros e do teste de contrato).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorCodeDoc {
    pub code: &'static str,
    pub status: u16,
    pub description: &'static str,
}

pub const ERROR_CODES: &[ErrorCodeDoc] = &[
    ErrorCodeDoc {
        code: "VALIDATION_ERROR",
        status: 400,
        description: "Path, query ou body inválido.",
    },
    ErrorCodeDoc {
        code: "ROUTE_NOT_FOUND",
        status: 404,
        description: "Rota inexistente.",
    },
    ErrorCodeDoc {
        code: "METHOD_NOT_ALLOWED",
        status: 405,
        description: "Método não suportado pela rota.",
    },
    ErrorCodeDoc {
        code: "PAYLOAD_TOO_LARGE",
        status: 413,
        description: "Corpo acima de MAX_BODY_BYTES.",
    },
    ErrorCodeDoc {
        code: "INTERNAL_ERROR",
        status: 500,
        description: "Falha inesperada.",
    },
    ErrorCodeDoc {
        code: "DATABASE_UNAVAILABLE",
        status: 503,
        description: "Escritor ou leitores do banco indisponíveis.",
    },
    ErrorCodeDoc {
        code: "MIGRATIONS_PENDING",
        status: 503,
        description: "Há migração pendente; o serviço ainda não recebe tráfego.",
    },
    ErrorCodeDoc {
        code: "REQUEST_TIMEOUT",
        status: 504,
        description: "Tempo limite da requisição (REQUEST_TIMEOUT_SECS) excedido.",
    },
];

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
        }
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
            StoreError::Conflict(_) | StoreError::NotFound | StoreError::Internal(_) => {
                Self::internal()
            }
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
