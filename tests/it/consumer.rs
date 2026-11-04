//! O app consumidor sem token admin (0.11.0): token de app, agentes e tokens do próprio app,
//! canal criado ao assinar e o ciclo de vida dele, categorias, configurações do consumidor,
//! webhook do próprio app, importação e a receita "agente com canal próprio".

use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::support::{ADMIN_TOKEN, TestContext};

/// Token de app com os escopos dados (emitido pelo admin).
async fn app_token(context: &TestContext, app: &str, scopes: &[&str]) -> String {
    context.issue(app, None, "app", scopes).await
}

async fn call(
    context: &TestContext,
    token: &str,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    context.call(method, uri, Some(token), body).await
}

fn code(body: &Value) -> Option<&str> {
    body["error"]["code"].as_str()
}

#[tokio::test]
async fn the_admin_issues_app_tokens_that_manage_only_their_own_app() {
    let context = TestContext::new().await;
    context.create_app("dndicas").await;
    context.create_app("outro").await;
    context.create_agent("outro", "estranho").await;
    let app = app_token(&context, "dndicas", &["read", "write", "delete"]).await;

    // O app cria o agente; repetir igual devolve o existente; com outros campos, 409.
    let body = json!({ "app": "dndicas", "slug": "maddox", "name": "Maddox" });
    let (status, created) = call(&context, &app, "POST", "/v1/agents", Some(body.clone())).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["created"], true);
    let (status, again) = call(&context, &app, "POST", "/v1/agents", Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(
        (again["id"].clone(), again["created"].clone()),
        (created["id"].clone(), json!(false))
    );
    let (status, conflict) = call(
        &context,
        &app,
        "POST",
        "/v1/agents",
        Some(json!({ "app": "dndicas", "slug": "maddox", "name": "Outro nome" })),
    )
    .await;
    assert_eq!(
        (status, code(&conflict)),
        (StatusCode::CONFLICT, Some("SLUG_ALREADY_EXISTS"))
    );
    assert_eq!(
        conflict["error"]["details"]["existing"]["id"],
        created["id"]
    );

    // O app emite o token do agente, com escopos nunca acima dos dele, e o token funciona.
    let (status, issued) = call(
        &context,
        &app,
        "POST",
        "/v1/tokens",
        Some(json!({ "agent": "dndicas/maddox", "kind": "agent", "scopes": ["read", "write"] })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{issued}");
    let agent = issued["secret"].as_str().expect("secret").to_owned();
    let (status, who) = call(&context, &agent, "GET", "/v1/whoami", None).await;
    assert_eq!(
        (status, who["kind"].clone()),
        (StatusCode::OK, json!("agent"))
    );

    // Escaladas: outro app, token admin ou app, settings, o próprio app e memória individual.
    let cases = [
        (
            "POST",
            "/v1/agents",
            Some(json!({ "app": "outro", "slug": "x", "name": "X" })),
            StatusCode::FORBIDDEN,
            "FOREIGN_APP",
        ),
        (
            "GET",
            "/v1/agents/outro%2Festranho",
            None,
            StatusCode::FORBIDDEN,
            "FOREIGN_APP",
        ),
        (
            "PATCH",
            "/v1/agents/outro%2Festranho",
            Some(json!({ "disabled": true })),
            StatusCode::FORBIDDEN,
            "FOREIGN_APP",
        ),
        (
            "POST",
            "/v1/tokens",
            Some(json!({ "agent": "outro/estranho", "kind": "agent", "scopes": ["read"] })),
            StatusCode::FORBIDDEN,
            "FOREIGN_APP",
        ),
        (
            "POST",
            "/v1/tokens",
            Some(json!({ "app": "dndicas", "kind": "admin", "scopes": ["read"] })),
            StatusCode::FORBIDDEN,
            "SCOPE_REQUIRED",
        ),
        (
            "POST",
            "/v1/tokens",
            Some(json!({ "app": "dndicas", "kind": "app", "scopes": ["read"] })),
            StatusCode::FORBIDDEN,
            "SCOPE_REQUIRED",
        ),
        (
            "GET",
            "/v1/tokens?app=outro",
            None,
            StatusCode::FORBIDDEN,
            "FOREIGN_APP",
        ),
        (
            "PUT",
            "/v1/settings",
            Some(json!({ "limits.max_batch": 10 })),
            StatusCode::FORBIDDEN,
            "SCOPE_REQUIRED",
        ),
        (
            "PATCH",
            "/v1/apps/dndicas",
            Some(json!({ "name": "Novo" })),
            StatusCode::FORBIDDEN,
            "SCOPE_REQUIRED",
        ),
        (
            "POST",
            "/v1/apps",
            Some(json!({ "slug": "novo", "name": "Novo" })),
            StatusCode::FORBIDDEN,
            "SCOPE_REQUIRED",
        ),
        (
            "DELETE",
            "/v1/apps/dndicas",
            None,
            StatusCode::FORBIDDEN,
            "SCOPE_REQUIRED",
        ),
        (
            "POST",
            "/v1/channels",
            Some(json!({ "slug": "dndicas/x", "name": "X" })),
            StatusCode::FORBIDDEN,
            "SCOPE_REQUIRED",
        ),
        (
            "POST",
            "/v1/memories",
            Some(json!({ "channel": "dndicas/x", "text": "texto qualquer" })),
            StatusCode::FORBIDDEN,
            "SCOPE_REQUIRED",
        ),
        (
            "GET",
            "/metrics",
            None,
            StatusCode::FORBIDDEN,
            "SCOPE_REQUIRED",
        ),
    ];
    for (method, uri, body, status, expected) in cases {
        let (got, response) = call(&context, &app, method, uri, body).await;
        assert_eq!(
            (got, code(&response)),
            (status, Some(expected)),
            "{method} {uri}: {response}"
        );
    }
    // Escopo acima do token de app.
    let read_app = app_token(&context, "dndicas", &["read", "write"]).await;
    let (status, response) = call(
        &context,
        &read_app,
        "POST",
        "/v1/tokens",
        Some(json!({ "agent": "dndicas/maddox", "kind": "agent", "scopes": ["read", "delete"] })),
    )
    .await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::FORBIDDEN, Some("SCOPE_REQUIRED"))
    );

    // Listas só do próprio app; o token de app não mexe no próprio token.
    let (_, agents) = call(&context, &app, "GET", "/v1/agents", None).await;
    let slugs: Vec<&str> = agents["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .map(|agent| agent["qualified_slug"].as_str().expect("slug"))
        .collect();
    assert_eq!(slugs, ["dndicas/maddox"]);
    let (_, tokens) = call(&context, &app, "GET", "/v1/tokens", None).await;
    let tokens = tokens["tokens"].as_array().expect("tokens");
    assert!(tokens.iter().all(|token| token["app"]["slug"] == "dndicas"));
    let own = tokens
        .iter()
        .find(|token| token["kind"] == "app")
        .expect("own")["id"]
        .as_str()
        .expect("id")
        .to_owned();
    let (status, response) = call(
        &context,
        &app,
        "POST",
        &format!("/v1/tokens/{own}/rotate"),
        None,
    )
    .await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::FORBIDDEN, Some("SCOPE_REQUIRED"))
    );

    // Desativar o agente e revogar o token de agente; o token de app revogado dá 401.
    let agent_id = issued["token"]["id"].as_str().expect("id");
    let (status, _) = call(
        &context,
        &app,
        "POST",
        &format!("/v1/tokens/{agent_id}/revoke"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(&context, &agent, "GET", "/v1/whoami", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, disabled) = call(
        &context,
        &app,
        "PATCH",
        "/v1/agents/dndicas%2Fmaddox",
        Some(json!({ "disabled": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{disabled}");
    assert!(disabled["disabled_at"].is_string());
    let (status, _) = context
        .call(
            "POST",
            &format!("/v1/tokens/{own}/revoke"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(&context, &app, "GET", "/v1/whoami", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // O token admin continua igual.
    let (status, _) = context
        .call("GET", "/v1/apps", Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn app_tokens_create_channels_by_subscribing_and_manage_their_lifecycle() {
    let context = TestContext::new().await;
    context.create_app("dndicas").await;
    context.create_app("outro").await;
    context.create_agent("outro", "leitor").await;
    let app = app_token(&context, "dndicas", &["read", "write", "delete"]).await;
    let other = app_token(&context, "outro", &["read", "write", "delete"]).await;
    let (status, _) = call(
        &context,
        &app,
        "POST",
        "/v1/agents",
        Some(json!({ "app": "dndicas", "slug": "maddox", "name": "Maddox" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Assinar um canal exato inexistente cria o canal e registra o app.
    let body = json!({ "agent": "dndicas/maddox", "target": "dndicas/campanha/x/privado", "mode": "inject" });
    let (status, created) = call(
        &context,
        &app,
        "POST",
        "/v1/subscriptions",
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(
        (
            created["created"].clone(),
            created["channel_created"].clone()
        ),
        (json!(true), json!(true))
    );
    let (status, again) = call(&context, &app, "POST", "/v1/subscriptions", Some(body)).await;
    assert_eq!(
        (status, again["created"].clone()),
        (StatusCode::OK, json!(false))
    );
    assert_eq!(
        context.count("SELECT COUNT(*) FROM channels WHERE slug = 'dndicas/campanha/x/privado'"),
        1
    );
    let (_, channel) = call(
        &context,
        &app,
        "GET",
        "/v1/channels/dndicas%2Fcampanha%2Fx%2Fprivado",
        None,
    )
    .await;
    assert_eq!(channel["created_by"], "dndicas");
    assert_eq!(channel["reach"]["mode"], "inject");
    let (_, audit) = context
        .call(
            "GET",
            "/v1/audit?action=channel.create",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    let event = audit["events"]
        .as_array()
        .expect("events")
        .iter()
        .find(|event| event["after"]["slug"] == "dndicas/campanha/x/privado")
        .expect("audit")
        .clone();
    assert!(event["token_id"].is_string());
    // Prefixo não cria canal; canal existente continua funcionando.
    let channels_before = context.count("SELECT COUNT(*) FROM channels");
    let (status, _) = call(
        &context,
        &app,
        "POST",
        "/v1/subscriptions",
        Some(json!({ "agent": "dndicas/maddox", "target": "dndicas/publico/*", "mode": "search" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        context.count("SELECT COUNT(*) FROM channels"),
        channels_before
    );
    // O token de agente não assina.
    let agent = context
        .issue(
            "dndicas",
            Some("dndicas/maddox"),
            "agent",
            &["read", "write"],
        )
        .await;
    let (status, response) = call(
        &context,
        &agent,
        "POST",
        "/v1/subscriptions",
        Some(json!({ "agent": "dndicas/maddox", "target": "dndicas/y", "mode": "inject" })),
    )
    .await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::FORBIDDEN, Some("SCOPE_REQUIRED"))
    );
    // Nem assina o agente de outro app.
    let (status, response) = call(
        &context,
        &app,
        "POST",
        "/v1/subscriptions",
        Some(json!({ "agent": "outro/leitor", "target": "dndicas/y", "mode": "inject" })),
    )
    .await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::FORBIDDEN, Some("FOREIGN_APP"))
    );

    // O app altera nome, descrição e prioridade do canal que criou; outro app e o agente não.
    let path = "/v1/channels/dndicas%2Fcampanha%2Fx%2Fprivado";
    let version = channel["version"].as_i64().expect("version");
    let patch =
        json!({ "name": "Campanha X", "description": "Privado da campanha", "priority": 40 });
    let (status, updated) = call(&context, &app, "PATCH", path, Some(patch.clone())).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(
        (
            updated["name"].clone(),
            updated["description"].clone(),
            updated["priority"].clone(),
            updated["slug"].clone()
        ),
        (
            json!("Campanha X"),
            json!("Privado da campanha"),
            json!(40),
            json!("dndicas/campanha/x/privado")
        )
    );
    assert!(updated["version"].as_i64().expect("version") > version);
    let (status, response) = call(&context, &other, "PATCH", path, Some(patch.clone())).await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::FORBIDDEN, Some("FOREIGN_APP"))
    );
    let (status, response) = call(&context, &agent, "PATCH", path, Some(patch.clone())).await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::FORBIDDEN, Some("SCOPE_REQUIRED"))
    );
    let reader = app_token(&context, "dndicas", &["read"]).await;
    let (status, response) = call(&context, &reader, "PATCH", path, Some(patch)).await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::FORBIDDEN, Some("SCOPE_REQUIRED"))
    );

    // O app arquiva e restaura o canal que criou; outro app não.
    let (status, archived) = call(&context, &app, "POST", &format!("{path}/archive"), None).await;
    assert_eq!(status, StatusCode::OK, "{archived}");
    assert!(archived["archived_at"].is_string());
    let (status, response) = call(&context, &other, "POST", &format!("{path}/restore"), None).await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::FORBIDDEN, Some("FOREIGN_APP"))
    );
    let (status, restored) = call(&context, &app, "POST", &format!("{path}/restore"), None).await;
    assert_eq!(
        (status, restored["archived_at"].clone()),
        (StatusCode::OK, Value::Null)
    );

    // Compartilhado com um agente ativo de outro app: 409; o admin continua podendo.
    context
        .subscribe("outro/leitor", "dndicas/campanha/x/privado", "search", None)
        .await;
    let (status, response) = call(&context, &app, "POST", &format!("{path}/archive"), None).await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::CONFLICT, Some("CHANNEL_SHARED"))
    );
    assert_eq!(response["error"]["details"]["apps"], json!(["outro"]));
    let (status, response) = call(
        &context,
        &app,
        "PATCH",
        path,
        Some(json!({ "name": "Compartilhado" })),
    )
    .await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::CONFLICT, Some("CHANNEL_SHARED"))
    );
    assert_eq!(response["error"]["details"]["apps"], json!(["outro"]));
    let (status, updated) = context
        .call(
            "PATCH",
            path,
            Some(ADMIN_TOKEN),
            Some(json!({ "name": "Compartilhado" })),
        )
        .await;
    assert_eq!(
        (status, updated["name"].clone()),
        (StatusCode::OK, json!("Compartilhado"))
    );
    let (status, _) = context
        .call("POST", &format!("{path}/archive"), Some(ADMIN_TOKEN), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    // Canal criado pelo admin: o app não arquiva.
    context.create_channel("dndicas/admin").await;
    let (status, response) = call(
        &context,
        &app,
        "POST",
        "/v1/channels/dndicas%2Fadmin/archive",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::FORBIDDEN, Some("FOREIGN_APP"))
    );
    let (status, response) = call(
        &context,
        &app,
        "PATCH",
        "/v1/channels/dndicas%2Fadmin",
        Some(json!({ "name": "Meu" })),
    )
    .await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::FORBIDDEN, Some("FOREIGN_APP"))
    );
    // O app vê todos os canais (para assinar), com alcance só onde os agentes dele chegam.
    let (_, list) = call(&context, &app, "GET", "/v1/channels", None).await;
    let admin_channel = list["channels"]
        .as_array()
        .expect("channels")
        .iter()
        .find(|channel| channel["slug"] == "dndicas/admin")
        .expect("listed")
        .clone();
    assert_eq!(admin_channel.get("reach"), None);
}

#[tokio::test]
async fn agents_create_their_own_categories_without_overwriting() {
    let context = TestContext::new().await;
    context.create_app("app-a").await;
    context.create_agent("app-a", "dev").await;
    context.create_channel("eng/global").await;
    context
        .subscribe("app-a/dev", "eng/global", "inject", None)
        .await;
    let agent = context
        .issue("app-a", Some("app-a/dev"), "agent", &["read", "write"])
        .await;
    let reader = context
        .issue("app-a", Some("app-a/dev"), "agent", &["read"])
        .await;
    let before = call(&context, &agent, "GET", "/v1/context", None).await.1["version"].clone();

    let body = json!({ "key": "QUESTS", "label": "Missões", "priority": 25 });
    let (status, created) = call(
        &context,
        &agent,
        "POST",
        "/v1/categories",
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(
        created,
        json!({ "key": "QUESTS", "label": "Missões", "priority": 25, "created": true })
    );
    let (status, again) = call(&context, &agent, "POST", "/v1/categories", Some(body)).await;
    assert_eq!(
        (status, again["created"].clone()),
        (StatusCode::OK, json!(false))
    );
    let (status, conflict) = call(
        &context,
        &agent,
        "POST",
        "/v1/categories",
        Some(json!({ "key": "QUESTS", "label": "Outra", "priority": 25 })),
    )
    .await;
    assert_eq!(
        (status, code(&conflict)),
        (StatusCode::CONFLICT, Some("CATEGORY_CONFLICT"))
    );
    let (status, conflict) = call(
        &context,
        &agent,
        "POST",
        "/v1/categories",
        Some(json!({ "key": "NAMING", "label": "Nomes", "priority": 1 })),
    )
    .await;
    assert_eq!(
        (status, code(&conflict)),
        (StatusCode::CONFLICT, Some("CATEGORY_CONFLICT")),
        "nunca sobrescreve"
    );
    let (status, denied) = call(
        &context,
        &reader,
        "POST",
        "/v1/categories",
        Some(json!({ "key": "X", "label": "X", "priority": 1 })),
    )
    .await;
    assert_eq!(
        (status, code(&denied)),
        (StatusCode::FORBIDDEN, Some("SCOPE_REQUIRED"))
    );
    let (status, invalid) = call(
        &context,
        &agent,
        "POST",
        "/v1/categories",
        Some(json!({ "key": "quests", "label": "X", "priority": 1 })),
    )
    .await;
    assert_eq!(
        (status, code(&invalid)),
        (StatusCode::BAD_REQUEST, Some("VALIDATION_ERROR"))
    );

    // O agente grava nela; a lista mostra para qualquer `read`; a `version` mudou.
    let (status, written) = context
        .call("POST", "/v1/memories", Some(&agent), Some(json!({ "channel": "eng/global", "text": "A missão do dragão começa na vila.", "category": "QUESTS" })))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{written}");
    let (_, list) = call(&context, &reader, "GET", "/v1/categories", None).await;
    assert_eq!(
        list["categories"]
            .as_array()
            .expect("list")
            .last()
            .expect("last")["key"],
        "QUESTS"
    );
    let after = call(&context, &agent, "GET", "/v1/context", None).await.1["version"].clone();
    assert_ne!(before, after);
    let (_, audit) = context
        .call(
            "GET",
            "/v1/audit?action=category.create",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(audit["events"].as_array().expect("events").len(), 1);
    // O admin vê e edita; retirar a categoria em uso dá 409.
    let (_, settings) = context
        .call(
            "GET",
            "/v1/settings?key=categories.list",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert!(settings.to_string().contains("QUESTS"));
    let (status, in_use) = context
        .call("PUT", "/v1/settings", Some(ADMIN_TOKEN), Some(json!({ "categories.list": [{ "key": "PROJECT_RULES", "label": "Regras", "priority": 50 }] })))
        .await;
    assert_eq!(
        (status, code(&in_use)),
        (StatusCode::CONFLICT, Some("CATEGORY_IN_USE"))
    );
}

#[tokio::test]
async fn consumer_settings_show_limits_and_the_effective_values_of_writable_channels() {
    let context = TestContext::new().await;
    context.create_app("app-a").await;
    context.create_agent("app-a", "dev").await;
    for slug in ["eng/global", "eng/leitura", "eng/fora"] {
        context.create_channel(slug).await;
    }
    context
        .subscribe("app-a/dev", "eng/global", "inject", None)
        .await;
    context
        .subscribe("app-a/dev", "eng/leitura", "inject", Some(&["read"]))
        .await;
    let (status, _) = context
        .call("PUT", "/v1/channels/eng%2Fglobal/settings", Some(ADMIN_TOKEN), Some(json!({ "memory.text_max": 500, "context.text_max_chars": 220, "context.category_priorities": { "CONSTRAINTS": 60 } })))
        .await;
    assert_eq!(status, StatusCode::OK);
    let agent = context
        .issue("app-a", Some("app-a/dev"), "agent", &["read", "write"])
        .await;
    let app = app_token(&context, "app-a", &["read"]).await;
    for token in [&agent, &app] {
        let (status, body) = call(&context, token, "GET", "/v1/consumer-settings", None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["memory"], json!({ "text_min": 3, "text_max": 4000 }));
        assert_eq!(body["limits"]["max_batch"], 100);
        assert_eq!(body["context"]["default_format"], "tagged");
        assert_eq!(body["consolidation"], json!({ "stale_after_days": 90 }));
        assert_eq!(
            body["search"],
            json!({ "vector_weight": 1.0, "text_weight": 1.0, "rrf_k": 60 })
        );
        let channels = body["channels"].as_array().expect("channels");
        let slugs: Vec<&str> = channels
            .iter()
            .map(|channel| channel["channel"].as_str().expect("slug"))
            .collect();
        // Token de app só com `read`: nenhum canal com escrita.
        if token == &app {
            assert!(slugs.is_empty(), "{body}");
            continue;
        }
        assert_eq!(slugs, ["eng/global"], "só onde escreve");
        assert_eq!(channels[0]["memory_text_max"], 500);
        assert_eq!(channels[0]["text_max_chars"], 220);
        assert_eq!(channels[0]["budget_tokens"], 4000);
        assert_eq!(channels[0]["category_priorities"]["CONSTRAINTS"], 60);
        assert_eq!(channels[0]["category_priorities"]["PROJECT_RULES"], 50);
        let raw = body.to_string();
        assert!(!raw.contains("redator") && !raw.contains("embedding") && !raw.contains("api_key"));
    }
    let writer = app_token(&context, "app-a", &["read", "write"]).await;
    let (_, body) = call(&context, &writer, "GET", "/v1/consumer-settings", None).await;
    assert_eq!(body["channels"].as_array().expect("channels").len(), 1);
    // `get_settings` continua admin.
    let (status, _) = call(&context, &agent, "GET", "/v1/settings", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&context, &app, "GET", "/v1/settings", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Desuso e pesos da busca: os valores globais efetivos, para qualquer token com `read`.
    let (status, body) = context
        .call("PUT", "/v1/settings", Some(ADMIN_TOKEN), Some(json!({ "consolidation.stale_after_days": 30, "search.vector_weight": 2, "search.text_weight": 0.5, "search.rrf_k": 10 })))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for token in [&agent, &app, &ADMIN_TOKEN.to_owned()] {
        let (status, body) = call(&context, token, "GET", "/v1/consumer-settings", None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["consolidation"]["stale_after_days"], 30);
        assert_eq!(
            body["search"],
            json!({ "vector_weight": 2.0, "text_weight": 0.5, "rrf_k": 10 })
        );
    }
    // MCP e CLI repassam o mesmo objeto.
    let result = context
        .call_tool(&app, "get_consumer_settings", json!({}))
        .await;
    let structured = &result["result"]["structuredContent"];
    assert_eq!(
        structured["consolidation"]["stale_after_days"], 30,
        "{result}"
    );
    assert_eq!(structured["search"]["rrf_k"], 10);
}

#[tokio::test]
async fn app_tokens_read_only_their_own_app_with_the_webhook_state() {
    let context = TestContext::new().await;
    let own = context.create_app("dndicas").await;
    context.create_app("outro").await;
    context.create_agent("dndicas", "maddox").await;
    let app = app_token(&context, "dndicas", &["read", "write"]).await;
    let reader = app_token(&context, "dndicas", &["read"]).await;
    let agent = context
        .issue(
            "dndicas",
            Some("dndicas/maddox"),
            "agent",
            &["read", "write"],
        )
        .await;
    let secret = "w".repeat(40);

    // Sem webhook: `webhook: null`.
    let (status, body) = call(&context, &reader, "GET", "/v1/apps/dndicas", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["slug"], "dndicas");
    assert_eq!(body["webhook"], Value::Null);

    let (status, _) = call(
        &context,
        &app,
        "PUT",
        "/v1/apps/dndicas/webhook",
        Some(json!({ "url": "https://dndicas.local/eventos", "secret": secret })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Pelo slug e pelo id: a URL e o estado do segredo, nunca o valor.
    let id = own["id"].as_str().expect("id");
    for target in ["dndicas", id] {
        let (status, body) = call(
            &context,
            &reader,
            "GET",
            &format!("/v1/apps/{target}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["webhook"]["url"], "https://dndicas.local/eventos");
        assert_eq!(body["webhook"]["secret"]["configured"], true);
        assert!(body["webhook"]["secret"]["updated_at"].is_string());
        assert!(!body.to_string().contains(&secret), "{body}");
    }

    // Outro app: 403 FOREIGN_APP; inexistente: 404; agente: 403 SCOPE_REQUIRED.
    let (status, response) = call(&context, &app, "GET", "/v1/apps/outro", None).await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::FORBIDDEN, Some("FOREIGN_APP"))
    );
    let (status, response) = call(&context, &app, "GET", "/v1/apps/nao-existe", None).await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::NOT_FOUND, Some("APP_NOT_FOUND"))
    );
    let (status, response) = call(&context, &agent, "GET", "/v1/apps/dndicas", None).await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::FORBIDDEN, Some("SCOPE_REQUIRED"))
    );
    // O admin lê qualquer app; `list_apps` continua só do admin.
    let (status, body) = call(&context, ADMIN_TOKEN, "GET", "/v1/apps/outro", None).await;
    assert_eq!(
        (status, body["slug"].clone()),
        (StatusCode::OK, json!("outro"))
    );
    let (status, response) = call(&context, &app, "GET", "/v1/apps", None).await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::FORBIDDEN, Some("SCOPE_REQUIRED"))
    );

    // MCP: as mesmas regras.
    let result = context
        .call_tool(&reader, "get_app", json!({ "app": "dndicas" }))
        .await;
    assert_eq!(result["result"]["isError"], false, "{result}");
    let structured = &result["result"]["structuredContent"];
    assert_eq!(structured["webhook"]["secret"]["configured"], true);
    assert!(!result.to_string().contains(&secret));
    let result = context
        .call_tool(&reader, "get_app", json!({ "app": "outro" }))
        .await;
    assert_eq!(result["result"]["isError"], true, "{result}");
    assert_eq!(
        result["result"]["structuredContent"]["error"]["code"],
        "FOREIGN_APP"
    );
}

#[tokio::test]
async fn app_tokens_set_their_own_webhook_and_import_into_their_channels() {
    let context = TestContext::new().await;
    context.create_app("dndicas").await;
    context.create_app("outro").await;
    context.create_agent("dndicas", "maddox").await;
    context.create_channel("dndicas/campanha").await;
    context.create_channel("outro/canal").await;
    context
        .subscribe("dndicas/maddox", "dndicas/campanha", "inject", None)
        .await;
    let app = app_token(&context, "dndicas", &["read", "write"]).await;
    let secret = "s".repeat(40);

    // Webhook do próprio app: o segredo nunca volta; outro app → 403; url null remove.
    let (status, response) = call(
        &context,
        &app,
        "PUT",
        "/v1/apps/dndicas/webhook",
        Some(json!({ "url": "https://dndicas.local/eventos" })),
    )
    .await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::BAD_REQUEST, Some("VALIDATION_ERROR")),
        "url exige segredo"
    );
    let (status, hooked) = call(
        &context,
        &app,
        "PUT",
        "/v1/apps/dndicas/webhook",
        Some(json!({ "url": "https://dndicas.local/eventos", "secret": secret })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{hooked}");
    assert_eq!(hooked["webhook"]["url"], "https://dndicas.local/eventos");
    assert_eq!(hooked["webhook"]["secret"]["configured"], true);
    assert!(!hooked.to_string().contains(&secret));
    let (_, audit) = context
        .call(
            "GET",
            "/v1/audit?action=app.webhook_updated",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert!(!audit.to_string().contains(&secret));
    assert_eq!(audit["events"].as_array().expect("events").len(), 1);
    // `identities.updated` vai para o próprio app.
    assert_eq!(context.count("SELECT COUNT(*) FROM jobs WHERE kind = 'webhook' AND payload_json LIKE '%identities.updated%'"), 1);
    let (status, response) = call(
        &context,
        &app,
        "PUT",
        "/v1/apps/outro/webhook",
        Some(json!({ "url": null })),
    )
    .await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::FORBIDDEN, Some("FOREIGN_APP"))
    );
    let (status, removed) = call(
        &context,
        &app,
        "PUT",
        "/v1/apps/dndicas/webhook",
        Some(json!({ "url": null })),
    )
    .await;
    assert_eq!(
        (status, removed["webhook"].clone()),
        (StatusCode::OK, Value::Null)
    );
    let (status, _) = context
        .call(
            "PUT",
            "/v1/apps/outro/webhook",
            Some(ADMIN_TOKEN),
            Some(json!({ "url": "https://outro.local/e", "secret": secret })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "o admin continua podendo");

    // Importação nos canais dos agentes do app; repetir não duplica; outro canal → 403.
    let import = json!({
        "channel": "dndicas/campanha",
        "memories": [
            { "text": "A fraqueza do dragão é o gelo.", "title": "A Fraqueza do Dragão", "tags": ["missao"], "key": "lore:dragao" },
            { "text": "A vila de Pedra Alta fica no norte.", "title": "Vila de Pedra Alta" }
        ]
    });
    let (status, _, imported) = context
        .call_with(
            "POST",
            "/v1/memories/import",
            Some(&app),
            Some(import.clone()),
            &[("Idempotency-Key", "lote-1")],
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{imported}");
    assert_eq!(imported["created"].as_array().expect("created").len(), 2);
    let (_, _, replay) = context
        .call_with(
            "POST",
            "/v1/memories/import",
            Some(&app),
            Some(import),
            &[("Idempotency-Key", "lote-1")],
        )
        .await;
    assert_eq!(replay, imported);
    assert_eq!(context.count("SELECT COUNT(*) FROM memories"), 2);
    let id = imported["created"][0]["id"].as_str().expect("id");
    let (_, memory) = context
        .call(
            "GET",
            &format!("/v1/memories/{id}"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(
        (memory["title"].clone(), memory["title_locked"].clone()),
        (json!("A Fraqueza do Dragão"), json!(true))
    );
    assert_eq!(
        (memory["tags"].clone(), memory["key"].clone()),
        (json!(["missao"]), json!("lore:dragao"))
    );
    assert_eq!(
        (
            memory["author"]["app"].clone(),
            memory["author"]["agent"].clone()
        ),
        (json!("dndicas"), Value::Null)
    );
    // Um item com a chave existente vira edição (upsert).
    let (status, _, updated) = context
        .call_with("POST", "/v1/memories/import", Some(&app), Some(json!({ "channel": "dndicas/campanha", "memories": [{ "text": "A fraqueza do dragão é o fogo azul.", "key": "lore:dragao" }] })), &[("Idempotency-Key", "lote-2")])
        .await;
    assert_eq!(status, StatusCode::CREATED, "{updated}");
    assert_eq!(updated["updated"], json!([{ "index": 0, "id": id }]));
    assert_eq!(context.count("SELECT COUNT(*) FROM memories"), 2);
    let (status, _, denied) = context
        .call_with(
            "POST",
            "/v1/memories/import",
            Some(&app),
            Some(json!({ "channel": "outro/canal", "memories": [{ "text": "texto alheio" }] })),
            &[("Idempotency-Key", "lote-3")],
        )
        .await;
    assert_eq!(
        (status, code(&denied)),
        (StatusCode::FORBIDDEN, Some("CHANNEL_NOT_SUBSCRIBED"))
    );
    let (status, _, denied) = context
        .call_with("POST", "/v1/memories/import", Some(&app), Some(json!({ "channel": "dndicas/campanha", "memories": [{ "text": "texto com autor", "author": { "app": "outro" } }] })), &[("Idempotency-Key", "lote-4")])
        .await;
    assert_eq!(
        (status, code(&denied)),
        (StatusCode::FORBIDDEN, Some("FOREIGN_APP"))
    );
    let agent = context
        .issue(
            "dndicas",
            Some("dndicas/maddox"),
            "agent",
            &["read", "write"],
        )
        .await;
    let (status, _, denied) = context
        .call_with("POST", "/v1/memories/import", Some(&agent), Some(json!({ "channel": "dndicas/campanha", "memories": [{ "text": "texto do agente" }] })), &[("Idempotency-Key", "lote-5")])
        .await;
    assert_eq!(
        (status, code(&denied)),
        (StatusCode::FORBIDDEN, Some("SCOPE_REQUIRED"))
    );
    // O app lê as memórias dos canais dos agentes dele.
    let (_, list) = call(&context, &app, "GET", "/v1/memories?tag=missao", None).await;
    assert_eq!(list["memories"].as_array().expect("memories").len(), 1);
}

/// A receita "agente com canal próprio" (guia `tokens-e-escopos`), com o token de app.
async fn recipe(context: &TestContext, app: &str) -> Option<String> {
    let steps = [
        (
            "POST",
            "/v1/agents",
            json!({ "app": "opex", "slug": "arquiteto", "name": "Arquiteto" }),
        ),
        (
            "POST",
            "/v1/subscriptions",
            json!({ "agent": "opex/arquiteto", "target": "opex", "mode": "inject" }),
        ),
        (
            "POST",
            "/v1/subscriptions",
            json!({ "agent": "opex/arquiteto", "target": "agentes/arquiteto", "mode": "inject" }),
        ),
    ];
    for (method, uri, body) in steps {
        let (status, response) = call(context, app, method, uri, Some(body)).await;
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "{uri}: {response}"
        );
    }
    let (_, tokens) = call(
        context,
        app,
        "GET",
        "/v1/tokens?agent=opex%2Farquiteto",
        None,
    )
    .await;
    let active = tokens["tokens"]
        .as_array()
        .expect("tokens")
        .iter()
        .any(|token| token["status"] == "active");
    if active {
        return None;
    }
    let (status, issued) = call(context, app, "POST", "/v1/tokens", Some(json!({ "agent": "opex/arquiteto", "kind": "agent", "scopes": ["read", "write", "delete"] }))).await;
    assert_eq!(status, StatusCode::CREATED, "{issued}");
    issued["secret"].as_str().map(str::to_owned)
}

#[tokio::test]
async fn the_agent_with_its_own_channel_recipe_is_idempotent_and_rotates_without_orphans() {
    let context = TestContext::new().await;
    context.create_app("opex").await;
    context.create_channel("opex").await;
    let app = app_token(&context, "opex", &["read", "write", "delete"]).await;
    let first = recipe(&context, &app).await.expect("token");
    assert!(
        recipe(&context, &app).await.is_none(),
        "a segunda execução não emite outro token"
    );
    let count = |sql: &str| context.count(sql);
    assert_eq!(
        count("SELECT COUNT(*) FROM agents WHERE slug = 'arquiteto'"),
        1
    );
    assert_eq!(
        count("SELECT COUNT(*) FROM channels WHERE slug = 'agentes/arquiteto'"),
        1
    );
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM subscriptions s JOIN agents g ON g.id = s.agent_id WHERE g.slug = 'arquiteto'"
        ),
        2
    );
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM tokens t JOIN agents g ON g.id = t.agent_id WHERE g.slug = 'arquiteto' AND t.revoked_at IS NULL"
        ),
        1
    );
    let (_, agent) = call(&context, &app, "GET", "/v1/agents/opex%2Farquiteto", None).await;
    assert_eq!(agent["qualified_slug"], "opex/arquiteto");
    let (_, context_block) = call(&context, &first, "GET", "/v1/context", None).await;
    assert!(context_block["version"].is_string());

    // Segredo perdido: `rotate_token` troca sem deixar token órfão.
    let (_, tokens) = call(
        &context,
        &app,
        "GET",
        "/v1/tokens?agent=opex%2Farquiteto",
        None,
    )
    .await;
    let id = tokens["tokens"][0]["id"].as_str().expect("id").to_owned();
    let (status, rotated) = call(
        &context,
        &app,
        "POST",
        &format!("/v1/tokens/{id}/rotate"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{rotated}");
    assert_eq!(
        rotated["token"]["scopes"],
        json!(["read", "write", "delete"])
    );
    assert_ne!(rotated["token"]["id"], json!(id));
    let fresh = rotated["secret"].as_str().expect("secret");
    let (status, _) = call(&context, &first, "GET", "/v1/whoami", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(&context, fresh, "GET", "/v1/whoami", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM tokens t JOIN agents g ON g.id = t.agent_id WHERE g.slug = 'arquiteto' AND t.revoked_at IS NULL"
        ),
        1
    );
    assert_eq!(
        count(
            "SELECT COUNT(*) FROM tokens t JOIN agents g ON g.id = t.agent_id WHERE g.slug = 'arquiteto' AND t.revoked_at IS NOT NULL"
        ),
        1
    );
    let (status, response) = call(
        &context,
        &app,
        "POST",
        &format!("/v1/tokens/{id}/rotate"),
        None,
    )
    .await;
    assert_eq!(
        (status, code(&response)),
        (StatusCode::BAD_REQUEST, Some("VALIDATION_ERROR")),
        "revogado não gira"
    );
}
