//! Índice somente leitura do OpenAPI gerado, filtrado pelo tipo do token (portado do
//! sap-middleman).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

use super::catalog::ADMIN_TAGS;

const SCHEMA_REF_PREFIX: &str = "#/components/schemas/";
const METHODS: &[&str] = &[
    "get", "put", "post", "delete", "patch", "options", "head", "trace",
];

#[derive(Clone, Debug)]
pub struct OperationEntry {
    pub operation_id: String,
    pub method: String,
    pub path: String,
    pub tags: Vec<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub admin_only: bool,
    pub security: Vec<String>,
    operation: Value,
}

impl OperationEntry {
    pub fn visible_to(&self, admin: bool) -> bool {
        admin || !self.admin_only
    }

    pub fn summary_json(&self) -> Value {
        json!({
            "operation": self.operation_id,
            "method": self.method.to_ascii_uppercase(),
            "path": self.path,
            "tags": self.tags,
            "summary": self.summary,
            "security": self.security,
        })
    }
}

#[derive(Clone, Debug)]
pub struct OpenApiIndex {
    pub title: String,
    pub version: String,
    pub tags: Vec<(String, Option<String>)>,
    operations: Vec<OperationEntry>,
    schemas: BTreeMap<String, Value>,
    security_schemes: Map<String, Value>,
    /// Schemas reachable from at least one operation visible to service clients.
    service_schemas: BTreeSet<String>,
}

