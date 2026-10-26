use crate::auth::AuthUser;
use crate::conversations::require_signed_in_account;
use crate::error::{ApiError, ApiResult};
use crate::nodes::node_has;
use crate::state::AppState;
use crate::store::{CONVERSATION_COLUMNS, ConversationRow, append_event, lock_owned, parse_enum};
use axum::Json;
use axum::extract::{Path, State};
use mycelium_proto::api::ContinueRequest;
use mycelium_proto::conversation::{AgentId, Conversation, EventKind};
use mycelium_proto::link::ControlToNode;
use mycelium_proto::stream::StreamMessage;
use serde_json::json;
use uuid::Uuid;

pub const ADOPT_SINCE: (u64, u64, u64) = (0, 11, 0);

pub struct Target {
    pub agent: String,
    pub account: Option<String>,
}

pub async fn request(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<ContinueRequest>) -> ApiResult<Json<Conversation>> {
    user.require_full_access()?;
    let account = request.account.as_deref().map(str::trim).filter(|account| !account.is_empty()).map(str::to_string);
    let target = Target { agent: request.agent, account };
    let row = continue_on(&state, user.id, id, target, None).await?;

    Ok(Json(row.to_proto()?))
}

pub async fn continue_on(state: &AppState, owner: Uuid, id: Uuid, target: Target, reason: Option<&str>) -> ApiResult<ConversationRow> {
    let mut tx = state.pool.begin().await?;
    let before = lock_owned(&mut tx, id, owner).await?;

    if matches!(before.status.as_str(), "working" | "input") {
        return Err(ApiError::conflict("conversation_busy", "This conversation is still working. Wait for the turn to finish or stop it, then continue elsewhere."));
    }

    if before.agent != target.agent {
        return Err(ApiError::bad_request("handoff_unavailable", "Continuing on another harness or model is not available yet."));
    }

    if before.account == target.account {
        return Err(ApiError::conflict("same_target", "This conversation already runs there. Choose another account or model."));
    }

    let agent: AgentId = parse_enum(&before.agent)?;

    if let Some(account) = &target.account {
        require_signed_in_account(state, before.node_id, agent, account).await?;
    }

    let node = sqlx::query_as::<_, (String, String)>("select name, version from nodes where id = $1").bind(before.node_id).fetch_one(&mut *tx).await?;

    if !node_has(&node.1, ADOPT_SINCE) {
        return Err(ApiError::conflict("node_outdated", format!("Update {} to continue on another account.", node.0)));
    }

    let row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("update conversations set account = $2 where id = $1 returning {CONVERSATION_COLUMNS}")))
        .bind(id)
        .bind(&target.account)
        .fetch_one(&mut *tx)
        .await?;
    let payload = json!({
        "kind": "account",
        "agent": target.agent,
        "account": target.account,
        "from": {"agent": before.agent, "account": before.account},
        "reason": reason,
    });
    let event = append_event(&mut tx, id, EventKind::Continued, &payload, None).await?;
    tx.commit().await?;

    state.hub.send(row.node_id, ControlToNode::CloseSession { conversation_id: id });
    state.remove_session(row.node_id, id).await?;

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    state.publish(Some(owner), StreamMessage::Conversation(row.to_proto()?));

    Ok(row)
}
