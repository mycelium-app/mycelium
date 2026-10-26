use crate::auth::AuthUser;
use crate::chats::{self, CHAT_AGENT};
use crate::conversations::require_signed_in_account;
use crate::error::{ApiError, ApiResult};
use crate::model_providers;
use crate::nodes::node_has;
use crate::state::AppState;
use crate::store::{CONVERSATION_COLUMNS, ConversationRow, append_event, lock_owned, parse_enum};
use axum::Json;
use axum::extract::{Path, State};
use mycelium_proto::api::ContinueRequest;
use mycelium_proto::conversation::{AgentId, Conversation, EventKind};
use mycelium_proto::link::ControlToNode;
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

pub const ADOPT_SINCE: (u64, u64, u64) = (0, 10, 0);
pub const SUMMARY_MESSAGES: i64 = 8;
pub const SUMMARY_MESSAGE_LIMIT: usize = 1500;
const FILES_LISTED: i64 = 30;

pub struct Target {
    pub agent: String,
    pub account: Option<String>,
    pub provider_id: Option<Uuid>,
    pub model: Option<String>,
    pub node_id: Option<Uuid>,
    pub cwd: Option<String>,
}

pub async fn request(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<ContinueRequest>) -> ApiResult<Json<Conversation>> {
    user.require_full_access()?;
    let account = request.account.as_deref().map(str::trim).filter(|account| !account.is_empty()).map(str::to_string);
    let target = Target { agent: request.agent, account, provider_id: request.provider_id, model: request.model, node_id: request.node_id, cwd: request.cwd };
    let row = continue_on(&state, user.id, id, target, None).await?;

    Ok(Json(row.to_proto()?))
}

async fn provider_name(state: &AppState, provider_id: Option<Uuid>) -> ApiResult<Option<String>> {
    let Some(provider_id) = provider_id else {
        return Ok(None);
    };

    Ok(sqlx::query_scalar::<_, String>("select name from model_providers where id = $1").bind(provider_id).fetch_optional(&state.pool).await?)
}