impl OpenApiIndex {
    pub fn new(document: &Value) -> Self {
        let schemas: BTreeMap<String, Value> = document["components"]["schemas"]
            .as_object()
            .map(|schemas| {
                schemas
                    .iter()
                    .map(|(name, schema)| (name.clone(), schema.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let security_schemes = document["components"]["securitySchemes"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        let tags = document["tags"]
            .as_array()
            .map(|tags| {
                tags.iter()
                    .filter_map(|tag| {
                        Some((
                            tag["name"].as_str()?.to_owned(),
                            tag["description"].as_str().map(str::to_owned),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut operations = Vec::new();
        if let Some(paths) = document["paths"].as_object() {
            for (path, item) in paths {
                for method in METHODS {
                    let Some(operation) = item.get(*method).filter(|value| value.is_object())
                    else {
                        continue;
                    };
                    let Some(operation_id) = operation["operationId"].as_str() else {
                        continue;
                    };
                    let tags: Vec<String> = operation["tags"]
                        .as_array()
                        .map(|tags| {
                            tags.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default();
                    let admin_only = tags.iter().any(|tag| ADMIN_TAGS.contains(&tag.as_str()));
                    let security = operation["security"]
                        .as_array()
                        .map(|requirements| {
                            requirements
                                .iter()
                                .filter_map(Value::as_object)
                                .flat_map(|requirement| requirement.keys().cloned())
                                .collect::<BTreeSet<_>>()
                                .into_iter()
                                .collect()
                        })
                        .unwrap_or_default();
                    operations.push(OperationEntry {
                        operation_id: operation_id.to_owned(),
                        method: (*method).to_owned(),
                        path: path.clone(),
                        tags,
                        summary: operation["summary"].as_str().map(str::to_owned),
                        description: operation["description"].as_str().map(str::to_owned),
                        admin_only,
                        security,
                        operation: operation.clone(),
                    });
                }
            }
        }
        operations
            .sort_by(|left, right| (&left.path, &left.method).cmp(&(&right.path, &right.method)));

        let mut index = Self {
            title: document["info"]["title"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            version: document["info"]["version"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            tags,
            operations,
            schemas,
            security_schemes,
            service_schemas: BTreeSet::new(),
        };
        let mut service_schemas = BTreeSet::new();
        for operation in index
            .operations
            .iter()
            .filter(|operation| !operation.admin_only)
        {
            service_schemas.extend(index.referenced_schemas(&operation.operation));
        }
        index.service_schemas = service_schemas;
        index
    }

    pub fn operations(&self, admin: bool) -> impl Iterator<Item = &OperationEntry> {
        self.operations
            .iter()
            .filter(move |operation| operation.visible_to(admin))
    }

    pub fn operation(&self, operation_id: &str, admin: bool) -> Option<&OperationEntry> {
        self.operations(admin)
            .find(|operation| operation.operation_id == operation_id)
    }

    pub fn visible_tags(&self, admin: bool) -> impl Iterator<Item = &(String, Option<String>)> {
        self.tags
            .iter()
            .filter(move |(name, _)| admin || !ADMIN_TAGS.contains(&name.as_str()))
    }

    pub fn schema_visible(&self, name: &str, admin: bool) -> bool {
        self.schemas.contains_key(name) && (admin || self.service_schemas.contains(name))
    }

    pub fn schema_names(&self, admin: bool) -> Vec<&str> {
        self.schemas
            .keys()
            .map(String::as_str)
            .filter(|name| self.schema_visible(name, admin))
            .collect()
    }

    /// Returns an operation with every transitively referenced schema and the
    /// security schemes it declares.
    pub fn operation_document(&self, operation: &OperationEntry) -> Value {
        let schemas = self.schema_map(self.referenced_schemas(&operation.operation));
        let security_schemes: Map<String, Value> = operation
            .security
            .iter()
            .filter_map(|name| {
                self.security_schemes
                    .get(name)
                    .map(|scheme| (name.clone(), scheme.clone()))
            })
            .collect();
        json!({
            "operation": operation.operation_id,
            "method": operation.method.to_ascii_uppercase(),
            "path": operation.path,
            "definition": operation.operation,
            "components": {
                "schemas": schemas,
                "securitySchemes": security_schemes,
            },
        })
    }

    /// Returns a schema with its transitive references.
    pub fn schema_document(&self, name: &str) -> Option<Value> {
        let schema = self.schemas.get(name)?;
        let mut referenced = self.referenced_schemas(schema);
        referenced.remove(name);
        Some(json!({
            "name": name,
            "schema": schema,
            "components": { "schemas": self.schema_map(referenced) },
        }))
    }

    fn schema_map(&self, names: BTreeSet<String>) -> Map<String, Value> {
        names
            .into_iter()
            .filter_map(|name| {
                let schema = self.schemas.get(&name)?.clone();
                Some((name, schema))
            })
            .collect()
    }

    /// Resolves schema references transitively without looping on cycles.
    fn referenced_schemas(&self, value: &Value) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        let mut pending = Vec::new();
        collect_refs(value, &mut pending);
        while let Some(name) = pending.pop() {
            if found.insert(name.clone())
                && let Some(schema) = self.schemas.get(&name)
            {
                collect_refs(schema, &mut pending);
            }
        }
        found
    }
}

fn collect_refs(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            if let Some(name) = object
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|reference| reference.strip_prefix(SCHEMA_REF_PREFIX))
            {
                output.push(name.to_owned());
            }
            for nested in object.values() {
                collect_refs(nested, output);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_refs(item, output)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> OpenApiIndex {
        OpenApiIndex::new(&json!({
            "info": {"title": "T", "version": "1"},
            "tags": [{"name": "Public"}, {"name": "Identities"}],
            "paths": {
                "/public": {"post": {
                    "operationId": "publicOp", "tags": ["Public"],
                    "security": [{"bearerAuth": []}],
                    "requestBody": {"content": {"application/json": {"schema": {"$ref": "#/components/schemas/A"}}}}
                }},
                "/admin": {"get": {
                    "operationId": "adminOp", "tags": ["Identities"],
                    "responses": {"200": {"content": {"application/json": {"schema": {"$ref": "#/components/schemas/Secret"}}}}}
                }}
            },
            "components": {
                "schemas": {
                    "A": {"properties": {"b": {"$ref": "#/components/schemas/B"}}},
                    "B": {"properties": {"a": {"$ref": "#/components/schemas/A"}}},
                    "Secret": {"type": "object"}
                },
                "securitySchemes": {"bearerAuth": {"type": "http", "scheme": "bearer"}}
            }
        }))
    }

    #[test]
    fn hides_admin_operations_tags_and_schemas_from_service_clients() {
        let index = fixture();
        assert!(index.operation("adminOp", false).is_none());
        assert!(index.operation("adminOp", true).is_some());
        assert!(index.operation("publicOp", false).is_some());
        assert!(!index.schema_visible("Secret", false));
        assert!(index.schema_visible("Secret", true));
        assert!(index.schema_visible("B", false));
        assert_eq!(
            index
                .visible_tags(false)
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["Public"]
        );
    }

    #[test]
    fn resolves_cyclic_references_and_security_schemes() {
        let index = fixture();
        let operation = index.operation("publicOp", false).expect("operation");
        let document = index.operation_document(operation);
        let schemas = document["components"]["schemas"]
            .as_object()
            .expect("schemas");
        assert_eq!(
            schemas.keys().map(String::as_str).collect::<Vec<_>>(),
            ["A", "B"]
        );
        assert_eq!(
            document["components"]["securitySchemes"]["bearerAuth"]["scheme"],
            "bearer"
        );
        let schema = index.schema_document("A").expect("schema");
        assert_eq!(
            schema["components"]["schemas"]
                .as_object()
                .expect("refs")
                .keys()
                .collect::<Vec<_>>(),
            ["B"]
        );
    }

    #[test]
    fn indexes_every_generated_operation() {
        let document = serde_json::to_value(crate::openapi::document()).expect("OpenAPI JSON");
        let index = OpenApiIndex::new(&document);
        let mut expected = 0;
        for item in document["paths"].as_object().expect("paths").values() {
            for method in METHODS {
                if item.get(*method).is_some() {
                    expected += 1;
                }
            }
        }
        assert_eq!(index.operations(true).count(), expected);
        let agent_ids: Vec<_> = index
            .operations(false)
            .map(|operation| operation.operation_id.as_str())
            .collect();
        for admin_id in [
            "create_app",
            "list_tokens",
            "issue_token",
            "get_settings",
            "get_status",
            "get_audit",
        ] {
            assert!(!agent_ids.contains(&admin_id), "{admin_id} leaked");
        }
        assert!(agent_ids.contains(&"whoami"));
        assert!(agent_ids.contains(&"get_guide"));
        assert!(agent_ids.contains(&"mcp"));
        assert!(!index.schema_visible("IssuedToken", false));
        assert!(!index.schema_visible("SettingsView", false));
        assert!(index.schema_visible("WhoAmI", false));
    }
}
