use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::prompts;
use crate::state::AppState;
use crate::store::CONVERSATION_GROUP_SQL;
use axum::Json;
use axum::extract::{Path, State};
use mycelium_proto::api::{MessageProposal, MessageProposalState, PromptRequest};
use mycelium_proto::stream::StreamMessage;
use uuid::Uuid;

const LISTED: i64 = 100;

fn proposal_columns() -> String {
    format!("p.id, p.conversation_id, p.target_id, coalesce((select a.name from assistants a where a.conversation_id = t.id), t.title) as target_title, coalesce(n.name, '') as target_node, pr.name as target_project, p.message, p.reason, p.state, p.created_at, p.decided_at,
    coalesce((select a.name from assistants a where a.conversation_id = s.id), s.title) as source_title, t.archived_at is not null as target_archived, coalesce(t.managed_by = p.conversation_id, false) as source_manages_target,
    (select g.name from groups g where g.id = (select {CONVERSATION_GROUP_SQL} from conversations where id = p.conversation_id)) as source_group,
    (select g.name from groups g where g.id = (select {CONVERSATION_GROUP_SQL} from conversations where id = p.target_id)) as target_group")
}

const PROPOSAL_JOINS: &str = "message_proposals p join conversations t on t.id = p.target_id left join nodes n on n.id = t.node_id left join projects pr on pr.id = t.project_id left join conversations s on s.id = p.conversation_id";

#[derive(sqlx::FromRow)]
struct ProposalRow {
    id: Uuid,
    conversation_id: Option<Uuid>,
    target_id: Uuid,
    target_title: String,
    target_node: String,
    target_project: Option<String>,
    message: String,
    reason: String,
    state: String,
    created_at: chrono::DateTime<chrono::Utc>,
    decided_at: Option<chrono::DateTime<chrono::Utc>>,
    source_title: Option<String>,
    target_archived: bool,
    source_manages_target: bool,
    source_group: Option<String>,
    target_group: Option<String>,
}

impl ProposalRow {
    fn to_proto(self) -> MessageProposal {
        let state = match self.state.as_str() {
            "sent" => MessageProposalState::Sent,
            "declined" => MessageProposalState::Declined,
            "withdrawn" => MessageProposalState::Withdrawn,
            "superseded" => MessageProposalState::Superseded,
            _ => MessageProposalState::Proposed,
        };

        MessageProposal {
            id: self.id,
            conversation_id: self.conversation_id,
            target_id: self.target_id,
            target_title: label(self.target_id, &self.target_title),
            target_node: self.target_node,
            target_project: self.target_project,
            message: self.message,
            reason: self.reason,
            state,
            created_at: self.created_at,
            decided_at: self.decided_at,
            source_group: self.source_group,
            target_group: self.target_group,
        }
    }
}

pub fn label(id: Uuid, title: &str) -> String {
    if title.trim().is_empty() { id.to_string() } else { title.to_string() }
}

async fn find_owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<ProposalRow> {
    let row = sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!("select {} from {PROPOSAL_JOINS} where p.id = $1 and p.owner = $2", proposal_columns()))).bind(id).bind(owner).fetch_optional(&state.pool).await?;

    row.ok_or_else(|| ApiError::not_found("That proposal"))
}

fn publish(state: &AppState, owner: Uuid, row: ProposalRow) -> MessageProposal {
    let proposal = row.to_proto();
    state.publish(Some(owner), StreamMessage::MessageProposal(proposal.clone()));

    proposal
}

struct SessionlessAssistant {
    id: Uuid,
    name: String,
    group_id: Option<Uuid>,
}

enum Resolved {
    Conversation(Uuid),
    SessionlessAssistant(SessionlessAssistant),
}

