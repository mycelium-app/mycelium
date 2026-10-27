use crate::assistants;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::instructions::{self, find_layer};
use crate::prompts;
use crate::state::AppState;
use crate::store::CONVERSATION_GROUP_SQL;
use axum::Json;
use axum::extract::{Path, State};
use mycelium_proto::api::{DeclineInstructionProposalRequest, InstructionProposal, InstructionProposalState, PromptRequest};
use mycelium_proto::stream::StreamMessage;
use uuid::Uuid;

const LISTED: i64 = 100;
pub const NO_REVISION: &str = "none";

const PROPOSAL_COLUMNS: &str = "p.id, p.conversation_id, c.title as conversation_title, p.layer, p.layer_name, p.base_revision, p.base_text, p.text, p.reason, p.state, p.note, p.created_at, p.decided_at,
    coalesce((select v.id from instruction_versions v where v.layer = p.layer order by v.seq desc limit 1), '') as current_revision";

#[derive(sqlx::FromRow)]
struct ProposalRow {
    id: Uuid,
    conversation_id: Option<Uuid>,
    conversation_title: Option<String>,
    layer: String,
    layer_name: String,
    base_revision: String,
    base_text: String,
    text: String,
    reason: String,
    state: String,
    note: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    decided_at: Option<chrono::DateTime<chrono::Utc>>,
    current_revision: String,
}

impl ProposalRow {
    fn to_proto(self) -> InstructionProposal {
        let state = match self.state.as_str() {
            "approved" => InstructionProposalState::Approved,
            "declined" => InstructionProposalState::Declined,
            "superseded" => InstructionProposalState::Superseded,
            _ => InstructionProposalState::Proposed,
        };

        InstructionProposal {
            id: self.id,
            conversation_id: self.conversation_id,
            conversation_title: self.conversation_title,
            layer: self.layer,
            layer_name: self.layer_name,
            out_of_date: state == InstructionProposalState::Proposed && self.current_revision != self.base_revision,
            base_revision: self.base_revision,
            base_text: self.base_text,
            text: self.text,
            reason: self.reason,
            state,
            note: self.note,
            created_at: self.created_at,
            decided_at: self.decided_at,
        }
    }
}

pub struct LayerText {
    pub key: String,
    pub name: String,
    pub revision: String,
    pub text: String,
}

async fn find_owned(state: &AppState, id: Uuid, owner: Uuid) -> ApiResult<ProposalRow> {
    let row = sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!("select {PROPOSAL_COLUMNS} from instruction_proposals p left join conversations c on c.id = p.conversation_id where p.id = $1 and p.owner = $2")))
        .bind(id)
        .bind(owner)
        .fetch_optional(&state.pool)
        .await?;

    row.ok_or_else(|| ApiError::not_found("That proposal"))
}

fn publish(state: &AppState, owner: Uuid, row: ProposalRow) -> InstructionProposal {
    let proposal = row.to_proto();
    state.publish(Some(owner), StreamMessage::InstructionProposal(proposal.clone()));

    proposal
}

pub async fn republish_layer(state: &AppState, owner: Uuid, layer: &str) -> ApiResult<()> {
    let rows = sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!(
        "select {PROPOSAL_COLUMNS} from instruction_proposals p left join conversations c on c.id = p.conversation_id where p.layer = $1 and p.state = 'proposed'",
    )))
    .bind(layer)
    .fetch_all(&state.pool)
    .await?;

    for row in rows {
        publish(state, owner, row);
    }

    Ok(())
}

pub async fn layer_text(state: &AppState, conversation_id: Uuid, requested: Option<&str>) -> ApiResult<Result<LayerText, String>> {
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let (key, name) = match requested.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) if name.eq_ignore_ascii_case("global") => ("global".to_string(), "Global".to_string()),
        Some(name) => {
            let group = sqlx::query_as::<_, (Uuid, String)>("select id, name from groups where owner = $1 and lower(name) = lower($2)").bind(owner).bind(name).fetch_optional(&state.pool).await?;

            match group {
                Some((id, name)) => (id.to_string(), name),
                None => return Ok(Err(format!("No instructions layer is called {name}. Use global, or the name of a group."))),
            }
        }
        None => {
            let group = sqlx::query_as::<_, (Uuid, String)>(sqlx::AssertSqlSafe(format!("select g.id, g.name from conversations c join groups g on g.id = {CONVERSATION_GROUP_SQL} where c.id = $1"))).bind(conversation_id).fetch_optional(&state.pool).await?;

            match group {
                Some((id, name)) => (id.to_string(), name),
                None => ("global".to_string(), "Global".to_string()),
            }
        }
    };
    let latest = sqlx::query_as::<_, (String, String)>("select id, text from instruction_versions where layer = $1 order by seq desc limit 1").bind(&key).fetch_optional(&state.pool).await?;
    let (revision, text) = latest.unwrap_or_default();

    Ok(Ok(LayerText { key, name, revision: if revision.is_empty() { NO_REVISION.to_string() } else { revision }, text }))
}

