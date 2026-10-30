//! Documentação viva (desenho §15): guias embutidos e o índice do OpenAPI da versão em
//! execução, servidos pelas operações do grupo Documentação nas três vias. Guias `admin` e
//! operações, tags e esquemas administrativos ficam ocultos para tokens `agent`.

pub mod catalog;
pub mod openapi_index;
pub mod search;

use std::fmt::Write as _;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::{IntoParams, ToSchema};

use self::{
    catalog::{Guide, load_guides},
    openapi_index::{OpenApiIndex, OperationEntry},
    search::MAX_SEARCH_TERMS,
};
use super::{Core, principal::Principal};
use crate::{error::ApiError, store::Store};

const DEFAULT_SEARCH_LIMIT: usize = 10;
const MAX_SEARCH_LIMIT: usize = 25;
const MAX_QUERY_CHARS: usize = 200;

/// Índice imutável compartilhado por todas as requisições.
#[derive(Debug)]
pub struct Docs {
    pub guides: Vec<Guide>,
    pub openapi: OpenApiIndex,
}

impl Docs {
    pub fn new(openapi: &utoipa::openapi::OpenApi) -> Self {
        let document = serde_json::to_value(openapi).expect("OpenAPI serializes to JSON");
        Self {
            guides: load_guides(),
            openapi: OpenApiIndex::new(&document),
        }
    }

    pub fn guide(&self, id: &str, admin: bool) -> Option<&Guide> {
        self.guides
            .iter()
            .find(|guide| guide.id == id && guide.visible_to(admin))
    }

    pub fn visible_guides(&self, admin: bool) -> impl Iterator<Item = &Guide> {
        self.guides
            .iter()
            .filter(move |guide| guide.visible_to(admin))
    }

    /// Se `guide` documenta `operation`: guias e operações se ligam pela tag OpenAPI.
    pub fn guide_covers(&self, guide: &Guide, operation: &OperationEntry) -> bool {
        operation
            .tags
            .iter()
            .any(|tag| guide.tags.contains(&tag.as_str()))
    }

    fn valid_guides(&self, admin: bool) -> Vec<String> {
        self.visible_guides(admin)
            .map(|guide| guide.id.to_owned())
            .collect()
    }

    fn find_guide(&self, id: &str, admin: bool) -> Result<&Guide, ApiError> {
        self.guide(id.trim(), admin).ok_or_else(|| {
            not_found(
                "GUIDE_NOT_FOUND",
                "guia desconhecido",
                id,
                self.valid_guides(admin),
            )
        })
    }

    fn guide_operations(&self, guide: &Guide, admin: bool) -> Vec<String> {
        self.openapi
            .operations(admin)
            .filter(|operation| self.guide_covers(guide, operation))
            .map(|operation| operation.operation_id.clone())
            .collect()
    }
}

