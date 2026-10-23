use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::versions::newer;
use mycelium_proto::api::NodeSummary;
use mycelium_proto::conversation::{AgentId, AgentInfo, Conversation, ConversationStatus, Event, EventKind, LoginInfo, QueuedPrompt};
use mycelium_proto::stream::StreamMessage;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

pub type Tx<'a> = Transaction<'a, Postgres>;

pub const CONVERSATION_COLUMNS: &str = "id, owner, node_id, agent, cwd, title, title_user_set, status, acp_session_id, mode, model, account, options, parent_id, side_kind, created_at, updated_at, archived_at";

pub fn enum_text<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(text)) => text,
        _ => String::new(),
    }
}

pub fn parse_enum<T: DeserializeOwned>(text: &str) -> ApiResult<T> {
    serde_json::from_value(Value::String(text.to_string())).map_err(ApiError::internal)
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ConversationRow {
    pub id: Uuid,
    pub owner: Uuid,
    pub node_id: Uuid,
    pub agent: String,
    pub cwd: String,
    pub title: String,
    pub title_user_set: bool,
    pub status: String,
    pub acp_session_id: Option<String>,
    pub mode: Option<String>,
    pub model: Option<String>,
    pub account: Option<String>,
    pub options: Option<Value>,
    pub parent_id: Option<Uuid>,
    pub side_kind: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ConversationRow {
    pub fn to_proto(&self) -> ApiResult<Conversation> {
        Ok(Conversation {
            id: self.id,
            owner: self.owner,
            node_id: self.node_id,
            agent: parse_enum(&self.agent)?,
            cwd: self.cwd.clone(),
            title: self.title.clone(),
            title_user_set: self.title_user_set,
            status: self.parsed_status()?,
            acp_session_id: self.acp_session_id.clone(),
            mode: self.mode.clone(),
            model: self.model.clone(),
            account: self.account.clone(),
            options: self.options.clone(),
            parent_id: self.parent_id,
            side_kind: self.side_kind.as_deref().map(parse_enum).transpose()?,
            created_at: self.created_at,
            updated_at: self.updated_at,
            archived_at: self.archived_at,
        })
    }

    pub fn parsed_status(&self) -> ApiResult<ConversationStatus> {
        parse_enum(&self.status)
    }

    pub fn is_idle(&self) -> bool {
        self.acp_session_id.is_some() && matches!(self.status.as_str(), "review" | "done")
    }
}

pub async fn find_owned(pool: &PgPool, id: Uuid, owner: Uuid) -> ApiResult<ConversationRow> {
    let row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("select {CONVERSATION_COLUMNS} from conversations where id = $1 and owner = $2")))
        .bind(id)
        .bind(owner)
        .fetch_optional(pool)
        .await?;

    row.ok_or_else(|| ApiError::not_found("That conversation"))
}

pub async fn lock_owned(tx: &mut Tx<'_>, id: Uuid, owner: Uuid) -> ApiResult<ConversationRow> {
    let row = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("select {CONVERSATION_COLUMNS} from conversations where id = $1 and owner = $2 for update")))
        .bind(id)
        .bind(owner)
        .fetch_optional(&mut **tx)
        .await?;

    row.ok_or_else(|| ApiError::not_found("That conversation"))
}

pub async fn lock_on_node(tx: &mut Tx<'_>, id: Uuid, node_id: Uuid) -> sqlx::Result<Option<ConversationRow>> {
    sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("select {CONVERSATION_COLUMNS} from conversations where id = $1 and node_id = $2 for update")))
        .bind(id)
        .bind(node_id)
        .fetch_optional(&mut **tx)
        .await
}

pub async fn set_status(tx: &mut Tx<'_>, id: Uuid, status: ConversationStatus) -> sqlx::Result<ConversationRow> {
    sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("update conversations set status = $2, updated_at = now() where id = $1 returning {CONVERSATION_COLUMNS}")))
        .bind(id)
        .bind(enum_text(&status))
        .fetch_one(&mut **tx)
        .await
}

pub async fn append_event(tx: &mut Tx<'_>, conversation_id: Uuid, kind: EventKind, payload: &Value, node_seq: Option<i64>) -> sqlx::Result<Option<Event>> {
    let seq = sqlx::query_scalar::<_, i64>(
        "insert into events (conversation_id, seq, kind, payload, node_seq)
         select $1, coalesce(max(seq), 0) + 1, $2, $3, $4 from events where conversation_id = $1
         on conflict do nothing returning seq",
    )
    .bind(conversation_id)
    .bind(enum_text(&kind))
    .bind(payload)
    .bind(node_seq)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(seq) = seq else {
        return Ok(None);
    };

    match kind {
        EventKind::UserMessage => {
            let text = payload["text"].as_str().map(str::trim).filter(|text| !text.is_empty());

            if let Some(text) = text {
                sqlx::query("insert into search_entries (conversation_id, seq, text) values ($1, $2, $3) on conflict do nothing").bind(conversation_id).bind(seq).bind(text).execute(&mut **tx).await?;
            }
        }
        EventKind::TurnEnd => {
            sqlx::query(
                "insert into search_entries (conversation_id, seq, text)
                 select $1, $2, string_agg(payload->'update'->'content'->>'text', '' order by seq) from events
                 where conversation_id = $1 and kind = 'acp_update' and payload->'update'->>'sessionUpdate' = 'agent_message_chunk'
                   and seq < $2 and seq > coalesce((select max(seq) from events where conversation_id = $1 and kind = 'user_message' and seq < $2), 0)
                 having string_agg(payload->'update'->'content'->>'text', '') <> ''
                 on conflict do nothing",
            )
            .bind(conversation_id)
            .bind(seq)
            .execute(&mut **tx)
            .await?;
        }
        _ => {}
    }

    Ok(Some(Event { conversation_id, seq, kind, payload: payload.clone() }))
}

