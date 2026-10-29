use crate::artifacts;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;

const POINTS_PER_SERIES: i64 = 120;
const QUERY_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Deserialize)]
pub struct RangeQuery {
    range: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Series {
    name: String,
    points: Vec<(i64, f64)>,
}

#[derive(Serialize)]
pub struct PanelData {
    series: Vec<Series>,
}

fn range_seconds(text: &str) -> ApiResult<i64> {
    match text {
        "1h" => Ok(3600),
        "24h" => Ok(86_400),
        "7d" => Ok(7 * 86_400),
        "30d" => Ok(30 * 86_400),
        _ => Err(ApiError::bad_request("range_invalid", "Choose a time range of 1h, 24h, 7d or 30d.")),
    }
}

fn find_panel<'a>(panels: &'a [Value], id: i64) -> Option<&'a Value> {
    for panel in panels {
        if panel["id"].as_i64() == Some(id) {
            return Some(panel);
        }

        if let Some(found) = panel["panels"].as_array().and_then(|nested| find_panel(nested, id)) {
            return Some(found);
        }
    }

    None
}

pub fn series_from_prometheus(response: &Value, legend: Option<&str>) -> Vec<Series> {
    response["data"]["result"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|result| {
            let labels = result["metric"].as_object();
            let name = legend.map(String::from).unwrap_or_else(|| labels.map(|labels| labels.iter().filter(|(label, _)| label.as_str() != "__name__").map(|(label, value)| format!("{label}={}", value.as_str().unwrap_or_default())).collect::<Vec<_>>().join(", ")).unwrap_or_default());
            let points = result["values"].as_array().into_iter().flatten().filter_map(|pair| Some((pair[0].as_f64()? as i64 * 1000, pair[1].as_str()?.parse().ok()?))).collect();

            Series { name, points }
        })
        .collect()
}

pub async fn panel_data(State(state): State<AppState>, user: AuthUser, Path((id, panel_id)): Path<(Uuid, i64)>, Query(query): Query<RangeQuery>) -> ApiResult<Json<PanelData>> {
    user.require_operator(&state)?;

    let seconds = range_seconds(query.range.as_deref().unwrap_or("24h"))?;
    let dashboard = artifacts::dashboard_of(&state, id, user.id).await?;
    let panels = dashboard["panels"].as_array().cloned().unwrap_or_default();
    let panel = find_panel(&panels, panel_id).ok_or_else(|| ApiError::not_found("That panel"))?;
    let Some(base) = &state.config.prometheus_url else {
        return Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "datasource_unavailable", "No data source is configured. Ask the operator to set MYCELIUM_PROMETHEUS_URL."));
    };

    if panel["datasource"]["type"].as_str() != Some("prometheus") {
        return Err(ApiError::bad_request("datasource_unsupported", "This panel reads a data source other than Prometheus, which Mycelium cannot query. Change the panel to use Prometheus."));
    }

    let end = chrono::Utc::now().timestamp();
    let start = end - seconds;
    let step = (seconds / POINTS_PER_SERIES).max(15);
    let client = reqwest::Client::builder().timeout(QUERY_TIMEOUT).build().map_err(ApiError::internal)?;
    let mut series = Vec::new();

    for target in panel["targets"].as_array().into_iter().flatten() {
        let Some(expression) = target["expr"].as_str().filter(|expression| !expression.trim().is_empty()) else {
            continue;
        };
        let url = format!("{}/api/v1/query_range", base.trim_end_matches('/'));
        let response = client
            .get(url)
            .query(&[("query", expression.to_string()), ("start", start.to_string()), ("end", end.to_string()), ("step", step.to_string())])
            .send()
            .await
            .map_err(|_| ApiError::new(StatusCode::BAD_GATEWAY, "datasource_unreachable", "The data source did not answer. Check that Prometheus is running, then refresh."))?;
        let body: Value = response.json().await.map_err(|_| ApiError::new(StatusCode::BAD_GATEWAY, "datasource_invalid", "The data source answered with something that is not Prometheus JSON."))?;

        if body["status"] != "success" {
            return Err(ApiError::bad_request("query_failed", format!("The data source rejected the query: {}", body["error"].as_str().unwrap_or("no reason given"))));
        }

        series.extend(series_from_prometheus(&body, target["legendFormat"].as_str().filter(|legend| !legend.contains("{{"))));
    }

    Ok(Json(PanelData { series }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_prometheus_matrix_becomes_named_series_with_millisecond_times() {
        let response = json!({"status": "success", "data": {"result": [{"metric": {"__name__": "up", "job": "restic"}, "values": [[1700000000, "1.5"], [1700000060, "2"]]}]}});

        assert_eq!(series_from_prometheus(&response, None), vec![Series { name: "job=restic".into(), points: vec![(1_700_000_000_000, 1.5), (1_700_000_060_000, 2.0)] }]);
    }

    #[test]
    fn a_panel_nested_in_a_row_is_found_by_id() {
        let panels = vec![json!({"id": 1, "type": "row", "panels": [{"id": 5, "type": "stat"}]})];

        assert_eq!(find_panel(&panels, 5).map(|panel| panel["type"].clone()), Some(json!("stat")));
    }
}