pub async fn propose(state: &AppState, conversation_id: Uuid, requested: Option<&str>, base_revision: &str, text: &str, reason: &str) -> ApiResult<Result<InstructionProposal, String>> {
    let layer = match layer_text(state, conversation_id, requested).await? {
        Ok(layer) => layer,
        Err(message) => return Ok(Err(format!("Nothing was proposed. {message}"))),
    };

    if base_revision.trim() != layer.revision {
        return Ok(Err(format!(
            "Nothing was proposed. The {} instructions are at revision {}, not {}. Read the current text with get_instructions, make your edit on it and propose again with that revision.",
            layer.name,
            layer.revision,
            base_revision.trim()
        )));
    }

    if text == layer.text {
        return Ok(Err(format!("Nothing was proposed. The new text is the same as the current {} instructions.", layer.name)));
    }

    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    let stored_base = if layer.revision == NO_REVISION { "" } else { layer.revision.as_str() };

    let superseded = sqlx::query_scalar::<_, Uuid>("update instruction_proposals set state = 'superseded', decided_at = now() where conversation_id = $1 and layer = $2 and state = 'proposed' returning id")
        .bind(conversation_id)
        .bind(&layer.key)
        .fetch_all(&state.pool)
        .await?;
    let id = sqlx::query_scalar::<_, Uuid>("insert into instruction_proposals (owner, conversation_id, layer, layer_name, base_revision, base_text, text, reason) values ($1, $2, $3, $4, $5, $6, $7, $8) returning id")
        .bind(owner)
        .bind(conversation_id)
        .bind(&layer.key)
        .bind(&layer.name)
        .bind(stored_base)
        .bind(&layer.text)
        .bind(text)
        .bind(reason)
        .fetch_one(&state.pool)
        .await?;

    for earlier in superseded {
        publish(state, owner, find_owned(state, earlier, owner).await?);
    }

    Ok(Ok(publish(state, owner, find_owned(state, id, owner).await?)))
}

pub async fn propose_for_assistant(state: &AppState, owner: Uuid, assistant_id: Uuid, conversation_id: Option<Uuid>, text: &str, reason: &str) -> ApiResult<Result<InstructionProposal, String>> {
    let assistant = assistants::load(state, owner, assistant_id).await?;
    let current = assistants::instructions_of(state, assistant_id).await?;

    if text == current {
        return Ok(Err(format!("Nothing was proposed. The new text is the same as the current instructions of {}.", assistant.name)));
    }

    let key = instructions::assistant_layer(assistant_id);

    instructions::record_assistant_version(state, assistant_id, &current).await?;

    let revision = sqlx::query_scalar::<_, String>("select id from instruction_versions where layer = $1 order by seq desc limit 1").bind(&key).fetch_one(&state.pool).await?;
    let superseded = sqlx::query_scalar::<_, Uuid>("update instruction_proposals set state = 'superseded', decided_at = now() where layer = $1 and state = 'proposed' returning id").bind(&key).fetch_all(&state.pool).await?;
    let id = sqlx::query_scalar::<_, Uuid>("insert into instruction_proposals (owner, conversation_id, layer, layer_name, base_revision, base_text, text, reason) values ($1, $2, $3, $4, $5, $6, $7, $8) returning id")
        .bind(owner)
        .bind(conversation_id)
        .bind(&key)
        .bind(&assistant.name)
        .bind(revision)
        .bind(current)
        .bind(text)
        .bind(reason)
        .fetch_one(&state.pool)
        .await?;

    for earlier in superseded {
        publish(state, owner, find_owned(state, earlier, owner).await?);
    }

    Ok(Ok(publish(state, owner, find_owned(state, id, owner).await?)))
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<InstructionProposal>>> {
    let rows = sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!(
        "select {PROPOSAL_COLUMNS} from instruction_proposals p left join conversations c on c.id = p.conversation_id where p.owner = $1 order by (p.state = 'proposed') desc, p.created_at desc limit {LISTED}",
    )))
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows.into_iter().map(ProposalRow::to_proto).collect()))
}

pub async fn approve(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<InstructionProposal>> {
    let found = find_owned(&state, id, user.id).await?;

    if found.state != "proposed" {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    if found.current_revision != found.base_revision {
        return Err(ApiError::conflict("proposal_out_of_date", "The instructions changed after this edit was proposed. Decline it, or ask the agent to propose it again on the current text."));
    }

    let layer = find_layer(&state, &user, &found.layer).await?;

    instructions::apply(&state, &user, &layer, &found.text).await?;
    sqlx::query("update instruction_proposals set state = 'approved', decided_at = now() where id = $1").bind(id).execute(&state.pool).await?;

    Ok(Json(publish(&state, user.id, find_owned(&state, id, user.id).await?)))
}

pub async fn decline(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, request: Option<Json<DeclineInstructionProposalRequest>>) -> ApiResult<Json<InstructionProposal>> {
    let request = request.map(|Json(request)| request).unwrap_or_default();
    let found = find_owned(&state, id, user.id).await?;

    if found.state != "proposed" {
        return Err(ApiError::conflict("proposal_not_proposed", "This proposal has already been decided."));
    }

    let note = request.note.as_deref().map(str::trim).filter(|note| !note.is_empty());

    sqlx::query("update instruction_proposals set state = 'declined', note = $2, decided_at = now() where id = $1").bind(id).bind(note).execute(&state.pool).await?;

    if let Some(conversation_id) = found.conversation_id.filter(|_| request.notify) {
        let mut text = if request.redo {
            format!("The {} instructions changed after you proposed your edit to them. Read them again with get_instructions and, if your edit still applies, propose it again on the current text.", found.layer_name)
        } else {
            format!("I declined the edit you proposed to the {} instructions. Do not edit the instruction files another way.", found.layer_name)
        };

        if let Some(note) = note {
            text.push_str(&format!(" My note: {note}"));
        }

        let _ = prompts::prompt(State(state.clone()), user.clone(), Path(conversation_id), Json(PromptRequest { text, ..Default::default() })).await;
    }

    Ok(Json(publish(&state, user.id, find_owned(&state, id, user.id).await?)))
}
