//! `get_context` (005): escolha por modo, `version`/ETag e `304` nas três vias, invalidação por
//! cada tipo de mudança, estabilidade com contadores, `usage_log`, `exclude_context` e os
//! resources e o prompt do MCP.

use std::process::{Command, Output};

use axum::http::{StatusCode, header};
use serde_json::{Value, json};

use crate::support::{ADMIN_TOKEN, TestContext};

/// `(at, op, agent_id, channel_ids, memory_ids, rendered_tokens)` de `usage_log`.
type UsageRow = (String, String, Option<String>, String, String, Option<i64>);

struct Network {
    context: TestContext,
    agent: String,
    subscription: String,
}

/// `app-a/leitor` assina `eng/global` (`inject`), `eng/atlas` (`inject`), `eng/busca`
/// (`search`) e `eng/gestao` (`manage`); cada canal tem uma memória.
async fn network() -> Network {
    let context = TestContext::new().await;
    context.create_app("app-a").await;
    context.create_agent("app-a", "leitor").await;
    for slug in ["eng/global", "eng/atlas", "eng/busca", "eng/gestao"] {
        context.create_channel(slug).await;
    }
    let subscription = context
        .subscribe("app-a/leitor", "eng/global", "inject", None)
        .await;
    context
        .subscribe("app-a/leitor", "eng/atlas", "inject", None)
        .await;
    context
        .subscribe("app-a/leitor", "eng/busca", "search", None)
        .await;
    context
        .subscribe("app-a/leitor", "eng/gestao", "manage", None)
        .await;
    for (channel, text) in [
        (
            "eng/global",
            "Nunca rodar a suite completa de testes localmente.",
        ),
        (
            "eng/atlas",
            "Toda chamada externa passa pelo cliente central.",
        ),
        ("eng/busca", "Memoria do canal de busca sobre deploy."),
        ("eng/gestao", "Memoria do canal de gestao sobre deploy."),
    ] {
        context.create_memory(ADMIN_TOKEN, channel, text).await;
    }
    let agent = context
        .issue("app-a", Some("app-a/leitor"), "agent", &["read", "write"])
        .await;
    Network {
        context,
        agent,
        subscription: subscription["id"].as_str().expect("id").to_owned(),
    }
}

async fn get(context: &TestContext, token: &str, uri: &str) -> (StatusCode, Value, String) {
    let (status, headers, body) = context.call_with("GET", uri, Some(token), None, &[]).await;
    let etag = headers
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    (status, body, etag)
}

async fn version(context: &TestContext, token: &str, uri: &str) -> String {
    let (status, body, _) = get(context, token, uri).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["version"].as_str().expect("version").to_owned()
}

fn content(body: &Value) -> &str {
    body["content"].as_str().expect("content")
}

#[tokio::test]
async fn only_inject_channels_enter_and_the_etag_is_the_version() {
    let network = network().await;
    let context = &network.context;
    let (status, body, etag) = get(context, &network.agent, "/v1/context").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let version = body["version"].as_str().expect("version");
    assert!(
        version.starts_with("cv_") && version.len() == 15,
        "{version}"
    );
    assert_eq!(etag, format!("\"{version}\""));
    assert_eq!(body["format"], "tagged");
    assert_eq!(
        body["counts"],
        json!({ "full": 2, "titles": 0, "overflow": 0 })
    );
    assert!(body["rendered_tokens"].as_u64().expect("tokens") > 10);
    let text = content(&body);
    assert!(text.starts_with(&format!("<memories version=\"{version}\">\n")));
    assert!(text.contains("<channel slug=\"eng/global\">\n<PROJECT_RULES>\n#m_"));
    assert!(text.contains("Toda chamada externa passa pelo cliente central."));
    assert!(
        !text.contains("deploy"),
        "search e manage ficam fora: {text}"
    );
    assert!(text.ends_with("</memories>"));

    let (_, headers, _) = context
        .call_with("GET", "/v1/context", Some(&network.agent), None, &[])
        .await;
    assert_eq!(headers[header::CACHE_CONTROL], "private, no-cache");
    assert_eq!(headers[header::VARY], "Authorization, Engramind-Channels");

    let (status, only, _) = get(context, &network.agent, "/v1/context?channels=eng/atlas").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(only["counts"]["full"], 1);
    assert!(!content(&only).contains("eng/global"));
    assert_ne!(only["version"], body["version"]);

    let (_, headers, header_body) = context
        .call_with(
            "GET",
            "/v1/context",
            Some(&network.agent),
            None,
            &[("Engramind-Channels", "eng/atlas")],
        )
        .await;
    assert_eq!(header_body["version"], only["version"]);
    assert!(headers.contains_key(header::ETAG));
}