/// `label` é a frase completa, por exemplo `"secao desconhecida"`.
fn not_found(code: &'static str, label: &str, value: &str, valid: Vec<String>) -> ApiError {
    ApiError::not_found(code, format!("{label}: {value}")).with_details(json!({ "valid": valid }))
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DocText {
    #[schema(example = "overview")]
    pub id: String,
    pub title: String,
    /// Seção pedida, quando houver.
    pub section: Option<String>,
    /// Markdown.
    pub content: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct GetGuideQuery {
    /// Título de uma seção (acentos e caixa ignorados); omita para o guia inteiro.
    pub section: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct SearchDocsQuery {
    /// Palavras-chave (2 a 200 caracteres, até 10 termos). Acentos e caixa são ignorados;
    /// termos comuns em inglês também casam com os guias em português.
    #[param(example = "idempotencia")]
    pub q: String,
    /// Máximo de resultados, de 1 a 25 (padrão 10).
    #[param(minimum = 1, maximum = 25)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct ListOperationsQuery {
    /// Só as operações documentadas por este guia.
    pub guide: Option<String>,
    /// Só as operações com esta tag OpenAPI exata, por exemplo `Identities`.
    pub tag: Option<String>,
}

impl<S: Store> Core<S> {
    pub fn get_overview(&self, principal: &Principal) -> Result<DocText, ApiError> {
        self.authorize(principal, "get_overview")?;
        let docs = self.docs();
        let admin = principal.is_admin();
        let overview = docs.find_guide("overview", admin)?;
        let mut content = format!(
            "{} {}\n\n{}",
            docs.openapi.title, docs.openapi.version, overview.content
        );
        content.push_str("\n## Guias disponiveis para este token\n\n");
        for guide in docs.visible_guides(admin) {
            let _ = writeln!(content, "- `{}`: {}", guide.id, guide.title);
        }
        Ok(DocText {
            id: overview.id.to_owned(),
            title: overview.title.clone(),
            section: None,
            content,
        })
    }

    pub fn list_guides(&self, principal: &Principal) -> Result<Value, ApiError> {
        self.authorize(principal, "list_guides")?;
        let docs = self.docs();
        let admin = principal.is_admin();
        let guides: Vec<Value> = docs
            .visible_guides(admin)
            .map(|guide| {
                json!({
                    "id": guide.id,
                    "title": guide.title,
                    "audience": match guide.audience {
                        catalog::Audience::All => "all",
                        catalog::Audience::Admin => "admin",
                    },
                    "tags": guide.tags,
                    "sections": guide
                        .sections
                        .iter()
                        .filter(|section| section.level > 1)
                        .map(|section| section.heading.as_str())
                        .collect::<Vec<_>>(),
                    "operations": docs.guide_operations(guide, admin),
                })
            })
            .collect();
        Ok(json!({
            "service": { "title": docs.openapi.title, "version": docs.openapi.version },
            "guides": guides,
            "tags": docs
                .openapi
                .visible_tags(admin)
                .map(|(name, description)| json!({ "name": name, "description": description }))
                .collect::<Vec<_>>(),
        }))
    }

    pub fn get_guide(
        &self,
        principal: &Principal,
        guide: &str,
        query: GetGuideQuery,
    ) -> Result<DocText, ApiError> {
        self.authorize(principal, "get_guide")?;
        let guide = self.docs().find_guide(guide, principal.is_admin())?;
        let Some(heading) = query
            .section
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(DocText {
                id: guide.id.to_owned(),
                title: guide.title.clone(),
                section: None,
                content: guide.content.to_owned(),
            });
        };
        let section = guide.section(heading).ok_or_else(|| {
            not_found(
                "SECTION_NOT_FOUND",
                "secao desconhecida",
                heading,
                guide
                    .sections
                    .iter()
                    .map(|section| section.heading.clone())
                    .collect(),
            )
        })?;
        Ok(DocText {
            id: guide.id.to_owned(),
            title: guide.title.clone(),
            section: Some(section.heading.clone()),
            content: guide.section_text(section).to_owned(),
        })
    }

    pub fn search_docs(
        &self,
        principal: &Principal,
        query: SearchDocsQuery,
    ) -> Result<Value, ApiError> {
        self.authorize(principal, "search_docs")?;
        let docs = self.docs();
        let admin = principal.is_admin();
        let text = query.q.trim();
        let length = text.chars().count();
        if !(2..=MAX_QUERY_CHARS).contains(&length) {
            return Err(ApiError::validation("q deve ter entre 2 e 200 caracteres"));
        }
        let limit = match query.limit {
            None => DEFAULT_SEARCH_LIMIT,
            Some(limit) if (1..=MAX_SEARCH_LIMIT).contains(&(limit as usize)) => limit as usize,
            Some(_) => return Err(ApiError::validation("limit deve estar entre 1 e 25")),
        };
        let topics = search::topics(text);
        if topics.is_empty() {
            return Err(ApiError::validation(
                "q deve conter ao menos um termo com 2 caracteres",
            ));
        }
        debug_assert!(topics.len() <= MAX_SEARCH_TERMS);

        let mut hits: Vec<(search::Score, search::TopicMask, Value)> = Vec::new();
        for guide in docs.visible_guides(admin) {
            for section in &guide.sections {
                let body = guide.section_body(section);
                if let Some(hit) = search::score(&section.heading, body, &topics) {
                    hits.push((
                        hit.score,
                        hit.topics,
                        json!({
                            "kind": "guide",
                            "guide": guide.id,
                            "section": section.heading,
                            "snippet": hit.snippet,
                        }),
                    ));
                }
            }
        }
        for operation in docs.openapi.operations(admin) {
            let heading = format!(
                "{} {}",
                operation.operation_id,
                operation.summary.as_deref().unwrap_or_default()
            );
            let body = format!(
                "{} {} {} {} {} {}",
                operation.operation_id,
                operation.method,
                operation.path,
                operation.tags.join(" "),
                operation.summary.as_deref().unwrap_or_default(),
                operation.description.as_deref().unwrap_or_default()
            );
            if let Some(hit) = search::score(&heading, &body, &topics) {
                hits.push((
                    hit.score,
                    hit.topics,
                    json!({
                        "kind": "operation",
                        "operation": operation.operation_id,
                        "method": operation.method.to_ascii_uppercase(),
                        "path": operation.path,
                        "snippet": hit.snippet,
                    }),
                ));
            }
        }
        // A ordenação estável mantém seções de guia antes de operações no empate.
        hits.sort_by_key(|hit| std::cmp::Reverse(hit.0));
        let total = hits.len();
        let ranking: Vec<_> = hits
            .iter()
            .map(|(score, mask, _)| (*score, *mask))
            .collect();
        let results: Vec<Value> = search::select(&ranking, topics.len(), limit)
            .into_iter()
            .map(|index| {
                let (score, _, hit) = &hits[index];
                let mut hit = hit.clone();
                hit["score"] = json!(score.value());
                hit["matched_terms"] = json!(score.matched);
                hit
            })
            .collect();
        Ok(json!({ "total": total, "terms": topics.len(), "results": results }))
    }

    pub fn list_operations(
        &self,
        principal: &Principal,
        query: ListOperationsQuery,
    ) -> Result<Value, ApiError> {
        self.authorize(principal, "list_operations")?;
        let docs = self.docs();
        let admin = principal.is_admin();
        let guide = match query
            .guide
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            Some(guide) => Some(docs.find_guide(guide, admin)?),
            None => None,
        };
        let tag = query
            .tag
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(tag) = tag
            && !docs
                .openapi
                .visible_tags(admin)
                .any(|(name, _)| name == tag)
        {
            return Err(not_found(
                "TAG_NOT_FOUND",
                "tag desconhecida",
                tag,
                docs.openapi
                    .visible_tags(admin)
                    .map(|(name, _)| name.clone())
                    .collect(),
            ));
        }
        let operations: Vec<Value> = docs
            .openapi
            .operations(admin)
            .filter(|operation| {
                guide.is_none_or(|guide| docs.guide_covers(guide, operation))
                    && tag.is_none_or(|tag| operation.tags.iter().any(|value| value == tag))
            })
            .map(OperationEntry::summary_json)
            .collect();
        Ok(json!({ "operations": operations }))
    }

    pub fn get_operation(&self, principal: &Principal, name: &str) -> Result<Value, ApiError> {
        self.authorize(principal, "get_operation")?;
        let docs = self.docs();
        let admin = principal.is_admin();
        let name = name.trim();
        let operation = docs.openapi.operation(name, admin).ok_or_else(|| {
            not_found(
                "OPERATION_NOT_FOUND",
                "operacao desconhecida",
                name,
                docs.openapi
                    .operations(admin)
                    .map(|operation| operation.operation_id.clone())
                    .collect(),
            )
        })?;
        Ok(docs.openapi.operation_document(operation))
    }

    pub fn get_schema(&self, principal: &Principal, name: &str) -> Result<Value, ApiError> {
        self.authorize(principal, "get_schema")?;
        let docs = self.docs();
        let admin = principal.is_admin();
        let name = name.trim();
        if !docs.openapi.schema_visible(name, admin) {
            return Err(not_found(
                "SCHEMA_NOT_FOUND",
                "esquema desconhecido",
                name,
                docs.openapi
                    .schema_names(admin)
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            ));
        }
        docs.openapi
            .schema_document(name)
            .ok_or_else(ApiError::internal)
    }
}
