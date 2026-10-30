//! Documentação viva pelo REST: guias, busca e índice do OpenAPI filtrados pelo token.

use axum::http::StatusCode;
use serde_json::Value;

use crate::support::{ADMIN_TOKEN, TestContext};

async fn agent(context: &TestContext) -> String {
    context.create_app("app-a").await;
    context.create_agent("app-a", "curador").await;
    context
        .issue("app-a", Some("app-a/curador"), "agent", &["read"])
        .await
}

fn ids(list: &Value, field: &str, key: &str) -> Vec<String> {
    list[field]
        .as_array()
        .unwrap_or_else(|| panic!("{field}: {list}"))
        .iter()
        .map(|item| item[key].as_str().expect("id").to_owned())
        .collect()
}

#[tokio::test]
async fn admin_guides_operations_and_schemas_are_hidden_from_agents() {
    let context = TestContext::new().await;
    let agent = agent(&context).await;

    let (_, admin_guides) = context
        .call("GET", "/v1/docs/guides", Some(ADMIN_TOKEN), None)
        .await;
    let (_, agent_guides) = context
        .call("GET", "/v1/docs/guides", Some(&agent), None)
        .await;
    let admin_ids = ids(&admin_guides, "guides", "id");
    let agent_ids = ids(&agent_guides, "guides", "id");
    for hidden in ["configuracoes", "operacao"] {
        assert!(admin_ids.contains(&hidden.to_owned()));
        assert!(!agent_ids.contains(&hidden.to_owned()), "{hidden}");
    }
    let (status, body) = context
        .call("GET", "/v1/docs/guides/configuracoes", Some(&agent), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "GUIDE_NOT_FOUND");
    assert!(
        !body["error"]["details"]["valid"]
            .to_string()
            .contains("configuracoes")
    );

    let (_, operations) = context
        .call("GET", "/v1/docs/operations", Some(&agent), None)
        .await;
    let names = ids(&operations, "operations", "operation");
    assert!(names.contains(&"whoami".to_owned()));
    assert!(
        !names
            .iter()
            .any(|name| name == "issue_token" || name == "get_settings")
    );
    let (status, _) = context
        .call("GET", "/v1/docs/operations/issue_token", Some(&agent), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = context
        .call(
            "GET",
            "/v1/docs/operations/issue_token",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["definition"]["operationId"], "issue_token");
    assert!(body["components"]["schemas"]["IssuedToken"].is_object());

    let (status, body) = context
        .call("GET", "/v1/docs/schemas/IssuedToken", Some(&agent), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "SCHEMA_NOT_FOUND");
    let (status, _) = context
        .call("GET", "/v1/docs/schemas/WhoAmI", Some(&agent), None)
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = context
        .call(
            "GET",
            "/v1/docs/operations?tag=Settings",
            Some(&agent),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "TAG_NOT_FOUND");
}

#[tokio::test]
async fn guides_sections_overview_and_search_are_served() {
    let context = TestContext::new().await;
    let (status, overview) = context
        .call("GET", "/v1/docs/overview", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let content = overview["content"].as_str().expect("content");
    assert!(content.starts_with(&format!("Engramind API {}", env!("CARGO_PKG_VERSION"))));
    assert!(content.contains("`configuracoes`"));

    let (status, section) = context
        .call(
            "GET",
            "/v1/docs/guides/tokens-e-escopos?section=EMISSAO",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(section["section"], "Emissão");
    assert!(
        section["content"]
            .as_str()
            .expect("content")
            .starts_with("## Emissão")
    );

    let (status, body) = context
        .call(
            "GET",
            "/v1/docs/guides/tokens-e-escopos?section=nada",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "SECTION_NOT_FOUND");
    assert!(
        body["error"]["details"]["valid"]
            .as_array()
            .is_some_and(|valid| !valid.is_empty())
    );

    let (status, results) = context
        .call(
            "GET",
            "/v1/docs/search?q=revocation%20idempotencia&limit=5",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{results}");
    assert!(results["total"].as_u64().is_some_and(|total| total > 0));
    assert!(
        results["results"]
            .as_array()
            .is_some_and(|hits| hits.len() <= 5)
    );
    let (_, revoke) = context
        .call("GET", "/v1/docs/search?q=revoke", Some(ADMIN_TOKEN), None)
        .await;
    assert!(revoke.to_string().contains("revoke_token"), "{revoke}");

    for query in ["q=a", "q=token&limit=26", "limit=3"] {
        let (status, _) = context
            .call(
                "GET",
                &format!("/v1/docs/search?{query}"),
                Some(ADMIN_TOKEN),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
    }
}