#[tokio::test]
async fn if_none_match_answers_304_without_a_body() {
    let network = network().await;
    let context = &network.context;
    let version = version(context, &network.agent, "/v1/context").await;
    for tag in [
        format!("\"{version}\""),
        version.clone(),
        format!("W/\"{version}\""),
        format!("\"cv_000000000000\", \"{version}\""),
    ] {
        let (status, headers, body) = context
            .call_with(
                "GET",
                "/v1/context",
                Some(&network.agent),
                None,
                &[("If-None-Match", &tag)],
            )
            .await;
        assert_eq!(status, StatusCode::NOT_MODIFIED, "{tag}");
        assert_eq!(body, Value::Null);
        assert_eq!(headers[header::ETAG], format!("\"{version}\"").as_str());
    }
    for tag in ["*", "\"cv_000000000000\""] {
        let (status, _, _) = context
            .call_with(
                "GET",
                "/v1/context",
                Some(&network.agent),
                None,
                &[("If-None-Match", tag)],
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{tag}");
    }
    let (status, _, _) = context
        .call_with(
            "GET",
            "/v1/context?format=markdown",
            Some(&network.agent),
            None,
            &[("If-None-Match", &version)],
        )
        .await;
    assert_eq!(status, StatusCode::OK, "cada formato tem a própria version");
}

#[tokio::test]
async fn counters_and_searches_never_change_the_version() {
    let network = network().await;
    let context = &network.context;
    let (_, first, _) = get(context, &network.agent, "/v1/context").await;
    for _ in 0..3 {
        let (status, body) = context
            .call(
                "POST",
                "/v1/memories/search",
                Some(&network.agent),
                Some(json!({ "query": "testes cliente deploy" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    context.core.flush_counters().await.expect("flush");
    let (_, second, _) = get(context, &network.agent, "/v1/context").await;
    context.core.flush_counters().await.expect("flush");
    assert!(context.count("SELECT COUNT(*) FROM memories WHERE injected_count > 0") > 0);
    let (_, third, _) = get(context, &network.agent, "/v1/context").await;
    assert_eq!(first, second);
    assert_eq!(second, third);
}

#[tokio::test]
async fn every_real_change_changes_the_version() {
    let network = network().await;
    let context = &network.context;
    let agent = network.agent.as_str();
    let mut seen = vec![version(context, agent, "/v1/context").await];
    let mut expect_change = async |label: &str| {
        let current = version(context, agent, "/v1/context").await;
        assert!(!seen.contains(&current), "{label} manteve a version");
        seen.push(current);
    };

    let created = context
        .create_memory(ADMIN_TOKEN, "eng/global", "Nova memoria do canal global.")
        .await;
    expect_change("create_memory").await;
    let id = created["memory"]["id"].as_str().expect("id").to_owned();

    let (status, body) = context
        .call(
            "PATCH",
            &format!("/v1/memories/{id}"),
            Some(ADMIN_TOKEN),
            Some(json!({ "text": "Memoria editada do canal global." })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    expect_change("update_memory").await;

    let (status, _) = context
        .call(
            "POST",
            &format!("/v1/memories/{id}/pin"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    expect_change("pin_memory").await;

    let (status, _) = context
        .call(
            "POST",
            &format!("/v1/memories/{id}/archive"),
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    expect_change("archive_memory").await;

    let (status, body) = context
        .call(
            "PATCH",
            &format!("/v1/subscriptions/{}", network.subscription),
            Some(ADMIN_TOKEN),
            Some(json!({ "priority": 50 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    expect_change("prioridade da assinatura").await;

    let (status, body) = context
        .call(
            "PATCH",
            "/v1/channels/eng%2Fatlas",
            Some(ADMIN_TOKEN),
            Some(json!({ "priority": 7 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    expect_change("prioridade do canal").await;

    let (status, body) = context
        .call(
            "PUT",
            "/v1/channels/eng%2Fglobal/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "context.budget_tokens": 5 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    expect_change("orçamento do canal").await;

    let (status, body) = context
        .call(
            "PUT",
            "/v1/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "context.titles_budget_tokens": 100 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    expect_change("configuração global").await;

    let (status, body) = context
        .call(
            "PUT",
            "/v1/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "categories.list": [
                { "key": "PROJECT_RULES", "label": "Regras", "priority": 1 },
                { "key": "ARCHITECTURE", "label": "Arquitetura", "priority": 40 },
                { "key": "CONSTRAINTS", "label": "Restrições", "priority": 30 },
                { "key": "CONFIG_VALUES", "label": "Valores", "priority": 20 },
                { "key": "NAMING", "label": "Nomes", "priority": 10 }
            ] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    expect_change("categorias").await;

    let (status, body) = context
        .call(
            "PATCH",
            &format!("/v1/subscriptions/{}", network.subscription),
            Some(ADMIN_TOKEN),
            Some(json!({ "mode": "search" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    expect_change("modo da assinatura").await;

    let (status, body) = context
        .call(
            "PUT",
            "/v1/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "context.default_format": "markdown" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let explicit = version(context, agent, "/v1/context?format=tagged").await;
    assert_eq!(
        Some(&explicit),
        seen.last(),
        "default_format não muda a version de quem passa format"
    );
}

#[tokio::test]
async fn budgets_cut_into_titles_and_overflow() {
    let network = network().await;
    let context = &network.context;
    for n in 0..6 {
        context
            .create_memory(
                ADMIN_TOKEN,
                "eng/global",
                &format!("Memoria numero {n} com algumas palavras a mais para pesar."),
            )
            .await;
    }
    let (status, body) = context
        .call(
            "PUT",
            "/v1/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "context.budget_tokens": 0, "context.titles_budget_tokens": 30 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, body, _) = get(context, &network.agent, "/v1/context").await;
    let counts = &body["counts"];
    assert_eq!(counts["full"], 0);
    let titles = counts["titles"].as_u64().expect("titles");
    let overflow = counts["overflow"].as_u64().expect("overflow");
    assert!(titles >= 1 && overflow >= 1, "{counts}");
    assert_eq!(titles + overflow, 8);
    assert!(content(&body).contains(&format!(
        "<overflow count=\"{overflow}\">use search_memories para o restante</overflow>"
    )));

    context
        .call(
            "PUT",
            "/v1/settings",
            Some(ADMIN_TOKEN),
            Some(json!({ "context.overflow_line": false })),
        )
        .await;
    let (_, body, _) = get(context, &network.agent, "/v1/context").await;
    assert!(!content(&body).contains("<overflow"));
    assert_eq!(body["counts"]["overflow"], overflow);
}

#[tokio::test]
async fn formats_render_the_same_choice() {
    let network = network().await;
    let context = &network.context;
    let (_, tagged, _) = get(context, &network.agent, "/v1/context").await;
    let (_, markdown, _) = get(context, &network.agent, "/v1/context?format=markdown").await;
    assert_eq!(markdown["format"], "markdown");
    assert_eq!(markdown["counts"], tagged["counts"]);
    assert!(content(&markdown).starts_with("## eng/atlas\n\n### PROJECT_RULES\n- "));
    assert!(!content(&markdown).contains("#m_"));
    let (_, document, _) = get(context, &network.agent, "/v1/context?format=json").await;
    assert_eq!(document["format"], "json");
    let channels = document["content"]["channels"]
        .as_array()
        .expect("channels");
    assert_eq!(channels.len(), 2);
    assert_eq!(
        channels[1]["categories"][0]["memories"][0]["text"],
        "Nunca rodar a suite completa de testes localmente."
    );
    assert_eq!(document["content"]["overflow"], 0);
    let (status, body, _) = get(context, &network.agent, "/v1/context?format=xml").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "VALIDATION_ERROR");
}

#[tokio::test]
async fn admin_without_channels_sees_every_active_channel() {
    let network = network().await;
    let context = &network.context;
    let (status, body) = context
        .call(
            "POST",
            "/v1/channels/eng%2Fgestao/archive",
            Some(ADMIN_TOKEN),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, body, _) = get(context, ADMIN_TOKEN, "/v1/context").await;
    assert_eq!(body["counts"]["full"], 3);
    let text = content(&body);
    assert!(text.contains("eng/busca"));
    assert!(!text.contains("eng/gestao"), "arquivado fica fora");
}

#[tokio::test]
async fn usage_log_records_deliveries_and_searches_with_the_instant() {
    let network = network().await;
    let context = &network.context;
    let version = version(context, &network.agent, "/v1/context").await;
    context
        .call_with(
            "GET",
            "/v1/context",
            Some(&network.agent),
            None,
            &[("If-None-Match", &version)],
        )
        .await;
    context
        .call(
            "POST",
            "/v1/memories/search",
            Some(&network.agent),
            Some(json!({ "query": "deploy" })),
        )
        .await;
    context.core.flush_counters().await.expect("flush");
    let rows: Vec<UsageRow> = context
        .db()
        .prepare(
            "SELECT at, op, agent_id, channel_ids, memory_ids, rendered_tokens
             FROM usage_log ORDER BY id",
        )
        .expect("prepare")
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");
    assert_eq!(rows.len(), 2, "o 304 não é gravado: {rows:?}");
    assert_eq!(rows[0].1, "get_context");
    assert!(
        rows[0].0.ends_with('Z') && rows[0].0.len() == 24,
        "{}",
        rows[0].0
    );
    assert!(
        rows[0]
            .2
            .as_deref()
            .is_some_and(|agent| agent.starts_with("ag_"))
    );
    let channels: Vec<String> = serde_json::from_str(&rows[0].3).expect("channels");
    assert_eq!(channels.len(), 2);
    let memories: Vec<String> = serde_json::from_str(&rows[0].4).expect("memories");
    assert_eq!(memories.len(), 2);
    assert!(rows[0].5.is_some_and(|tokens| tokens > 0));
    assert_eq!(rows[1].1, "search_memories");
    assert_eq!(rows[1].5, None);
    let in_period = context.count(&format!(
        "SELECT COUNT(*) FROM usage_log WHERE at >= '{}' AND at <= '9999'",
        &rows[0].0[..10]
    ));
    assert_eq!(in_period, 2);
    assert_eq!(
        context.count("SELECT COUNT(*) FROM memories WHERE injected_count = 1"),
        2
    );
}

#[tokio::test]
async fn search_can_exclude_the_context_and_reports_its_version() {
    let network = network().await;
    let context = &network.context;
    let version = version(context, &network.agent, "/v1/context").await;
    let search = |exclude: bool| {
        context.call(
            "POST",
            "/v1/memories/search",
            Some(&network.agent),
            Some(json!({ "query": "testes deploy", "exclude_context": exclude })),
        )
    };
    let (status, all) = search(false).await;
    assert_eq!(status, StatusCode::OK, "{all}");
    assert_eq!(all["context_version"], version.as_str());
    assert_eq!(all["results"].as_array().expect("results").len(), 2);
    let (_, excluded) = search(true).await;
    let texts: Vec<&str> = excluded["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|hit| hit["memory"]["text"].as_str().expect("text"))
        .collect();
    assert_eq!(texts, ["Memoria do canal de busca sobre deploy."]);
    assert_eq!(excluded["context_version"], version.as_str());
}

#[tokio::test]
async fn mcp_tool_resources_and_prompt() {
    let network = network().await;
    let context = &network.context;
    let rest = version(context, &network.agent, "/v1/context").await;
    let result = context
        .call_tool(&network.agent, "get_context", json!({}))
        .await;
    let structured = &result["result"]["structuredContent"];
    assert_eq!(structured["version"], rest.as_str(), "{result}");
    let result = context
        .call_tool(
            &network.agent,
            "get_context",
            json!({ "if_none_match": rest }),
        )
        .await;
    assert_eq!(
        result["result"]["structuredContent"],
        json!({ "not_modified": true, "version": rest })
    );

    let (_, listed) = context
        .rpc(&network.agent, "resources/list", json!({}))
        .await;
    let uris: Vec<&str> = listed["result"]["resources"]
        .as_array()
        .expect("resources")
        .iter()
        .map(|resource| resource["uri"].as_str().expect("uri"))
        .collect();
    assert!(uris.contains(&"engramind://context"), "{uris:?}");
    assert!(uris.contains(&"engramind://channels/eng/global"));
    assert!(uris.contains(&"engramind://channels/eng/gestao"));
    assert!(uris.contains(&"engramind://guides/contexto-e-cache"));
    assert!(
        !uris.contains(&"engramind://guides/configuracoes"),
        "guia admin oculto"
    );

    let (_, templates) = context
        .rpc(&network.agent, "resources/templates/list", json!({}))
        .await;
    assert_eq!(
        templates["result"]["resourceTemplates"][0]["uriTemplate"],
        "engramind://channels/{slug}"
    );

    let (_, read) = context
        .rpc_with(
            &network.agent,
            "resources/read",
            json!({ "uri": "engramind://context" }),
            &[("Engramind-Channels", "eng/atlas")],
        )
        .await;
    let contents = &read["result"]["contents"][0];
    assert_eq!(contents["mimeType"], "text/plain");
    let text = contents["text"].as_str().expect("text");
    assert!(
        text.contains("eng/atlas") && !text.contains("eng/global"),
        "{text}"
    );

    let (_, read) = context
        .rpc(
            &network.agent,
            "resources/read",
            json!({ "uri": "engramind://channels/eng/gestao" }),
        )
        .await;
    let text = read["result"]["contents"][0]["text"]
        .as_str()
        .expect("text");
    assert!(
        text.starts_with("#m_") && text.contains(": Memoria do canal de gestao"),
        "{text}"
    );

    let (_, read) = context
        .rpc(
            &network.agent,
            "resources/read",
            json!({ "uri": "engramind://guides/contexto-e-cache" }),
        )
        .await;
    assert_eq!(read["result"]["contents"][0]["mimeType"], "text/markdown");

    for uri in [
        "engramind://nada",
        "engramind://channels/nao/existe",
        "engramind://guides/configuracoes",
    ] {
        let (_, read) = context
            .rpc(&network.agent, "resources/read", json!({ "uri": uri }))
            .await;
        assert_eq!(read["error"]["code"], -32002, "{uri}: {read}");
        assert!(read["error"]["data"]["error"]["code"].is_string(), "{read}");
    }
    context.create_channel("eng/sem-assinatura").await;
    let (_, read) = context
        .rpc(
            &network.agent,
            "resources/read",
            json!({ "uri": "engramind://channels/eng/sem-assinatura" }),
        )
        .await;
    assert_eq!(
        read["error"]["data"]["error"]["code"],
        "CHANNEL_NOT_SUBSCRIBED"
    );

    let (_, prompts) = context.rpc(&network.agent, "prompts/list", json!({})).await;
    assert_eq!(prompts["result"]["prompts"][0]["name"], "memory-writing");
    let (_, prompt) = context
        .rpc(
            &network.agent,
            "prompts/get",
            json!({ "name": "memory-writing" }),
        )
        .await;
    let text = prompt["result"]["messages"][0]["content"]["text"]
        .as_str()
        .expect("prompt");
    assert!(text.contains("Um fato por memória"), "{text}");
    let (_, missing) = context
        .rpc(&network.agent, "prompts/get", json!({ "name": "outro" }))
        .await;
    assert_eq!(missing["error"]["code"], -32002);

    let (_, initialize) = context
        .rpc(
            &network.agent,
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "teste", "version": "0" }
            }),
        )
        .await;
    let capabilities = &initialize["result"]["capabilities"];
    assert!(capabilities["resources"].is_object(), "{initialize}");
    assert!(capabilities["prompts"].is_object());
}

fn bin() -> String {
    std::env::var("NEXTEST_BIN_EXE_engramind")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_engramind").to_owned())
}

async fn cli(context: &TestContext, url: &str, token: &str, args: &[&str]) -> Output {
    let home = context.dir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let mut command = Command::new(bin());
    command
        .args(args)
        .env_clear()
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &home)
        .env("ENGRAMIND_URL", url)
        .env("ENGRAMIND_TOKEN", token);
    tokio::task::spawn_blocking(move || command.output().expect("run engramind"))
        .await
        .expect("join")
}

#[tokio::test]
async fn cli_prints_the_raw_block_and_honors_if_none_match() {
    let network = network().await;
    let context = &network.context;
    let url = context.serve().await;
    let (_, body, _) = get(context, &network.agent, "/v1/context").await;
    let version = body["version"].as_str().expect("version");

    let output = cli(context, &url, &network.agent, &["context"]).await;
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", content(&body))
    );

    let output = cli(
        context,
        &url,
        &network.agent,
        &["context", "--if-none-match", version],
    )
    .await;
    assert!(output.status.success());
    assert!(
        output.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );

    let output = cli(
        context,
        &url,
        &network.agent,
        &["context", "--if-none-match", version, "--json"],
    )
    .await;
    assert!(output.status.success());
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(parsed, json!({ "not_modified": true, "version": version }));

    let output = cli(
        context,
        &url,
        &network.agent,
        &["context", "--format", "json", "--json"],
    )
    .await;
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(parsed["format"], "json");
    assert!(parsed["content"]["channels"].is_array());

    let output = cli(
        context,
        &url,
        &network.agent,
        &["context", "--format", "json"],
    )
    .await;
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert!(
        parsed["channels"].is_array(),
        "o modo cru imprime só o content"
    );

    let output = cli(
        context,
        &url,
        &network.agent,
        &[
            "memories",
            "search",
            "testes",
            "deploy",
            "--exclude-context",
            "--json",
        ],
    )
    .await;
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(parsed["results"].as_array().expect("results").len(), 1);
    assert_eq!(parsed["context_version"], version);
}
