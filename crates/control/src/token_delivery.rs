use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use mycelium_proto::link::ControlToNode;
use uuid::Uuid;

pub fn already_decided() -> ApiError {
    ApiError::conflict("request_not_requested", "This request has already been decided.")
}

pub fn require_requested(state: &str) -> ApiResult<()> {
    if state != "requested" {
        return Err(already_decided());
    }

    Ok(())
}

pub async fn node_of(state: &AppState, conversation_id: Uuid) -> ApiResult<Uuid> {
    let node_id = sqlx::query_scalar::<_, Option<Uuid>>("select node_id from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;

    node_id.ok_or_else(|| ApiError::conflict("no_node", "This conversation does not run on a node, so there is nowhere to save the token. Decline the request."))
}

pub fn send_token(state: &AppState, node_id: Uuid, message: ControlToNode) -> ApiResult<()> {
    if !state.hub.send(node_id, message) {
        return Err(ApiError::conflict("node_offline", "The node this conversation runs on is offline, so the token cannot be delivered. Approve it again once the node is back."));
    }

    Ok(())
}
