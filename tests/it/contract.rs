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
