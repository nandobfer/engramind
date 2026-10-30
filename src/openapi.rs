use std::sync::LazyLock;

use axum::{Json, response::Redirect};
use utoipa::openapi::{
    KnownFormat, ObjectBuilder, RefOr, Required, SchemaFormat, Type,
    header::Header,
    path::{Operation, ParameterBuilder, ParameterIn},
    security::{Http, HttpAuthScheme, SecurityScheme},
    server::Server,
};
use utoipa::{Modify, OpenApi};

use crate::{
    error::{ErrorBody, ErrorResponse},
    health::{LiveResponse, ReadyResponse},
    store::MigrationStatus,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Engramind API",
        description = "Rede de memórias para agentes de IA: memórias duráveis em canais, entregues por REST, MCP e CLI."
    ),
    paths(
        crate::openapi::root_docs_redirect,
        crate::health::live,
        crate::health::ready,
        crate::openapi::openapi_json_contract,
        crate::openapi::swagger_docs_contract
    ),
    components(schemas(ErrorResponse, ErrorBody, LiveResponse, ReadyResponse, MigrationStatus)),
    modifiers(&SecurityAddon),
    tags(
        (name = "Health", description = "Verificações públicas de vida e prontidão"),
        (name = "Operations", description = "Rotas operacionais públicas (OpenAPI e Swagger UI)")
    )
)]
pub struct ApiDoc;

static DOCUMENT: LazyLock<utoipa::openapi::OpenApi> = LazyLock::new(ApiDoc::openapi);

/// Documento OpenAPI da versão em execução, gerado uma única vez e só no primeiro uso.
pub fn document() -> &'static utoipa::openapi::OpenApi {
    &DOCUMENT
}

/// Conteúdo de `docs/openapi.json` (snapshot versionado).
pub fn snapshot() -> String {
    let mut json = serde_json::to_string_pretty(document()).expect("OpenAPI serializa");
    json.push('\n');
    json
}

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        openapi.info.license = None;
        openapi.servers = Some(vec![Server::new(".")]);
        openapi
            .components
            .get_or_insert_default()
            .add_security_scheme(
                "bearerAuth",
                SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
            );

        let request_id_parameter = ParameterBuilder::new()
            .name("x-request-id")
            .parameter_in(ParameterIn::Header)
            .required(Required::False)
            .description(Some(
                "Optional UUID correlation identifier. Invalid values are replaced.",
            ))
            .schema(Some(
                ObjectBuilder::new()
                    .schema_type(Type::String)
                    .format(Some(SchemaFormat::KnownFormat(KnownFormat::Uuid)))
                    .build(),
            ))
            .build();
        let request_id_header = Header::builder()
            .schema(
                ObjectBuilder::new()
                    .schema_type(Type::String)
                    .format(Some(SchemaFormat::KnownFormat(KnownFormat::Uuid)))
                    .build(),
            )
            .description(Some("UUID correlation identifier used by this request."))
            .build();
        let bearer_challenge = Header::builder()
            .description(Some("Bearer authentication challenge."))
            .build();

        for path in openapi.paths.paths.values_mut() {
            for operation in operations_mut(path) {
                operation
                    .parameters
                    .get_or_insert_default()
                    .push(request_id_parameter.clone());
                for response in operation.responses.responses.values_mut() {
                    if let RefOr::T(response) = response {
                        response
                            .headers
                            .insert("x-request-id".to_owned(), request_id_header.clone());
                    }
                }
                if let Some(RefOr::T(response)) = operation.responses.responses.get_mut("401") {
                    response
                        .headers
                        .insert("WWW-Authenticate".to_owned(), bearer_challenge.clone());
                }
            }
        }
    }
}

pub fn operations_mut(path: &mut utoipa::openapi::path::PathItem) -> Vec<&mut Operation> {
    [
        &mut path.get,
        &mut path.put,
        &mut path.post,
        &mut path.delete,
        &mut path.options,
        &mut path.head,
        &mut path.patch,
        &mut path.trace,
    ]
    .into_iter()
    .filter_map(Option::as_mut)
    .collect()
}

#[utoipa::path(
    get,
    path = "/openapi.json",
    operation_id = "get_openapi_document",
    tag = "Operations",
    summary = "Devolve o documento OpenAPI da versão em execução",
    security(),
    responses(
        (status = 200, description = "Documento OpenAPI em JSON", body = serde_json::Value),
        (status = 504, description = "Tempo limite da requisição", body = ErrorResponse)
    )
)]
pub async fn openapi_json_contract() -> Json<&'static utoipa::openapi::OpenApi> {
    Json(document())
}

#[utoipa::path(
    get,
    path = "/",
    operation_id = "open_swagger_ui",
    tag = "Operations",
    summary = "Redireciona para a Swagger UI",
    description = "Usa redirect relativo para que a Swagger UI funcione atrás de proxy com prefixo.",
    security(),
    responses(
        (status = 303, description = "Redirect relativo para a Swagger UI", headers(("location" = String, description = "`docs/`, relativo, preservando qualquer prefixo público"))),
        (status = 504, description = "Tempo limite da requisição", body = ErrorResponse)
    )
)]
pub async fn root_docs_redirect() -> Redirect {
    Redirect::to("docs/")
}

#[utoipa::path(
    get,
    path = "/docs",
    operation_id = "get_swagger_ui",
    tag = "Operations",
    summary = "Abre a Swagger UI",
    description = "Redireciona para `docs/`; a Swagger UI carrega `../openapi.json`.",
    security(),
    responses(
        (status = 303, description = "Redirect relativo para o índice da Swagger UI", headers(("location" = String, description = "`docs/`, relativo, preservando qualquer prefixo público"))),
        (status = 504, description = "Tempo limite da requisição", body = ErrorResponse)
    )
)]
pub async fn swagger_docs_contract() {}
