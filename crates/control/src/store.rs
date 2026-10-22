use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use mycelium_proto::api::NodeSummary;
use mycelium_proto::conversation::{AgentInfo, Conversation, ConversationStatus, Event, EventKind, LoginInfo, QueuedPrompt};
use mycelium_proto::stream::StreamMessage;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

pub type Tx<'a> = Transaction<'a, Postgres>;

pub const CONVERSATION_COLUMNS: &str = "id, owner, node_id, agent, cwd, title, status, acp_session_id, mode, model, created_at, updated_at, archived_at";

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
    pub status: String,
    pub acp_session_id: Option<String>,
    pub mode: Option<String>,
    pub model: Option<String>,
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
            status: self.parsed_status()?,
            acp_session_id: self.acp_session_id.clone(),
            mode: self.mode.clone(),
            model: self.model.clone(),
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

    Ok(seq.map(|seq| Event { conversation_id, seq, kind, payload: payload.clone() }))
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
    draining: bool,
    last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn node_summaries(state: &AppState, only: Option<Uuid>) -> ApiResult<Vec<NodeSummary>> {
    let nodes = sqlx::query_as::<_, NodeRow>(
        "select id, name, hostname, os, arch, version, draining, last_seen_at from nodes
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

    let mut summaries = Vec::new();

    for node in nodes {
        let mut node_agents = Vec::new();

        for (_, agent, version, available) in agents.iter().filter(|row| row.0 == node.id) {
            node_agents.push(AgentInfo { agent: parse_enum(agent)?, version: version.clone(), available: *available });
        }

        let mut node_logins = Vec::new();

        for (_, agent, account, method, login_state) in logins.iter().filter(|row| row.0 == node.id) {
            node_logins.push(LoginInfo { agent: parse_enum(agent)?, account: account.clone(), method: method.clone(), state: parse_enum(login_state)? });
        }

        summaries.push(NodeSummary {
            id: node.id,
            name: node.name,
            hostname: node.hostname,
            os: node.os,
            arch: node.arch,
            version: node.version,
            online: state.hub.is_online(node.id),
            draining: node.draining,
            last_seen_at: node.last_seen_at,
            agents: node_agents,
            logins: node_logins,
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
