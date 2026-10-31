//! `get_usage` (008 FR-013): `usage_log` agregado por dia, agente, operação ou canal.

use chrono::Duration;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};

use super::{
    Core, ids,
    model::{UsageGroup, UsageQuery},
    principal::Principal,
};
use crate::{error::ApiError, store::Store};

/// Período padrão sem `since`.
const DEFAULT_DAYS: i64 = 7;
/// Intervalo máximo de uma consulta.
const MAX_DAYS: i64 = 366;

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct GetUsageQuery {
    /// Início (RFC 3339, inclusivo); padrão: 7 dias antes de `until`.
    pub since: Option<String>,
    /// Fim (RFC 3339, exclusivo); padrão: agora.
    pub until: Option<String>,
    /// `day` (padrão, UTC), `agent`, `operation` ou `channel`.
    pub group_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UsageTotals {
    /// Requisições registradas (`get_context` entregue, `search_memories`, `get_hints`).
    #[schema(example = 120)]
    pub requests: u64,
    /// Memórias devolvidas ou injetadas.
    #[schema(example = 2400)]
    pub memories: u64,
    /// Tokens do bloco renderizado (`get_context`).
    #[schema(example = 380_000)]
    pub rendered_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UsageRow {
    /// Dia (`AAAA-MM-DD`), id do agente (ou do app, para token sem agente), operação ou id do
    /// canal.
    #[schema(example = "ag_3f9e1a7c")]
    pub key: String,
    /// `app/agente`, slug do app ou do canal; `null` quando já não existe.
    #[schema(example = "app-a/desenvolvedor")]
    pub label: Option<String>,
    #[serde(flatten)]
    pub totals: UsageTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UsageReport {
    #[schema(example = "2026-10-24T00:00:00.000Z")]
    pub since: String,
    #[schema(example = "2026-10-31T00:00:00.000Z")]
    pub until: String,
    pub group_by: UsageGroup,
    pub rows: Vec<UsageRow>,
    /// Soma do período (no agrupamento por canal, uma requisição conta em cada canal das
    /// linhas, mas uma vez só aqui).
    pub total: UsageTotals,
}

fn instant(
    field: &str,
    value: Option<&str>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, ApiError> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => ids::parse_timestamp(value).map(Some).ok_or_else(|| {
            ApiError::validation(format!("{field} deve ser um instante RFC 3339"))
                .with_details(json!({ "field": field }))
        }),
        None => Ok(None),
    }
}

fn group(value: Option<&str>) -> Result<UsageGroup, ApiError> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("day") => Ok(UsageGroup::Day),
        Some("agent") => Ok(UsageGroup::Agent),
        Some("operation") => Ok(UsageGroup::Operation),
        Some("channel") => Ok(UsageGroup::Channel),
        Some(_) => Err(
            ApiError::validation("group_by deve ser day, agent, operation ou channel")
                .with_details(json!({ "field": "group_by" })),
        ),
    }
}

/// Período validado: `since < until` e no máximo `MAX_DAYS`.
fn period(
    query: &GetUsageQuery,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(String, String), ApiError> {
    let until = instant("until", query.until.as_deref())?.unwrap_or(now);
    let since =
        instant("since", query.since.as_deref())?.unwrap_or(until - Duration::days(DEFAULT_DAYS));
    if since >= until {
        return Err(ApiError::validation("since deve ser anterior a until")
            .with_details(json!({ "field": "since" })));
    }
    if until - since > Duration::days(MAX_DAYS) {
        return Err(
            ApiError::validation(format!("o periodo aceita no maximo {MAX_DAYS} dias"))
                .with_details(json!({ "field": "since" })),
        );
    }
    Ok((ids::timestamp(since), ids::timestamp(until)))
}

impl<S: Store> Core<S> {
    pub async fn get_usage(
        &self,
        principal: &Principal,
        query: GetUsageQuery,
    ) -> Result<UsageReport, ApiError> {
        self.authorize(principal, "get_usage")?;
        let group = group(query.group_by.as_deref())?;
        let (since, until) = period(&query, ids::now())?;
        let aggregates = self
            .store()
            .usage_report(UsageQuery {
                since: since.clone(),
                until: until.clone(),
                group,
            })
            .await?;
        // O total vem sempre por dia: no agrupamento por canal as linhas se sobrepõem.
        let by_day = if group == UsageGroup::Day {
            aggregates.clone()
        } else {
            self.store()
                .usage_report(UsageQuery {
                    since: since.clone(),
                    until: until.clone(),
                    group: UsageGroup::Day,
                })
                .await?
        };
        let total = UsageTotals {
            requests: by_day.iter().map(|row| row.requests).sum(),
            memories: by_day.iter().map(|row| row.memories).sum(),
            rendered_tokens: by_day.iter().map(|row| row.rendered_tokens).sum(),
        };
        let network = self.network().await?;
        let mut rows = Vec::with_capacity(aggregates.len());
        for row in aggregates {
            let label = match group {
                UsageGroup::Day | UsageGroup::Operation => Some(row.key.clone()),
                UsageGroup::Channel => network
                    .channels
                    .iter()
                    .find(|channel| channel.id == row.key)
                    .map(|channel| channel.slug.clone()),
                UsageGroup::Agent if row.key.starts_with("app_") => {
                    self.find_app(&row.key).await.ok().map(|app| app.slug)
                }
                UsageGroup::Agent => self
                    .find_agent(&row.key)
                    .await
                    .ok()
                    .map(|agent| agent.qualified_slug),
            };
            rows.push(UsageRow {
                key: row.key,
                label,
                totals: UsageTotals {
                    requests: row.requests,
                    memories: row.memories,
                    rendered_tokens: row.rendered_tokens,
                },
            });
        }
        Ok(UsageReport {
            since,
            until,
            group_by: group,
            rows,
            total,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(since: Option<&str>, until: Option<&str>) -> GetUsageQuery {
        GetUsageQuery {
            since: since.map(str::to_owned),
            until: until.map(str::to_owned),
            group_by: None,
        }
    }

    #[test]
    fn period_defaults_to_seven_days_and_is_bounded() {
        let now = ids::parse_timestamp("2026-10-31T00:00:00Z").expect("now");
        assert_eq!(
            period(&query(None, None), now).expect("padrao"),
            (
                "2026-10-24T00:00:00.000Z".to_owned(),
                "2026-10-31T00:00:00.000Z".to_owned()
            )
        );
        assert!(period(&query(Some("2026-11-01T00:00:00Z"), None), now).is_err());
        assert!(period(&query(Some("2024-01-01T00:00:00Z"), None), now).is_err());
        assert!(period(&query(Some("ontem"), None), now).is_err());
    }

    #[test]
    fn group_by_accepts_only_the_four_groupings() {
        assert_eq!(group(None).expect("padrao"), UsageGroup::Day);
        assert_eq!(group(Some("channel")).expect("canal"), UsageGroup::Channel);
        assert_eq!(
            group(Some("token")).expect_err("x").code,
            "VALIDATION_ERROR"
        );
    }
}