pub async fn resolve(state: &AppState, owner: Uuid, target: &str) -> ApiResult<Result<Uuid, String>> {
    Ok(match resolve_for_sender(state, owner, target, None).await? {
        Ok(Resolved::Conversation(id)) => Ok(id),
        Ok(Resolved::SessionlessAssistant(SessionlessAssistant { name, .. })) => Err(format!("The assistant {name} has no chat yet. Send it a message with send_to_conversation to start one.")),
        Err(reason) => Err(reason),
    })
}

async fn resolve_for_sender(state: &AppState, owner: Uuid, target: &str, sender: Option<Uuid>) -> ApiResult<Result<Resolved, String>> {
    let candidates = sqlx::query_as::<_, (Uuid, bool, bool)>("select id, archived_at is not null, coalesce(managed_by = $3, false) from conversations where owner = $1 and (id::text = lower($2) or title = $2) order by updated_at desc").bind(owner).bind(target).bind(sender).fetch_all(&state.pool).await?;
    let live: Vec<Uuid> = candidates.iter().filter(|(_, archived, _)| !archived).map(|(id, ..)| *id).collect();
    let usable = if live.is_empty() {
        candidates.iter().filter(|(_, archived, managed)| *archived && *managed).map(|(id, ..)| *id).collect()
    } else {
        live
    };

    if candidates.is_empty() {
        return resolve_assistant(state, owner, target).await;
    }

    Ok(match usable.as_slice() {
        [] => Err(format!("The conversation {target} is archived.")),
        [only] => Ok(Resolved::Conversation(*only)),
        _ => Err(format!("More than one conversation is titled {target}. Use its id, from find_conversations.")),
    })
}

async fn resolve_assistant(state: &AppState, owner: Uuid, target: &str) -> ApiResult<Result<Resolved, String>> {
    let found = sqlx::query_as::<_, (Uuid, String, Option<Uuid>, Option<Uuid>, bool)>("select id, name, conversation_id, group_id, archived_at is not null from assistants where owner = $1 and (id::text = lower($2) or lower(name) = lower($2))")
        .bind(owner)
        .bind(target)
        .fetch_all(&state.pool)
        .await?;

    Ok(match found.as_slice() {
        [] => Err(format!("None of the person's conversations or assistants is {target}. Look a conversation up with find_conversations, or an assistant's id with assistant_list.")),
        [(_, name, _, _, true)] => Err(format!("The assistant {name} is archived.")),
        [(_, _, Some(conversation_id), ..)] => Ok(Resolved::Conversation(*conversation_id)),
        [(id, name, None, group_id, false)] => Ok(Resolved::SessionlessAssistant(SessionlessAssistant { id: *id, name: name.clone(), group_id: *group_id })),
        _ => Err(format!("More than one assistant is named {target}. Use its id, from assistant_list.")),
    })
}

pub async fn propose(state: &AppState, conversation_id: Uuid, target: &str, message: &str, reason: &str) -> ApiResult<Result<(MessageProposal, Option<prompts::Sent>, u64), String>> {
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let target_id = match resolve_for_sender(state, owner, target, Some(conversation_id)).await? {
        Ok(Resolved::Conversation(target_id)) => target_id,
        Ok(Resolved::SessionlessAssistant(assistant)) => return start_assistant_chat(state, owner, conversation_id, assistant, message, reason).await,
        Err(reason) => return Ok(Err(format!("Nothing was proposed. {reason}"))),
    };

    if target_id == conversation_id {
        return Ok(Err("Nothing was proposed. That is this conversation. Choose another one.".to_string()));
    }

    let superseded = sqlx::query("update message_proposals set state = 'superseded', decided_at = now() where conversation_id = $1 and target_id = $2 and state = 'proposed'").bind(conversation_id).bind(target_id).execute(&state.pool).await?.rows_affected();
    let id = sqlx::query_scalar::<_, Uuid>("insert into message_proposals (owner, conversation_id, target_id, message, reason) values ($1, $2, $3, $4, $5) returning id")
        .bind(owner)
        .bind(conversation_id)
        .bind(target_id)
        .bind(crate::store::text_without_nul(message))
        .bind(crate::store::text_without_nul(reason))
        .fetch_one(&state.pool)
        .await?;

    let proposal = publish(state, owner, find_owned(state, id, owner).await?);

    if crate::conversation_connections::active(state, owner, conversation_id, target_id).await? || crate::group_connections::connected(state, owner, conversation_id, target_id).await? {
        let (sent, delivery) = approve_and_send(state, owner, id).await?;

        return Ok(Ok((sent, Some(delivery), superseded)));
    }

    Ok(Ok((proposal, None, superseded)))
}

