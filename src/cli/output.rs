//! Único ponto de escrita no terminal da CLI. O servidor nunca escreve aqui: ele usa `tracing`.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use serde::Serialize;
use serde_json::Value;

pub fn json<T: Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(text) => println!("{text}"),
        Err(error) => eprintln!("erro: falha ao serializar a saida: {error}"),
    }
}

pub fn line(text: &str) {
    println!("{text}");
}

pub fn warn(text: &str) {
    eprintln!("{text}");
}

pub fn failure(error: &anyhow::Error) {
    eprintln!("erro: {error:#}");
}

/// Erro no envelope do contrato: JSON no stdout com `--json`, linha legível no stderr sem.
pub fn error_envelope(envelope: &Value, as_json: bool) {
    if as_json {
        json(envelope);
        return;
    }
    let error = &envelope["error"];
    let code = error["code"].as_str().unwrap_or("ERROR");
    let message = error["message"].as_str().unwrap_or_default();
    eprintln!("erro [{code}]: {message}");
    if let Some(details) = error.get("details").filter(|details| !details.is_null()) {
        eprintln!("detalhes: {details}");
    }
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

/// Campos mostrados nas listas, nesta ordem (os demais ficam para `--json`).
const LIST_FIELDS: &[&str] = &[
    "id",
    "key",
    "qualified_slug",
    "slug",
    "name",
    "kind",
    "agent",
    "target",
    "mode",
    "scopes",
    "status",
    "value",
    "at",
    "action",
    "entity",
    "entity_id",
    "operation",
    "method",
    "path",
    "guide",
    "section",
    "title",
    "summary",
    "url",
    "token",
    "default",
];

fn field_text(value: &Value) -> Option<String> {
    match value {
        Value::Object(inner) => inner.get("slug").and_then(scalar),
        Value::Array(items) => Some(
            items
                .iter()
                .filter_map(scalar)
                .collect::<Vec<_>>()
                .join(","),
        ),
        other => scalar(other),
    }
}

/// Uma linha por item com os campos principais separados por dois espaços.
fn item_line(item: &Value) -> String {
    let Value::Object(map) = item else {
        return scalar(item).unwrap_or_default();
    };
    let mut fields: Vec<String> = LIST_FIELDS
        .iter()
        .filter_map(|name| map.get(*name))
        .filter_map(field_text)
        .filter(|text| !text.is_empty())
        .collect();
    if fields.is_empty() {
        fields = map.values().filter_map(field_text).collect();
    }
    fields.join("  ")
}

fn object_lines(map: &serde_json::Map<String, Value>, indent: &str) {
    for (key, value) in map {
        match value {
            Value::Null => {}
            Value::Array(items) if items.iter().all(|item| !item.is_object()) => line(&format!(
                "{indent}{key}: {}",
                items
                    .iter()
                    .filter_map(scalar)
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            Value::Array(items) => {
                line(&format!("{indent}{key}:"));
                for item in items {
                    line(&format!("{indent}  - {}", item_line(item)));
                }
            }
            Value::Object(inner) if inner.len() == 2 && inner.contains_key("slug") => {
                line(&format!(
                    "{indent}{key}: {}",
                    scalar(&inner["slug"]).unwrap_or_default()
                ))
            }
            Value::Object(inner) => {
                line(&format!("{indent}{key}:"));
                object_lines(inner, &format!("{indent}  "));
            }
            other => line(&format!(
                "{indent}{key}: {}",
                scalar(other).unwrap_or_default()
            )),
        }
    }
}

/// `engramind context` sem `--json`: o `content` cru, para colar no prompt (JSON formatado no
/// formato `json`); nada quando não mudou.
pub fn context(body: &Value) {
    match &body["content"] {
        Value::String(content) => line(content),
        Value::Null => {}
        document => json(document),
    }
}

/// Saída legível genérica: Markdown de `content`, listas em linhas, objetos em `chave: valor`.
pub fn human(value: &Value) {
    let Value::Object(map) = value else {
        line(&scalar(value).unwrap_or_default());
        return;
    };
    if let Some(Value::String(content)) = map.get("content") {
        line(content);
        return;
    }
    let lists: Vec<(&String, &Vec<Value>)> = map
        .iter()
        .filter_map(|(key, value)| value.as_array().map(|items| (key, items)))
        .collect();
    let only_list = lists.len() == 1
        && map
            .keys()
            .all(|key| key == lists[0].0 || key == "next_cursor" || key == "service");
    if only_list {
        for item in lists[0].1 {
            line(&item_line(item));
        }
        if let Some(cursor) = map.get("next_cursor").and_then(Value::as_str) {
            line(&format!("(mais itens: --cursor {cursor})"));
        }
        return;
    }
    object_lines(map, "");
}
