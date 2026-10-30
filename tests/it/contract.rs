use std::{collections::BTreeSet, fs, path::PathBuf};

use engramind::{app::route_paths, openapi};

/// `CARGO_MANIFEST_DIR` em tempo de execução: nos shards do CI o nextest o remapeia para o
/// checkout (`--workspace-remap .`).
fn snapshot_path() -> PathBuf {
    std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
        .join("docs/openapi.json")
}

/// `docs/openapi.json` é igual ao gerado; `UPDATE_SNAPSHOTS=1` regenera.
#[test]
fn openapi_snapshot_matches_the_generated_document() {
    let generated = openapi::snapshot();
    let path = snapshot_path();
    if std::env::var("UPDATE_SNAPSHOTS").is_ok_and(|value| value == "1") {
        fs::write(&path, &generated).expect("write snapshot");
        return;
    }
    let stored = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        stored == generated,
        "docs/openapi.json desatualizado; rode UPDATE_SNAPSHOTS=1 cargo nextest run contract::"
    );
}

#[test]
fn every_route_is_annotated_and_every_annotation_is_routed() {
    let routed: BTreeSet<&str> = route_paths().into_iter().collect();
    let documented: BTreeSet<&str> = openapi::document()
        .paths
        .paths
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(routed, documented);
}

#[test]
fn operations_have_stable_ids_tags_summaries_and_explicit_security() {
    let document = openapi::document();
    let mut ids = BTreeSet::new();
    for (path, item) in &document.paths.paths {
        let mut item = item.clone();
        let operations = openapi::operations_mut(&mut item);
        assert!(!operations.is_empty(), "{path}");
        for operation in operations {
            let id = operation.operation_id.clone().expect("operationId");
            assert!(
                id.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{id} nao e snake_case"
            );
            assert!(ids.insert(id.clone()), "operationId duplicado: {id}");
            assert!(
                operation.tags.as_ref().is_some_and(|tags| !tags.is_empty()),
                "{id} sem tag"
            );
            assert!(operation.summary.is_some(), "{id} sem resumo");
            assert!(operation.security.is_some(), "{id} sem security declarado");
            assert!(
                operation.responses.responses.contains_key("504"),
                "{id} sem 504"
            );
        }
    }
}