async fn start_assistant_chat(state: &AppState, owner: Uuid, conversation_id: Uuid, assistant: SessionlessAssistant, message: &str, reason: &str) -> ApiResult<Result<(MessageProposal, Option<prompts::Sent>, u64), String>> {
    let SessionlessAssistant { id: assistant_id, name, group_id } = assistant;
    let own_group = crate::group_connections::group_of(state, conversation_id).await?;
    let reachable = match (own_group, group_id) {
        (Some(own), Some(target)) => own == target || crate::group_connections::active(state, owner, own, target).await?,
        _ => false,
    };

    if !reachable {
        return Ok(Err(format!("Nothing was proposed. The assistant {name} has no chat to hold a message for the person to approve, and its group is not connected to this conversation's. Ask the person to connect the groups in Settings, Agent connections, or to start a chat with {name}.")));
    }

    let source = sqlx::query_scalar::<_, String>("select coalesce((select a.name from assistants a where a.conversation_id = s.id), s.title) from conversations s where s.id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let text = format!("From {}:\n\n{message}", label(conversation_id, &source));
    let woken = crate::assistants::wake(state, owner, assistant_id, PromptRequest { text, ..Default::default() }, &crate::assistants::StartOverrides::default()).await;
    let conversation = match woken {
        Ok(conversation) => conversation,
        Err(error) => return Ok(Err(format!("Nothing was proposed. {}", error.message()))),
    };
    let id = sqlx::query_scalar::<_, Uuid>("insert into message_proposals (owner, conversation_id, target_id, message, reason, state, decided_at) values ($1, $2, $3, $4, $5, 'sent', now()) returning id")
        .bind(owner)
        .bind(conversation_id)
        .bind(conversation.id)
        .bind(crate::store::text_without_nul(message))
        .bind(crate::store::text_without_nul(reason))
        .fetch_one(&state.pool)
        .await?;

    Ok(Ok((publish(state, owner, find_owned(state, id, owner).await?), Some(prompts::Sent::Delivered), 0)))
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<MessageProposal>>> {
    let rows = sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!("select {} from {PROPOSAL_JOINS} where p.owner = $1 order by (p.state = 'proposed') desc, p.created_at desc limit {LISTED}", proposal_columns())))
        .bind(user.id)
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(rows.into_iter().map(ProposalRow::to_proto).collect()))
}

