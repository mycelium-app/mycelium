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
    format!("p.id, p.conversation_id, p.target_id, t.title as target_title, coalesce(n.name, '') as target_node, pr.name as target_project, p.message, p.reason, p.state, p.created_at, p.decided_at,
    s.title as source_title, t.archived_at is not null as target_archived,
    (select g.name from conversations x join groups g on g.id = {CONVERSATION_GROUP_SQL} where x.id = p.conversation_id) as source_group,
    (select g.name from conversations x join groups g on g.id = {CONVERSATION_GROUP_SQL} where x.id = p.target_id) as target_group")
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
    source_group: Option<String>,
    target_group: Option<String>,
}

impl ProposalRow {
    fn to_proto(self) -> MessageProposal {
        let state = match self.state.as_str() {
            "sent" => MessageProposalState::Sent,
            "declined" => MessageProposalState::Declined,
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

pub async fn propose(state: &AppState, conversation_id: Uuid, target: &str, message: &str, reason: &str) -> ApiResult<Result<MessageProposal, String>> {
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let candidates = sqlx::query_as::<_, (Uuid, bool)>("select id, archived_at is not null from conversations where owner = $1 and (id::text = lower($2) or title = $2) order by updated_at desc").bind(owner).bind(target).fetch_all(&state.pool).await?;
    let live: Vec<Uuid> = candidates.iter().filter(|(_, archived)| !archived).map(|(id, _)| *id).collect();

    let target_id = match (candidates.is_empty(), live.as_slice()) {
        (true, _) => return Ok(Err(format!("Nothing was proposed. None of the person's conversations is {target}. Look one up with find_conversations."))),
        (false, []) => return Ok(Err(format!("Nothing was proposed. The conversation {target} is archived."))),
        (false, [only]) => *only,
        (false, _) => return Ok(Err(format!("Nothing was proposed. More than one conversation is titled {target}. Use its id, from find_conversations."))),
    };

    if target_id == conversation_id {
        return Ok(Err("Nothing was proposed. That is this conversation. Choose another one.".to_string()));
    }

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
        let user = AuthUser { id: owner, email: String::new(), name: String::new(), session: None, scopes: None, expires_at: None };
        let Json(sent) = approve(State(state.clone()), user, Path(id)).await?;

        return Ok(Ok(sent));
    }

    Ok(Ok(proposal))
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<MessageProposal>>> {
    let rows = sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!("select {} from {PROPOSAL_JOINS} where p.owner = $1 order by (p.state = 'proposed') desc, p.created_at desc limit {LISTED}", proposal_columns())))
        .bind(user.id)
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(rows.into_iter().map(ProposalRow::to_proto).collect()))
}

pub async fn approve(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<MessageProposal>> {
    let found = find_owned(&state, id, user.id).await?;

    if found.state != "proposed" {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    if found.target_archived {
        return Err(ApiError::conflict("target_archived", "The conversation this was for has been archived. Decline it, or ask the agent to propose it again."));
    }

    let claimed = sqlx::query("update message_proposals set state = 'sent', decided_at = now() where id = $1 and state = 'proposed'").bind(id).execute(&state.pool).await?;

    if claimed.rows_affected() == 0 {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    let source = found.source_title.as_deref().map(|title| label(found.conversation_id.unwrap_or_default(), title)).unwrap_or_else(|| "another conversation".to_string());
    let text = format!("From {source}:\n\n{}", found.message);
    let queued = prompts::prompt(State(state.clone()), user.clone(), Path(found.target_id), Json(PromptRequest { text, ..Default::default() })).await;

    if let Err(cause) = queued {
        sqlx::query("update message_proposals set state = 'proposed', decided_at = null where id = $1").bind(id).execute(&state.pool).await?;

        return Err(cause);
    }

    Ok(Json(publish(&state, user.id, find_owned(&state, id, user.id).await?)))
}

pub async fn decline(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<MessageProposal>> {
    let found = find_owned(&state, id, user.id).await?;

    if found.state != "proposed" {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    sqlx::query("update message_proposals set state = 'declined', decided_at = now() where id = $1").bind(id).execute(&state.pool).await?;

    if let Some(conversation_id) = found.conversation_id {
        let text = format!("I declined sending your message to {}. Do not send it to that conversation another way.", label(found.target_id, &found.target_title));
        let _ = prompts::prompt(State(state.clone()), user.clone(), Path(conversation_id), Json(PromptRequest { text, ..Default::default() })).await;
    }

    Ok(Json(publish(&state, user.id, find_owned(&state, id, user.id).await?)))
}

pub async fn approve_and_connect(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<MessageProposal>> {
    user.require_full_access()?;
    let found = find_owned(&state, id, user.id).await?;

    if found.state != "proposed" {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    let source = found.conversation_id.ok_or_else(|| ApiError::conflict("source_removed", "The source conversation has been removed."))?;
    crate::conversation_connections::connect(&state, &user, source, found.target_id).await?;

    approve(State(state), user, Path(id)).await
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

    crate::group_connections::connect(&state, &user, source_group, target_group).await?;

    approve(State(state), user, Path(id)).await
}
