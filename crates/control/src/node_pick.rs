use crate::agent_settings;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Query, State};
use mycelium_proto::link::{ControlToNode, ResourceSample};
use mycelium_proto::routing::{AnyNodePick, NodePreference};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

const WATCH_SECONDS: u32 = 10;
const FRESH_MILLISECONDS: i64 = 10_000;
const WAIT_FOR_SAMPLES: Duration = Duration::from_secs(4);
const SAMPLE_POLL: Duration = Duration::from_millis(200);
const UNMEASURED_FREE: f64 = 0.5;
const FREE_FLOOR: f64 = 0.01;

pub fn score(sample: Option<&ResourceSample>, preference: NodePreference) -> Option<f64> {
    let weight = preference.weight()?;
    let free = match sample {
        Some(sample) if sample.memory_total_bytes > 0 => {
            let cpu_free = (1.0 - f64::from(sample.cpu_percent) / 100.0).clamp(0.0, 1.0);
            let memory_free = (sample.memory_available_bytes as f64 / sample.memory_total_bytes as f64).clamp(0.0, 1.0);

            cpu_free * memory_free
        }
        _ => UNMEASURED_FREE,
    };

    Some(weight * free.max(FREE_FLOOR))
}

pub fn best(scored: &[(Uuid, f64)]) -> Option<Uuid> {
    scored.iter().max_by(|left, right| left.1.partial_cmp(&right.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| right.0.cmp(&left.0))).map(|(id, _)| *id)
}

fn is_fresh(sample: &ResourceSample) -> bool {
    chrono::Utc::now().timestamp_millis() - sample.at_ms < FRESH_MILLISECONDS
}

async fn measured(state: &AppState, online: &[Uuid]) -> HashMap<Uuid, ResourceSample> {
    for id in online {
        state.hub.send(*id, ControlToNode::Watch { seconds: WATCH_SECONDS });
    }

    let deadline = tokio::time::Instant::now() + WAIT_FOR_SAMPLES;

    loop {
        let samples: HashMap<Uuid, ResourceSample> = online.iter().filter_map(|id| state.hub.resources_of(*id).latest.map(|sample| (*id, sample))).collect();
        let waiting = online.iter().any(|id| !samples.get(id).is_some_and(is_fresh));

        if !waiting || tokio::time::Instant::now() >= deadline {
            return samples;
        }

        tokio::time::sleep(SAMPLE_POLL).await;
    }
}

pub async fn pick(state: &AppState, owner: Uuid, among: Option<&[Uuid]>) -> ApiResult<AnyNodePick> {
    let nodes = sqlx::query_as::<_, (Uuid, String)>("select id, name from nodes where created_by = $1 and revoked_at is null and not draining order by name").bind(owner).fetch_all(&state.pool).await?;
    let online: Vec<(Uuid, String)> = nodes.into_iter().filter(|(id, _)| state.hub.is_online(*id) && among.is_none_or(|allowed| allowed.contains(id))).collect();
    let preferences = agent_settings::read(state, owner).await?.node_preferences;
    let ids: Vec<Uuid> = online.iter().map(|(id, _)| *id).collect();
    let samples = measured(state, &ids).await;
    let scored: Vec<(Uuid, f64)> = online.iter().filter_map(|(id, _)| score(samples.get(id), preferences.get(id).copied().unwrap_or_default()).map(|score| (*id, score))).collect();
    let chosen = best(&scored).and_then(|id| online.iter().find(|(candidate, _)| *candidate == id));

    chosen.map(|(node_id, name)| AnyNodePick { node_id: *node_id, name: name.clone() }).ok_or_else(|| ApiError::conflict("no_node_available", "No node can start a conversation right now. Bring one online, or change a node set to Never in Settings, Agents."))
}

#[derive(serde::Deserialize)]
pub struct PreviewQuery {
    project_id: Option<Uuid>,
}

pub async fn preview(State(state): State<AppState>, user: AuthUser, Query(query): Query<PreviewQuery>) -> ApiResult<Json<AnyNodePick>> {
    let holding = match query.project_id {
        Some(id) => {
            let project = crate::projects::find_owned(&state, id, user.id).await?;

            Some(crate::projects::nodes_holding(&state, user.id, &project).await?)
        }
        None => None,
    };

    Ok(Json(pick(&state, user.id, holding.as_deref()).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(cpu_percent: f32, available: u64) -> ResourceSample {
        ResourceSample { at_ms: 0, available: true, cpu_percent, cores: 8, load: [0.0; 3], memory_total_bytes: 100, memory_available_bytes: available, swap_total_bytes: 0, swap_free_bytes: 0, disks: Vec::new(), processes: Vec::new(), ports: Vec::new(), docker: None }
    }

    #[test]
    fn an_idle_node_outscores_a_busy_one() {
        let idle = score(Some(&sample(10.0, 80)), NodePreference::Normal).unwrap();
        let busy = score(Some(&sample(90.0, 80)), NodePreference::Normal).unwrap();

        assert!(idle > busy);
    }

    #[test]
    fn little_free_memory_lowers_the_score_as_much_as_a_busy_cpu() {
        let cramped = score(Some(&sample(0.0, 10)), NodePreference::Normal).unwrap();
        let roomy = score(Some(&sample(0.0, 90)), NodePreference::Normal).unwrap();

        assert!(roomy > cramped);
    }

    #[test]
    fn a_preferred_node_wins_over_an_equally_free_normal_one_and_an_avoided_node_loses() {
        let free = sample(50.0, 50);
        let preferred = score(Some(&free), NodePreference::Preferred).unwrap();
        let normal = score(Some(&free), NodePreference::Normal).unwrap();
        let avoided = score(Some(&free), NodePreference::Avoid).unwrap();

        assert!(preferred > normal && normal > avoided);
    }

    #[test]
    fn a_node_set_to_never_is_not_scored() {
        assert_eq!(score(Some(&sample(0.0, 100)), NodePreference::Never), None);
    }

    #[test]
    fn an_avoided_node_still_wins_when_the_others_are_saturated() {
        let avoided_idle = score(Some(&sample(0.0, 100)), NodePreference::Avoid).unwrap();
        let normal_saturated = score(Some(&sample(99.0, 1)), NodePreference::Normal).unwrap();

        assert!(avoided_idle > normal_saturated);
    }

    #[test]
    fn the_best_node_is_the_highest_score_and_ties_go_to_the_lowest_id() {
        let (low, high) = (Uuid::from_u128(1), Uuid::from_u128(2));

        assert_eq!(best(&[(high, 1.0), (low, 1.0)]), Some(low));
        assert_eq!(best(&[(low, 0.5), (high, 1.0)]), Some(high));
        assert_eq!(best(&[]), None);
    }
}