pub async fn approve(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<MessageProposal>> {
    Ok(Json(approve_for(&state, user.id, id).await?))
}

pub async fn approve_for(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<MessageProposal> {
    let (proposal, _) = approve_and_send(state, owner, id).await?;

    Ok(proposal)
}

async fn approve_and_send(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<(MessageProposal, prompts::Sent)> {
    let found = find_owned(state, id, owner).await?;

    if found.state != "proposed" {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    if found.target_archived && !found.source_manages_target {
        return Err(ApiError::conflict("target_archived", "The conversation this was for has been archived. Decline it, or ask the agent to propose it again."));
    }

    let claimed = sqlx::query("update message_proposals set state = 'sent', decided_at = now() where id = $1 and state = 'proposed'").bind(id).execute(&state.pool).await?;

    if claimed.rows_affected() == 0 {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    let source = found.source_title.as_deref().map(|title| label(found.conversation_id.unwrap_or_default(), title)).unwrap_or_else(|| "another conversation".to_string());
    let text = format!("From {source}:\n\n{}", found.message);
    let manager = found.conversation_id.filter(|_| found.target_archived);
    let queued = prompts::send_reopening(state, owner, found.target_id, PromptRequest { text, ..Default::default() }, None, manager).await;

    if queued.is_ok()
        && let Some(source_id) = found.conversation_id
    {
        crate::conversation_connections::renew(state, owner, source_id, found.target_id).await?;
    }

    let sent = match queued {
        Ok(sent) => sent,
        Err(cause) => {
            sqlx::query("update message_proposals set state = 'proposed', decided_at = null where id = $1").bind(id).execute(&state.pool).await?;

            return Err(cause);
        }
    };

    Ok((publish(state, owner, find_owned(state, id, owner).await?), sent))
}

pub async fn decline(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<MessageProposal>> {
    Ok(Json(decline_for(&state, user.id, id).await?))
}

pub async fn decline_for(state: &AppState, owner: Uuid, id: Uuid) -> ApiResult<MessageProposal> {
    let found = find_owned(state, id, owner).await?;

    if found.state != "proposed" {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    sqlx::query("update message_proposals set state = 'declined', decided_at = now() where id = $1").bind(id).execute(&state.pool).await?;

    if let Some(conversation_id) = found.conversation_id {
        let text = format!("I declined sending your message to {}. Do not send it to that conversation another way.", label(found.target_id, &found.target_title));
        prompts::tell_agent(state, owner, conversation_id, text).await;
    }

    Ok(publish(state, owner, find_owned(state, id, owner).await?))
}

pub async fn withdraw_for(state: &AppState, conversation_id: Uuid, id: Uuid) -> ApiResult<Result<MessageProposal, String>> {
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let found = sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!("select {} from {PROPOSAL_JOINS} where p.id = $1 and p.owner = $2 and p.conversation_id = $3", proposal_columns()))).bind(id).bind(owner).bind(conversation_id).fetch_optional(&state.pool).await?;

    let Some(found) = found else {
        return Ok(Err("Nothing was withdrawn. This conversation has no message proposal with that id.".to_string()));
    };

    let withdrawn = sqlx::query("update message_proposals set state = 'withdrawn', decided_at = now() where id = $1 and state = 'proposed'").bind(id).execute(&state.pool).await?;

    if withdrawn.rows_affected() == 0 {
        return Ok(Err(format!("Nothing was withdrawn. The message to {} has already been decided.", label(found.target_id, &found.target_title))));
    }

    Ok(Ok(publish(state, owner, find_owned(state, id, owner).await?)))
}

pub async fn approve_and_connect(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<MessageProposal>> {
    user.require_full_access()?;
    let found = find_owned(&state, id, user.id).await?;

    if found.state != "proposed" {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    let source = found.conversation_id.ok_or_else(|| ApiError::conflict("source_removed", "The source conversation has been removed."))?;
    crate::conversation_connections::connect(&state, user.id, source, found.target_id).await?;

    Ok(Json(approve_for(&state, user.id, id).await?))
}

pub async fn approve_and_connect_groups(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<MessageProposal>> {
    user.require_full_access()?;
    let found = find_owned(&state, id, user.id).await?;

    if found.state != "proposed" {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    let source = found.conversation_id.ok_or_else(|| ApiError::conflict("source_removed", "The source conversation has been removed."))?;
    let groups = (crate::group_connections::group_of(&state, source).await?, crate::group_connections::group_of(&state, found.target_id).await?);

    let (Some(source_group), Some(target_group)) = groups else {
        return Err(ApiError::conflict("connection_no_group", "Both conversations need a group to connect their groups."));
    };

    if source_group == target_group {
        return Err(ApiError::conflict("connection_same_group", "Both conversations are already in the same group, so there is nothing to connect. Approve the message without connecting groups."));
    }

    let reverse_active = crate::group_connections::active(&state, user.id, target_group, source_group).await?;
    crate::group_connections::connect(&state, user.id, source_group, target_group, reverse_active).await?;

    Ok(Json(approve_for(&state, user.id, id).await?))
}
