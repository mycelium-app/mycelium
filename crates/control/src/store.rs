use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::versions::newer;
use mycelium_proto::api::{HarnessModel, HarnessModels, NodeSummary};
use mycelium_proto::conversation::{AgentId, AgentInfo, Attachment, Conversation, ConversationStatus, Event, EventKind, LoginInfo, QueuedPrompt, ResetCredits, UsageWindow};
use mycelium_proto::stream::StreamMessage;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

pub type Tx<'a> = Transaction<'a, Postgres>;

macro_rules! conversation_group_sql {
    () => {
        "coalesce((select a.group_id from assistants a where a.id = assistant_id), conversation_group(cwd, node_id, project_id))"
    };
}

pub const CONVERSATION_GROUP_SQL: &str = conversation_group_sql!();
pub const CONVERSATION_COLUMNS: &str = concat!("id, owner, node_id, agent, cwd, title, title_user_set, status, agent_active, acp_session_id, mode, model, account, options, parent_id, side_kind, imported_from, provider_id, project_id, ", conversation_group_sql!(), " as group_id, created_at, updated_at, archived_at, pinned_at, resume_at, branch, pull_request_label, pull_request_url, assistant_id, teach_me, routing");
pub const TURN_HAS_ENDED: &str = "coalesce((select max(seq) from events where conversation_id = $1 and kind = 'turn_end'), 0) >= coalesce((select max(seq) from events where conversation_id = $1 and kind = 'user_message'), 0)";

pub const WORKFLOW_AGENT_QUIET_SECONDS: i32 = 15 * 60;

pub fn turn_has_ended_of(conversation: &str) -> String {
    TURN_HAS_ENDED.replace("$1", conversation)
}

pub fn has_live_workflow_agent(conversation: &str) -> String {
    format!(
        "(exists (
             select 1 from events started
             left join lateral (
                 select max(t.updated_at) as at from transcripts t join conversations owner_of on owner_of.id = started.conversation_id
                 where t.node_id = owner_of.node_id and owner_of.acp_session_id is not null
                   and position('/' || owner_of.acp_session_id || '/' in t.path) > 0
                   and right(t.path, length('/agent-' || (started.payload->'line'->>'agentId') || '.jsonl')) = '/agent-' || (started.payload->'line'->>'agentId') || '.jsonl'
             ) mirrored on true
             where started.conversation_id = {conversation} and started.kind = 'workflow' and started.payload->'line'->>'type' = 'started'
               and started.payload->'line'->>'agentId' is not null
               and not exists (
                   select 1 from events finished
                   where finished.conversation_id = started.conversation_id and finished.kind = 'workflow'
                     and finished.payload->'line'->>'agentId' = started.payload->'line'->>'agentId'
                     and finished.payload->'line'->>'type' in ('result', 'failed')
               )
               and coalesce(mirrored.at, started.created_at) > now() - make_interval(secs => {WORKFLOW_AGENT_QUIET_SECONDS})
         ) or (
             select count(*) filter (where payload->'line'->>'type' = 'started' and created_at > now() - make_interval(secs => {WORKFLOW_AGENT_QUIET_SECONDS}))
                  - count(*) filter (where payload->'line'->>'type' in ('result', 'failed'))
             from events where conversation_id = {conversation} and kind = 'workflow' and payload->'line'->>'agentId' is null
         ) > 0)"
    )
}

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
    pub agent_active: bool,
    pub acp_session_id: Option<String>,
    pub mode: Option<String>,
    pub model: Option<String>,
    pub account: Option<String>,
    pub options: Option<Value>,
    pub parent_id: Option<Uuid>,
    pub side_kind: Option<String>,
    pub imported_from: Option<String>,
    pub provider_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub group_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
    pub pinned_at: Option<chrono::DateTime<chrono::Utc>>,
    pub resume_at: Option<chrono::DateTime<chrono::Utc>>,
    pub branch: Option<String>,
    pub pull_request_label: Option<String>,
    pub pull_request_url: Option<String>,
    pub assistant_id: Option<Uuid>,
    pub teach_me: bool,
    pub routing: Option<String>,
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
            agent_active: self.agent_active,
            acp_session_id: self.acp_session_id.clone(),
            mode: self.mode.clone(),
            model: self.model.clone(),
            account: self.account.clone(),
            options: self.options.clone(),
            parent_id: self.parent_id,
            side_kind: self.side_kind.as_deref().map(parse_enum).transpose()?,
            agent_id: self.imported_from.as_deref().and_then(|path| path.rsplit('/').next()?.strip_prefix("agent-")?.strip_suffix(".jsonl")).filter(|_| self.side_kind.as_deref() == Some("subagent")).map(str::to_string),
            provider_id: self.provider_id,
            project_id: self.project_id,
            group_id: self.group_id,
            created_at: self.created_at,
            updated_at: self.updated_at,
            archived_at: self.archived_at,
            pinned_at: self.pinned_at,
            resume_at: self.resume_at,
            branch: self.branch.clone(),
            pull_request_label: self.pull_request_label.clone(),
            pull_request_url: self.pull_request_url.clone(),
            assistant_id: self.assistant_id,
            teach_me: self.teach_me,
            routing: self.routing.as_deref().and_then(mycelium_proto::routing::Routing::parse),
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
    sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("update conversations set status = $2, agent_active = agent_active and $2 not in ('review', 'failed', 'done'), updated_at = now() where id = $1 returning {CONVERSATION_COLUMNS}")))
        .bind(id)
        .bind(enum_text(&status))
        .fetch_one(&mut **tx)
        .await
}