#[test]
fn document_declares_version_servers_and_bearer_scheme() {
    let json = serde_json::to_value(openapi::document()).expect("json");
    assert_eq!(json["info"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(json["servers"], serde_json::json!([{ "url": "." }]));
    assert_eq!(
        json["components"]["securitySchemes"]["bearerAuth"],
        serde_json::json!({ "type": "http", "scheme": "bearer" })
    );
}

mod catalog {
    //! Testes de contrato do §15 (itens 2 a 6): catálogo de operações × OpenAPI × MCP × CLI ×
    //! guias × erros × configurações.

    use std::collections::BTreeSet;

    use engramind::{
        cli::{
            client::LOCAL_ERRORS,
            help::{self, LOCAL_COMMANDS},
        },
        core::{
            docs::catalog::{ADMIN_TAGS, OPERATIONAL_TAGS},
            operations::{OPERATIONS, RequiredKind},
            settings::{SETTINGS, SettingType},
        },
        error::ERROR_CODES,
        mcp::tools::EngramindServer,
        openapi,
    };
    use serde_json::Value;

    use super::snapshot_path;
    use crate::support::TestContext;

    /// Rotas técnicas, fora do catálogo de operações.
    const TECHNICAL: &[&str] = &[
        "health_live",
        "health_ready",
        "get_openapi_document",
        "open_swagger_ui",
        "get_swagger_ui",
        "mcp",
    ];

    fn document() -> Value {
        serde_json::to_value(openapi::document()).expect("OpenAPI JSON")
    }

    /// `(método, caminho, operação)` de cada operação do OpenAPI.
    fn openapi_operations(document: &Value) -> Vec<(String, String, Value)> {
        let mut found = Vec::new();
        for (path, item) in document["paths"].as_object().expect("paths") {
            for (method, operation) in item.as_object().expect("item") {
                found.push((method.to_ascii_uppercase(), path.clone(), operation.clone()));
            }
        }
        found
    }

    fn guide(name: &str) -> String {
        std::fs::read_to_string(
            snapshot_path()
                .parent()
                .expect("docs")
                .join("guides")
                .join(name),
        )
        .expect("guide")
    }

    #[test]
    fn every_openapi_operation_is_cataloged_and_matches_it() {
        let document = document();
        let operations = openapi_operations(&document);
        for (method, path, operation) in &operations {
            let id = operation["operationId"].as_str().expect("operationId");
            if TECHNICAL.contains(&id) {
                continue;
            }
            let spec = OPERATIONS
                .iter()
                .find(|spec| spec.name == id)
                .unwrap_or_else(|| panic!("{id} fora do catalogo"));
            assert_eq!(spec.method, method, "{id}");
            assert_eq!(spec.path, path, "{id}");
            assert_eq!(operation["tags"][0], spec.tag, "{id}");
            assert_eq!(operation["summary"], spec.summary, "{id}");
            assert_eq!(
                operation["security"],
                serde_json::json!([{ "bearerAuth": [] }]),
                "{id}"
            );
            let has_key = operation["parameters"]
                .as_array()
                .is_some_and(|parameters| {
                    parameters
                        .iter()
                        .any(|parameter| parameter["name"] == "Idempotency-Key")
                });
            assert_eq!(has_key, !spec.read_only, "{id}: Idempotency-Key");
        }
        for spec in OPERATIONS {
            assert!(
                operations
                    .iter()
                    .any(|(_, _, operation)| operation["operationId"] == spec.name),
                "{} sem rota REST",
                spec.name
            );
        }
    }

    #[test]
    fn admin_tags_hold_exactly_the_admin_operations() {
        for spec in OPERATIONS {
            assert_eq!(
                ADMIN_TAGS.contains(&spec.tag),
                spec.kind == RequiredKind::Admin,
                "{}",
                spec.name
            );
        }
    }

    #[tokio::test]
    async fn every_operation_has_one_mcp_tool_with_the_rest_parameters() {
        let context = TestContext::new().await;
        let server = EngramindServer::new(context.core.clone());
        let tools = server.all_tools();
        let names: BTreeSet<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
        let expected: BTreeSet<&str> = OPERATIONS.iter().map(|spec| spec.name).collect();
        assert_eq!(names, expected);

        let document = document();
        let schemas = &document["components"]["schemas"];
        for spec in OPERATIONS {
            let operation = &document["paths"][spec.path][spec.method.to_ascii_lowercase()];
            let mut rest: BTreeSet<String> = operation["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|parameter| parameter["in"] != "header")
                .map(|parameter| parameter["name"].as_str().expect("name").to_owned())
                .collect();
            if let Some(reference) =
                operation["requestBody"]["content"]["application/json"]["schema"]["$ref"].as_str()
            {
                let name = reference.rsplit('/').next().expect("ref");
                if let Some(properties) = schemas[name]["properties"].as_object() {
                    rest.extend(properties.keys().cloned());
                } else {
                    rest.insert("settings".to_owned());
                }
            }
            if !spec.read_only {
                rest.insert("idempotency_key".to_owned());
            }
            let tool = tools
                .iter()
                .find(|tool| tool.name == spec.name)
                .expect("tool");
            let mcp: BTreeSet<String> = tool
                .input_schema
                .get("properties")
                .and_then(Value::as_object)
                .map(|properties| properties.keys().cloned().collect())
                .unwrap_or_default();
            assert_eq!(mcp, rest, "{}", spec.name);
            let annotations = tool.annotations.as_ref().expect("annotations");
            assert_eq!(
                annotations.read_only_hint,
                Some(spec.read_only),
                "{}",
                spec.name
            );
            assert_eq!(
                annotations.destructive_hint,
                Some(spec.destructive),
                "{}",
                spec.name
            );
            assert_eq!(
                annotations.idempotent_hint,
                Some(spec.idempotent),
                "{}",
                spec.name
            );
        }
    }

    #[tokio::test]
    async fn every_operation_belongs_to_exactly_one_guide() {
        let context = TestContext::new().await;
        let docs = context.core.docs();
        for operation in docs.openapi.operations(true) {
            let covering: Vec<&str> = docs
                .guides
                .iter()
                .filter(|guide| docs.guide_covers(guide, operation))
                .map(|guide| guide.id)
                .collect();
            let operational = operation
                .tags
                .iter()
                .all(|tag| OPERATIONAL_TAGS.contains(&tag.as_str()));
            if operational {
                assert!(
                    covering.len() <= 1,
                    "{}: {covering:?}",
                    operation.operation_id
                );
            } else {
                assert_eq!(
                    covering.len(),
                    1,
                    "{}: {covering:?}",
                    operation.operation_id
                );
            }
        }
    }

    #[test]
    fn every_cli_command_is_an_operation_or_a_local_command() {
        let help = help::document();
        let paths: BTreeSet<&str> = help
            .commands
            .iter()
            .map(|command| command.path.as_str())
            .collect();
        for command in &help.commands {
            assert!(
                command.operation.is_some() != command.local,
                "{}: operacao ou local",
                command.path
            );
        }
        for spec in OPERATIONS {
            assert!(paths.contains(spec.cli.join(" ").as_str()), "{}", spec.name);
        }
        for local in LOCAL_COMMANDS {
            assert!(paths.contains(local.join(" ").as_str()), "{local:?}");
        }
        <engramind::cli::Cli as clap::CommandFactory>::command().debug_assert();
    }

    /// Comandos `engramind …` citados em blocos de código e trechos `inline` de um Markdown.
    fn cited_commands(text: &str) -> Vec<String> {
        let mut cited = Vec::new();
        let mut in_fence = false;
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            if in_fence {
                let line = line.trim().trim_start_matches("$ ");
                if let Some(rest) = line.strip_prefix("engramind ") {
                    cited.push(rest.to_owned());
                }
                continue;
            }
            for (index, span) in line.split('`').enumerate() {
                if index % 2 == 1
                    && let Some(rest) = span.strip_prefix("engramind ")
                {
                    cited.push(rest.to_owned());
                }
            }
        }
        cited
    }

    #[test]
    fn every_command_cited_in_the_guides_exists() {
        let help = help::document();
        let paths: Vec<Vec<&str>> = help
            .commands
            .iter()
            .map(|command| command.path.split(' ').collect())
            .collect();
        let directory = snapshot_path().parent().expect("docs").join("guides");
        let mut checked = 0;
        for entry in std::fs::read_dir(directory).expect("guides") {
            let path = entry.expect("entry").path();
            let text = std::fs::read_to_string(&path).expect("guide");
            for cited in cited_commands(&text) {
                let words: Vec<&str> = cited
                    .split_whitespace()
                    .take_while(|word| !word.starts_with('-') && !word.starts_with('<'))
                    .collect();
                let exists = paths.iter().any(|command| {
                    words.len() >= command.len() && words[..command.len()] == command[..]
                });
                assert!(exists, "{}: engramind {cited}", path.display());
                checked += 1;
            }
        }
        assert!(checked > 10, "poucos comandos citados: {checked}");
    }

    /// A skill da CLI só cita comandos existentes em `engramind help --json` (§14, FR-016).
    #[test]
    fn every_command_cited_in_the_skill_exists() {
        let help = help::document();
        let paths: Vec<Vec<&str>> = help
            .commands
            .iter()
            .map(|command| command.path.split(' ').collect())
            .collect();
        let skill = snapshot_path()
            .parent()
            .and_then(|docs| docs.parent())
            .expect("raiz")
            .join("skills/engramind/SKILL.md");
        let text = std::fs::read_to_string(&skill).expect("skill");
        let mut checked = 0;
        for cited in cited_commands(&text) {
            let words: Vec<&str> = cited
                .split_whitespace()
                .take_while(|word| !word.starts_with('-') && !word.starts_with('<'))
                .collect();
            let exists = paths.iter().any(|command| {
                words.len() >= command.len() && words[..command.len()] == command[..]
            });
            assert!(exists, "SKILL.md: engramind {cited}");
            checked += 1;
        }
        assert!(checked >= 8, "poucos comandos citados na skill: {checked}");
    }

    #[test]
    fn every_error_code_is_documented_with_status_and_exit_code() {
        let errors = guide("erros.md");
        for doc in ERROR_CODES {
            let row = format!("| `{}` | {} | {} |", doc.code, doc.status, doc.exit_code);
            assert!(errors.contains(&row), "erros.md sem {row}");
        }
        let cli = guide("cli.md");
        for (code, exit, _) in LOCAL_ERRORS {
            let row = format!("| `{code}` | {exit} |");
            assert!(cli.contains(&row), "cli.md sem {row}");
        }
    }

    #[test]
    fn every_setting_is_documented_with_type_and_default() {
        let settings = guide("configuracoes.md");
        for spec in SETTINGS {
            let default = match spec.kind {
                SettingType::Categories => "as 5 do Magic Context".to_owned(),
                _ => spec.default.to_owned(),
            };
            let row = format!(
                "| `{}` | {} | {default} | {} | {} |",
                spec.key,
                spec.kind.name(),
                spec.kind.range(),
                if spec.channel_override { "sim" } else { "não" }
            );
            assert!(settings.contains(&row), "configuracoes.md sem {row}");
        }
    }
}