pub async fn continue_on(state: &AppState, owner: Uuid, id: Uuid, target: Target, reason: Option<&str>) -> ApiResult<ConversationRow> {
    let mut tx = state.pool.begin().await?;
    let before = lock_owned(&mut tx, id, owner).await?;

    if matches!(before.status.as_str(), "working" | "input") {
        return Err(ApiError::conflict("conversation_busy", "This conversation is still working. Wait for the turn to finish or stop it, then continue elsewhere."));
    }

    let from_chat = chats::is_chat(&before);
    let to_chat = target.agent == CHAT_AGENT;

    if from_chat && to_chat && reason.is_none() {
        return Err(ApiError::bad_request("already_a_chat", "This is already a chat. Change its model in the message box."));
    }

    let by_account = !from_chat && before.agent == target.agent && target.node_id.is_none_or(|node_id| node_id == before.node_id);

    if by_account && before.account == target.account {
        return Err(ApiError::conflict("same_target", "This conversation already runs there. Choose another account or model."));
    }

    let before_provider_name = provider_name(state, before.provider_id).await?;
    let before_node_name = sqlx::query_scalar::<_, String>("select name from nodes where id = $1").bind(before.node_id).fetch_one(&mut *tx).await?;
    let from = json!({
        "agent": before.agent, "account": before.account, "provider_id": before.provider_id, "provider_name": before_provider_name, "model": before.model,
        "node_id": before.node_id, "node_name": before_node_name, "cwd": before.cwd, "project_id": before.project_id,
    });

    let row;
    let payload;

    if by_account {
        let agent: AgentId = parse_enum(&before.agent)?;

        if let Some(account) = &target.account {
            require_signed_in_account(state, before.node_id, agent, account).await?;
        }

        let version = sqlx::query_scalar::<_, String>("select version from nodes where id = $1").bind(before.node_id).fetch_one(&mut *tx).await?;

        if !node_has(&version, ADOPT_SINCE) {
            return Err(ApiError::conflict("node_outdated", format!("Update {before_node_name} to continue on another account.")));
        }

        row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("update conversations set account = $2 where id = $1 returning {CONVERSATION_COLUMNS}")))
            .bind(id)
            .bind(&target.account)
            .fetch_one(&mut *tx)
            .await?;
        payload = json!({"kind": "account", "agent": target.agent, "account": target.account, "from": from, "reason": reason});
    } else if to_chat {
        let Some(provider_id) = target.provider_id else {
            return Err(ApiError::bad_request("model_required", "Choose a model for this chat."));
        };
        let provider = model_providers::find_owned(state, provider_id, owner).await?;
        let Some(model) = target.model.clone().or(provider.default_model.clone()) else {
            return Err(ApiError::bad_request("model_required", "Choose a model for this chat."));
        };
        let chat_node = chats::chat_node(state, owner).await?;

        row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
            "update conversations set agent = $2, account = null, provider_id = $3, model = $4, node_id = $5, cwd = '', project_id = null,
                 previous_session_ids = case when acp_session_id is null then previous_session_ids else array_append(previous_session_ids, acp_session_id) end, acp_session_id = null
             where id = $1 returning {CONVERSATION_COLUMNS}"
        )))
        .bind(id)
        .bind(CHAT_AGENT)
        .bind(provider.id)
        .bind(&model)
        .bind(chat_node)
        .fetch_one(&mut *tx)
        .await?;
        payload = json!({"kind": if from_chat { "model" } else { "handoff" }, "agent": CHAT_AGENT, "provider_id": provider.id, "provider_name": provider.name, "model": model, "from": from, "reason": reason});
    } else {
        let agent: AgentId = parse_enum(&target.agent).map_err(|_| ApiError::bad_request("agent_unknown", "Choose Claude Code, Codex or a chat model to continue on."))?;
        let earlier = if from_chat { earlier_workspace(&mut tx, id).await? } else { None };
        let node_id = target.node_id.or_else(|| earlier.as_ref().and_then(|earlier| earlier["node_id"].as_str()?.parse().ok())).unwrap_or(before.node_id);
        let cwd = target.cwd.clone().or_else(|| earlier.as_ref().and_then(|earlier| earlier["cwd"].as_str().map(str::to_string))).filter(|cwd| !cwd.is_empty()).or_else(|| (!from_chat).then(|| before.cwd.clone()));

        let Some(cwd) = cwd else {
            return Err(ApiError::bad_request("folder_required", "Choose a node and folder to continue on."));
        };

        let node_name = sqlx::query_scalar::<_, String>("select name from nodes where id = $1 and created_by = $2 and revoked_at is null").bind(node_id).bind(owner).fetch_optional(&mut *tx).await?;
        let Some(node_name) = node_name else {
            return Err(ApiError::not_found("That node"));
        };
        let available = sqlx::query_scalar::<_, bool>("select available from agents where node_id = $1 and agent = $2").bind(node_id).bind(&target.agent).fetch_optional(&mut *tx).await?;

        if available == Some(false) {
            return Err(ApiError::conflict("agent_unavailable", format!("{} is not installed on that node. Install it there or choose another node.", target.agent)));
        }

        if let Some(account) = &target.account {
            require_signed_in_account(state, node_id, agent, account).await?;
        }

        let earlier_project = earlier.as_ref().filter(|earlier| earlier["node_id"].as_str() == Some(node_id.to_string().as_str())).and_then(|earlier| earlier["project_id"].as_str()?.parse::<Uuid>().ok());
        let project_id = if node_id == before.node_id { before.project_id } else { earlier_project };

        row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
            "update conversations set agent = $2, account = $3, provider_id = null, model = null, node_id = $4, cwd = $5, project_id = $6,
                 previous_session_ids = case when acp_session_id is null then previous_session_ids else array_append(previous_session_ids, acp_session_id) end, acp_session_id = null
             where id = $1 returning {CONVERSATION_COLUMNS}"
        )))
        .bind(id)
        .bind(&target.agent)
        .bind(&target.account)
        .bind(node_id)
        .bind(&cwd)
        .bind(project_id)
        .fetch_one(&mut *tx)
        .await?;
        let moved = (node_id != before.node_id).then_some((node_id, node_name));
        payload = json!({
            "kind": "handoff", "agent": target.agent, "account": target.account, "cwd": cwd,
            "node_id": moved.as_ref().map(|(id, _)| id), "node_name": moved.as_ref().map(|(_, name)| name), "from": from, "reason": reason,
        });
    }

    let event = append_event(&mut tx, id, EventKind::Continued, &payload, None).await?;
    tx.commit().await?;

    if !from_chat {
        state.hub.send(before.node_id, ControlToNode::CloseSession { conversation_id: id });
        state.remove_session(before.node_id, id).await?;
    }

    if let Some(event) = event {
        state.publish(Some(owner), StreamMessage::Event(event));
    }

    state.publish(Some(owner), StreamMessage::Conversation(row.to_proto()?));

    Ok(row)
}

