use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::nodes::node_offline;
use crate::state::AppState;
use crate::store::publish_node;
use crate::versions::newer;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mycelium_proto::link::ControlToNode;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use uuid::Uuid;

static MESSAGES: LazyLock<Mutex<HashMap<Uuid, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn control_version() -> String {
    let version = option_env!("MYCELIUM_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));

    format!("v{}", version.trim_start_matches('v'))
}

fn order(state: &AppState) -> ControlToNode {
    ControlToNode::Update { releases_url: state.config.releases_url.trim_end_matches('/').to_string(), version: control_version() }
}

pub fn message_of(node_id: Uuid) -> Option<String> {
    MESSAGES.lock().ok().and_then(|messages| messages.get(&node_id).cloned())
}

pub async fn update(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let exists = sqlx::query_scalar::<_, bool>("select exists (select 1 from nodes where id = $1 and revoked_at is null)").bind(id).fetch_one(&state.pool).await?;

    if !exists {
        return Err(ApiError::not_found("That node"));
    }

    if !state.hub.is_online(id) || !state.hub.send(id, order(&state)) {
        return Err(node_offline());
    }

    Ok(StatusCode::NO_CONTENT)
}

pub fn offer_to_an_older_node(state: &AppState, node_id: Uuid, node_version: &str) {
    if let Ok(mut messages) = MESSAGES.lock() {
        messages.remove(&node_id);
    }

    if newer(node_version, Some(control_version())).is_some() {
        state.hub.send(node_id, order(state));
    }
}

pub async fn record_result(state: &AppState, node_id: Uuid, message: String) -> ApiResult<()> {
    if let Ok(mut messages) = MESSAGES.lock() {
        messages.insert(node_id, message);
    }

    publish_node(state, node_id).await
}