pub async fn highest_node_seq(pool: &PgPool, conversation_id: Uuid) -> sqlx::Result<i64> {
    sqlx::query_scalar::<_, i64>("select coalesce(max(node_seq), 0) from events where conversation_id = $1").bind(conversation_id).fetch_one(pool).await
}

pub async fn queue_of(pool: &PgPool, conversation_id: Uuid) -> sqlx::Result<Vec<QueuedPrompt>> {
    let rows = sqlx::query_as::<_, (Uuid, i32, String, chrono::DateTime<chrono::Utc>)>(
        "select id, position, text, created_at from queued_prompts where conversation_id = $1 order by position",
    )
    .bind(conversation_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|(id, position, text, created_at)| QueuedPrompt { id, conversation_id, position, text, created_at }).collect())
}

pub async fn publish_queue(state: &AppState, owner: Uuid, conversation_id: Uuid) -> ApiResult<()> {
    let queue = queue_of(&state.pool, conversation_id).await?;
    state.publish(Some(owner), StreamMessage::Queue { conversation_id, queue });

    Ok(())
}

#[derive(sqlx::FromRow)]
struct NodeRow {
    id: Uuid,
    name: String,
    hostname: String,
    os: String,
    arch: String,
    version: String,
    address: String,
    draining: bool,
    last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
    icon: Option<String>,
    instructions_revision: Option<String>,
    instructions_unmanaged: Vec<String>,
}

pub async fn node_summaries(state: &AppState, only: Option<Uuid>) -> ApiResult<Vec<NodeSummary>> {
    let nodes = sqlx::query_as::<_, NodeRow>(
        "select id, name, hostname, os, arch, version, address, draining, last_seen_at, icon, instructions_revision, instructions_unmanaged from nodes
         where revoked_at is null and ($1::uuid is null or id = $1) order by name",
    )
    .bind(only)
    .fetch_all(&state.pool)
    .await?;
    let ids: Vec<Uuid> = nodes.iter().map(|node| node.id).collect();

    let agents = sqlx::query_as::<_, (Uuid, String, Option<String>, bool)>("select node_id, agent, version, available from agents where node_id = any($1) order by agent")
        .bind(&ids)
        .fetch_all(&state.pool)
        .await?;
    let logins = sqlx::query_as::<_, (Uuid, String, Option<String>, String, String)>(
        "select node_id, agent, account, method, state from logins where node_id = any($1) order by agent",
    )
    .bind(&ids)
    .fetch_all(&state.pool)
    .await?;

    let current_revision = crate::instructions::current_revision(state).await?;
    let mut summaries = Vec::new();

    for node in nodes {
        let sessions = state.hub.sessions_of(node.id);
        let older = match &current_revision {
            Some(revision) => sqlx::query_scalar::<_, i64>("select count(*) from conversations where id = any($1) and instructions_revision is distinct from $2").bind(&sessions).bind(revision).fetch_one(&state.pool).await?,
            None => 0,
        };

        let mut node_agents = Vec::new();

        for (_, agent, version, available) in agents.iter().filter(|row| row.0 == node.id) {
            let agent_id: AgentId = parse_enum(agent)?;
            let update_available = version.as_deref().and_then(|installed| newer(installed, state.latest.get(agent)));
            node_agents.push(AgentInfo { agent: agent_id, version: version.clone(), available: *available, update_available });
        }

        let mut node_logins = Vec::new();

        for (_, agent, account, method, login_state) in logins.iter().filter(|row| row.0 == node.id) {
            node_logins.push(LoginInfo { agent: parse_enum(agent)?, account: account.clone(), method: method.clone(), state: parse_enum(login_state)? });
        }

        summaries.push(NodeSummary {
            id: node.id,
            name: node.name,
            icon: node.icon,
            hostname: node.hostname,
            os: node.os,
            arch: node.arch,
            update_available: newer(&node.version, state.latest.get("mycelium")),
            address: node.address,
            version: node.version,
            online: state.hub.is_online(node.id),
            draining: node.draining,
            last_seen_at: node.last_seen_at,
            agents: node_agents,
            logins: node_logins,
            sessions,
            instructions_revision: node.instructions_revision,
            instructions_unmanaged: node.instructions_unmanaged,
            sessions_on_older_instructions: older as u32,
        });
    }

    Ok(summaries)
}

pub async fn publish_node(state: &AppState, node_id: Uuid) -> ApiResult<()> {
    for summary in node_summaries(state, Some(node_id)).await? {
        state.publish(None, StreamMessage::Node(summary));
    }

    Ok(())
}