async fn earlier_workspace(tx: &mut crate::store::Tx<'_>, id: Uuid) -> ApiResult<Option<Value>> {
    let found = sqlx::query_scalar::<_, sqlx::types::Json<Value>>(
        "select payload->'from' from events where conversation_id = $1 and kind = 'continued' and payload->'from'->>'agent' in ('claude', 'codex') order by seq desc limit 1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;

    Ok(found.map(|found| found.0))
}

pub async fn last_handoff_marker(state: &AppState, id: Uuid) -> ApiResult<Option<i64>> {
    let seq = sqlx::query_scalar::<_, i64>("select seq from events where conversation_id = $1 and kind = 'continued' and payload->>'kind' = 'handoff' order by seq desc limit 1").bind(id).fetch_optional(&state.pool).await?;

    Ok(seq)
}

pub async fn pending_handoff(state: &AppState, id: Uuid) -> ApiResult<Option<i64>> {
    let unstarted = sqlx::query_scalar::<_, bool>("select acp_session_id is null and agent <> $2 from conversations where id = $1").bind(id).bind(CHAT_AGENT).fetch_optional(&state.pool).await?;

    if unstarted != Some(true) {
        return Ok(None);
    }

    let Some(marker) = last_handoff_marker(state, id).await? else {
        return Ok(None);
    };
    let delivered = sqlx::query_scalar::<_, bool>("select exists (select 1 from events where conversation_id = $1 and kind = 'user_message' and seq > $2)").bind(id).bind(marker).fetch_one(&state.pool).await?;

    Ok(if delivered { None } else { Some(marker) })
}

pub async fn recent_turns(state: &AppState, id: Uuid, before_seq: i64, limit: i64) -> ApiResult<Vec<String>> {
    let said = sqlx::query_as::<_, (String, String)>(
        "select * from (
            select e.kind, s.text, s.seq from search_entries s join events e on e.conversation_id = s.conversation_id and e.seq = s.seq
            where s.conversation_id = $1 and s.seq < $2 order by s.seq desc limit $3
         ) recent order by seq",
    )
    .bind(id)
    .bind(before_seq)
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;

    Ok(said.into_iter().map(|(kind, text)| format!("{}: {}", if kind == "user_message" { "User" } else { "Agent" }, text.chars().take(SUMMARY_MESSAGE_LIMIT).collect::<String>())).collect())
}

fn side_label(agent: &str, account: Option<&str>, provider_name: Option<&str>, model: Option<&str>) -> String {
    match agent {
        "claude" => format!("Claude Code{}", account.map(|account| format!(" ({account})")).unwrap_or_default()),
        "codex" => format!("Codex{}", account.map(|account| format!(" ({account})")).unwrap_or_default()),
        _ => [provider_name, model].into_iter().flatten().collect::<Vec<_>>().join(" "),
    }
}