pub fn without_nul(value: &mut Value) {
    match value {
        Value::String(text) if text.contains('\0') => *text = text.replace('\0', "\u{FFFD}"),
        Value::Array(items) => items.iter_mut().for_each(without_nul),
        Value::Object(fields) => {
            if fields.keys().any(|key| key.contains('\0')) {
                let entries = std::mem::take(fields);
                fields.extend(entries.into_iter().map(|(key, value)| (key.replace('\0', "\u{FFFD}"), value)));
            }

            fields.values_mut().for_each(without_nul);
        }
        _ => {}
    }
}

pub fn text_without_nul(text: &str) -> String {
    text.replace('\0', "\u{FFFD}")
}

fn has_nul(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains('\0'),
        Value::Array(items) => items.iter().any(has_nul),
        Value::Object(fields) => fields.iter().any(|(key, value)| key.contains('\0') || has_nul(value)),
        _ => false,
    }
}

pub const MIN_REDACTED_SECRET: i32 = 4;

fn redact(value: &mut Value, secrets: &[String]) {
    match value {
        Value::String(text) => {
            for secret in secrets {
                if text.contains(secret.as_str()) {
                    *text = text.replace(secret.as_str(), "[redacted]");
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| redact(item, secrets)),
        Value::Object(fields) => fields.values_mut().for_each(|item| redact(item, secrets)),
        _ => {}
    }
}

pub async fn append_event(tx: &mut Tx<'_>, conversation_id: Uuid, kind: EventKind, payload: &Value, node_seq: Option<i64>) -> sqlx::Result<Option<Event>> {
    let cleaned;
    let payload = if has_nul(payload) {
        let mut copy = payload.clone();
        without_nul(&mut copy);
        cleaned = copy;
        &cleaned
    } else {
        payload
    };
    let secrets = sqlx::query_scalar::<_, String>(
        "select s.value from conversations c join assistant_secrets s on s.assistant_id = c.assistant_id where c.id = $1 and s.value is not null and length(s.value) >= $2",
    )
    .bind(conversation_id)
    .bind(MIN_REDACTED_SECRET)
    .fetch_all(&mut **tx)
    .await?;
    let redacted;
    let payload = if secrets.is_empty() {
        payload
    } else {
        let mut copy = payload.clone();
        redact(&mut copy, &secrets);
        redacted = copy;
        &redacted
    };

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

pub async fn highest_node_seqs(pool: &PgPool, conversation_ids: &[Uuid]) -> sqlx::Result<std::collections::HashMap<Uuid, i64>> {
    let rows = sqlx::query_as::<_, (Uuid, i64)>("select conversation_id, coalesce(max(node_seq), 0) from events where conversation_id = any($1) group by conversation_id").bind(conversation_ids).fetch_all(pool).await?;

    Ok(rows.into_iter().collect())
}

pub async fn queue_of(pool: &PgPool, conversation_id: Uuid) -> sqlx::Result<Vec<QueuedPrompt>> {
    let rows = sqlx::query_as::<_, (Uuid, i32, String, sqlx::types::Json<Vec<Attachment>>, chrono::DateTime<chrono::Utc>)>(
        "select id, position, text, attachments, created_at from queued_prompts where conversation_id = $1 order by position",
    )
    .bind(conversation_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|(id, position, text, attachments, created_at)| QueuedPrompt { id, conversation_id, position, text, attachments: attachments.0, created_at }).collect())
}

pub async fn publish_queue(state: &AppState, owner: Uuid, conversation_id: Uuid) -> ApiResult<()> {
    let queue = queue_of(&state.pool, conversation_id).await?;
    state.publish(Some(owner), StreamMessage::Queue { conversation_id, queue });

    Ok(())
}

pub async fn enqueue(tx: &mut sqlx::PgConnection, conversation_id: Uuid, text: &str, attachments: &[Attachment]) -> sqlx::Result<()> {
    sqlx::query("insert into queued_prompts (conversation_id, position, text, attachments) select $1, coalesce(max(position), 0) + 1, $2, $3 from queued_prompts where conversation_id = $1")
        .bind(conversation_id)
        .bind(text)
        .bind(sqlx::types::Json(attachments))
        .execute(tx)
        .await?;

    Ok(())
}

pub async fn merge_queue(tx: &mut sqlx::PgConnection, conversation_id: Uuid) -> sqlx::Result<Option<Uuid>> {
    let rows = sqlx::query_as::<_, (Uuid, String, sqlx::types::Json<Vec<Attachment>>)>("select id, text, attachments from queued_prompts where conversation_id = $1 order by position for update")
        .bind(conversation_id)
        .fetch_all(&mut *tx)
        .await?;

    let Some((first_id, _, _)) = rows.first() else {
        return Ok(None);
    };

    if rows.len() > 1 {
        let text = rows.iter().map(|(_, text, _)| text.as_str()).filter(|text| !text.is_empty()).collect::<Vec<_>>().join("\n\n");
        let attachments: Vec<Attachment> = rows.iter().flat_map(|(_, _, attachments)| attachments.0.clone()).collect();

        sqlx::query("update queued_prompts set text = $2, attachments = $3, origin = (select origin from queued_prompts where conversation_id = $4 and origin is not null order by position limit 1) where id = $1")
            .bind(first_id)
            .bind(text)
            .bind(sqlx::types::Json(attachments))
            .bind(conversation_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("delete from queued_prompts where conversation_id = $1 and id <> $2").bind(conversation_id).bind(first_id).execute(&mut *tx).await?;
    }

    Ok(Some(*first_id))
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
    colour: Option<String>,
    instructions_revision: Option<String>,
    instructions_unmanaged: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct LoginRow {
    node_id: Uuid,
    agent: String,
    account: Option<String>,
    method: String,
    state: String,
    usage_left: Option<i16>,
    usage_windows: sqlx::types::Json<Vec<UsageWindow>>,
    usage_read_at: Option<chrono::DateTime<chrono::Utc>>,
    reset_credits: Option<sqlx::types::Json<ResetCredits>>,
}

#[derive(sqlx::FromRow)]
struct ReportedModelsRow {
    node_id: Uuid,
    agent: String,
    account: String,
    models: sqlx::types::Json<Vec<HarnessModel>>,
    default_model: Option<String>,
    reported_at: chrono::DateTime<chrono::Utc>,
}

pub async fn node_summaries(state: &AppState, only: Option<Uuid>) -> ApiResult<Vec<NodeSummary>> {
    let nodes = sqlx::query_as::<_, NodeRow>(
        "select id, name, hostname, os, arch, version, address, draining, last_seen_at, icon, colour, instructions_revision, instructions_unmanaged from nodes
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
    let logins = sqlx::query_as::<_, LoginRow>(
        "select node_id, agent, account, method, state, usage_left, usage_windows, usage_read_at, reset_credits from logins where node_id = any($1) order by agent",
    )
    .bind(&ids)
    .fetch_all(&state.pool)
    .await?;
    let usage_history = crate::account_usage::project_windows(state, &ids).await?;
    let reported_models = sqlx::query_as::<_, ReportedModelsRow>(
        "select node_id, agent, account, models, default_model, reported_at from harness_models where node_id = any($1) order by agent, account",
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

        for login in logins.iter().filter(|row| row.node_id == node.id) {
            let history = usage_history.iter().find(|group| group.0 == node.id && group.1 == login.agent && group.2 == login.account.clone().unwrap_or_default()).map(|group| group.3.as_slice()).unwrap_or_default();
            let windows = crate::account_usage::with_projections(login.usage_windows.0.clone(), history);
            node_logins.push(LoginInfo { agent: parse_enum(&login.agent)?, account: login.account.clone(), method: login.method.clone(), state: parse_enum(&login.state)?, usage_left: login.usage_left.and_then(|left| u8::try_from(left).ok()), windows, usage_read_at: login.usage_read_at, reset_credits: login.reset_credits.as_ref().map(|credits| credits.0.clone()) });
        }

        let mut harness_models = Vec::new();

        for reported in reported_models.iter().filter(|row| row.node_id == node.id) {
            harness_models.push(HarnessModels { agent: parse_enum(&reported.agent)?, account: Some(reported.account.clone()).filter(|account| !account.is_empty()), models: reported.models.0.clone(), default_model: reported.default_model.clone(), reported_at: reported.reported_at });
        }

        summaries.push(NodeSummary {
            id: node.id,
            name: node.name,
            icon: node.icon,
            colour: node.colour,
            hostname: node.hostname,
            os: node.os,
            arch: node.arch,
            update_available: newer(&node.version, state.latest.get("mycelium")),
            update_message: crate::node_updates::message_of(node.id),
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
            harness_models,
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