pub async fn handoff(state: &AppState, id: Uuid, before_seq: i64) -> ApiResult<String> {
    let marker = sqlx::query_scalar::<_, sqlx::types::Json<Value>>("select payload from events where conversation_id = $1 and seq = $2").bind(id).bind(before_seq).fetch_optional(&state.pool).await?.map(|found| found.0).unwrap_or(Value::Null);
    let from = &marker["from"];
    let text_of = |value: &Value| value.as_str().map(str::to_string);
    let from_label = side_label(from["agent"].as_str().unwrap_or_default(), text_of(&from["account"]).as_deref(), text_of(&from["provider_name"]).as_deref(), text_of(&from["model"]).as_deref());
    let title = sqlx::query_as::<_, (String, String, String)>("select c.title, c.cwd, n.name from conversations c join nodes n on n.id = c.node_id where c.id = $1").bind(id).fetch_one(&state.pool).await?;
    let goal = sqlx::query_scalar::<_, String>("select text from goals where conversation_id = $1").bind(id).fetch_optional(&state.pool).await?;
    let first = sqlx::query_scalar::<_, String>(
        "select s.text from search_entries s join events e on e.conversation_id = s.conversation_id and e.seq = s.seq
         where s.conversation_id = $1 and s.seq < $2 and e.kind = 'user_message' order by s.seq limit 1",
    )
    .bind(id)
    .bind(before_seq)
    .fetch_optional(&state.pool)
    .await?;
    let files = sqlx::query_scalar::<_, String>(
        "select path from (
            select location->>'path' as path, max(e.seq) as last
            from events e, jsonb_array_elements(case when jsonb_typeof(e.payload->'update'->'locations') = 'array' then e.payload->'update'->'locations' else '[]'::jsonb end) location
            where e.conversation_id = $1 and e.seq < $2 and e.kind = 'acp_update' and e.payload->'update'->>'sessionUpdate' = 'tool_call' and location->>'path' is not null
            group by 1 order by last desc limit $3
         ) touched order by path",
    )
    .bind(id)
    .bind(before_seq)
    .bind(FILES_LISTED)
    .fetch_all(&state.pool)
    .await?;
    let turns = recent_turns(state, id, before_seq, SUMMARY_MESSAGES).await?;
    let total = sqlx::query_scalar::<_, i64>("select count(*) from search_entries s join events e on e.conversation_id = s.conversation_id and e.seq = s.seq where s.conversation_id = $1 and s.seq < $2").bind(id).bind(before_seq).fetch_one(&state.pool).await?;
    let earlier = total - turns.len() as i64;
    let mut parts = vec![format!("This conversation continues from {from_label}. You are taking over the same conversation; the person sees one thread.")];
    let mut about = vec![format!("Title: {}", title.0)];

    if marker["agent"] != CHAT_AGENT {
        about.push(format!("Folder: {} on {}", title.1, title.2));
    }

    if let Some(goal) = goal {
        about.push(format!("Goal: {goal}"));
    }

    parts.push(about.join("\n"));

    if let Some(first) = first {
        parts.push(format!("The original request:\n{}", first.chars().take(SUMMARY_MESSAGE_LIMIT).collect::<String>()));
    }

    if !files.is_empty() {
        parts.push(format!("Files touched so far:\n{}", files.iter().map(|path| format!("- {path}")).collect::<Vec<_>>().join("\n")));
    }

    if !turns.is_empty() {
        parts.push(format!("The most recent messages, verbatim:\n\n{}", turns.join("\n\n")));
    }

    if earlier > 0 {
        parts.push(format!("There are {earlier} earlier messages. Call read_earlier_turns to read them."));
    }

    Ok(parts.join("\n\n"))
}
