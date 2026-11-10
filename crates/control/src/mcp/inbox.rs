use super::call::{ToolCall, ToolReply};
use crate::assistant_actions::is_voice_conversation;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::{message_proposals, prompts, start_proposals};
use chrono::{DateTime, Utc};
use mycelium_proto::api::{ApproveStartProposalRequest, PromptRequest, WorkflowRunStep};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

pub const TOOLS: [&str; 3] = ["list_needs_you", "resolve_needs_you", "focus_needs_you"];

const NOT_VOICE: &str = "Only the Voice assistant can act on the Inbox. Nothing was done.";
const PERMISSION_RULE: &str = "That one runs a command, so it is never resolved by voice. It is outlined in the Inbox. Tell the person: \"That one runs a command, so it needs your tap on Approve. I have outlined it.\"";

const GATE_RULE: &str = "A workflow gate is never resolved by voice. It is outlined in the Inbox. Tell the person: \"That workflow is waiting for your tap on Approve. I have outlined it.\"";

const WAITING_CONVERSATION: &str = "c.owner = $1 and c.archived_at is null and ((c.status = 'input' and c.managed_by is null) or c.flagged_at is not null) and not exists (select 1 from assistants a where a.id = c.assistant_id and a.builtin_key = 'voice')";

enum Target {
    Permission,
    Question,
    Flagged,
    MessageProposal(Uuid),
    StartProposal(Uuid),
    WorkflowGate,
}

struct Item {
    key: String,
    target: Target,
    conversation_id: Uuid,
    title: String,
    project: Option<String>,
    text: String,
    options: Vec<String>,
    waiting_since: DateTime<Utc>,
}

impl Target {
    fn kind(&self) -> &'static str {
        match self {
            Target::Permission => "permission",
            Target::Question => "question",
            Target::Flagged => "flagged",
            Target::MessageProposal(_) => "message_proposal",
            Target::StartProposal(_) => "start_proposal",
            Target::WorkflowGate => "workflow_gate",
        }
    }
}

pub fn definitions() -> Value {
    json!([
        {
            "name": "list_needs_you",
            "description": "List what is waiting on the person in the Inbox, newest first: permission requests, questions, message and start proposals, workflow gates and flagged conversations. Each has a key, a kind, the conversation title and project, the text and any options. The first item becomes the outlined one in the app. Only the Voice assistant may call it.",
            "inputSchema": {"type": "object", "properties": {}},
        },
        {
            "name": "resolve_needs_you",
            "description": "Resolve one Inbox item by its key. A question or flagged conversation takes answer as a new message, or approve or decline as that word. A message or start proposal is approved or declined. A permission request or workflow gate is never resolved this way: the call outlines it and the person must tap Approve. Only the Voice assistant may call it, and only for what the person said.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": {"type": "string", "description": "The key from list_needs_you."},
                    "action": {"type": "string", "enum": ["approve", "decline", "answer"]},
                    "answer": {"type": "string", "description": "What the person said, for a question or a flagged conversation."},
                },
                "required": ["key", "action"],
            },
        },
        {
            "name": "focus_needs_you",
            "description": "Outline an Inbox item in the app without resolving it, for moving to the next item or naming a conversation. Only the Voice assistant may call it.",
            "inputSchema": {
                "type": "object",
                "properties": {"key": {"type": "string", "description": "The key from list_needs_you."}},
                "required": ["key"],
            },
        },
    ])
}

async fn agent_text(state: &AppState, conversation_id: Uuid) -> ApiResult<String> {
    let text = sqlx::query_scalar::<_, String>(
        "select coalesce(string_agg(payload->'update'->'content'->>'text', '' order by seq), '') from events
         where conversation_id = $1 and kind = 'acp_update' and payload->'update'->>'sessionUpdate' = 'agent_message_chunk'
           and seq > coalesce((select max(seq) from events where conversation_id = $1 and (kind = 'user_message' or (kind = 'acp_update' and payload->'update'->>'sessionUpdate' <> 'agent_message_chunk'))), 0)",
    )
    .bind(conversation_id)
    .fetch_one(&state.pool)
    .await?;

    Ok(text)
}

async fn proposal_key(state: &AppState, conversation_id: Uuid, proposal_id: Uuid) -> ApiResult<String> {
    let seq = sqlx::query_scalar::<_, Option<i64>>(
        "select min(e.seq) from events e where e.conversation_id = $1 and e.kind = 'acp_update'
           and e.payload->'update'->>'toolCallId' = (select x.payload->'update'->>'toolCallId' from events x where x.conversation_id = $1 and x.kind = 'acp_update' and x.payload::text like $2 limit 1)",
    )
    .bind(conversation_id)
    .bind(format!("%(proposal {proposal_id})%"))
    .fetch_one(&state.pool)
    .await?;

    Ok(match seq {
        Some(seq) => format!("{conversation_id}/e{seq}"),
        None => format!("{conversation_id}/proposal-{proposal_id}"),
    })
}

async fn needs_you(state: &AppState, owner: Uuid) -> ApiResult<Vec<Item>> {
    let mut items = Vec::new();
    let mut busy = Vec::new();

    let permissions = sqlx::query_as::<_, (Uuid, i64, String, Option<String>, Option<String>, Value, DateTime<Utc>)>(sqlx::AssertSqlSafe(format!(
        "select r.conversation_id, r.seq, c.title, p.name, r.payload->'toolCall'->>'title', coalesce(r.payload->'options', '[]'::jsonb), coalesce(c.flagged_at, c.updated_at)
         from events r join conversations c on c.id = r.conversation_id left join projects p on p.id = c.project_id
         where {WAITING_CONVERSATION} and r.kind = 'permission_request'
           and not exists (select 1 from events s where s.conversation_id = r.conversation_id and s.kind = 'permission_resolved' and s.payload->>'request_id' = r.payload->>'request_id')"
    )))
    .bind(owner)
    .fetch_all(&state.pool)
    .await?;

    for (conversation_id, seq, title, project, command, options, waiting_since) in permissions {
        let options = options.as_array().into_iter().flatten().filter_map(|option| option["name"].as_str().map(String::from)).collect();

        busy.push(conversation_id);
        items.push(Item { key: format!("{conversation_id}/e{seq}"), target: Target::Permission, conversation_id, title, project, text: command.unwrap_or_else(|| "Permission needed".to_string()), options, waiting_since });
    }

    let messages = sqlx::query_as::<_, (Uuid, Uuid, String, Option<String>, String, String, DateTime<Utc>)>(sqlx::AssertSqlSafe(format!(
        "select m.id, m.conversation_id, c.title, p.name, m.message, t.title, coalesce(c.flagged_at, c.updated_at)
         from message_proposals m join conversations c on c.id = m.conversation_id join conversations t on t.id = m.target_id left join projects p on p.id = c.project_id
         where {WAITING_CONVERSATION} and m.owner = $1 and m.state = 'proposed'"
    )))
    .bind(owner)
    .fetch_all(&state.pool)
    .await?;

    for (id, conversation_id, title, project, message, target, waiting_since) in messages {
        busy.push(conversation_id);
        items.push(Item { key: proposal_key(state, conversation_id, id).await?, target: Target::MessageProposal(id), conversation_id, title, project, text: format!("Send to {target}: {message}"), options: Vec::new(), waiting_since });
    }

    let starts = sqlx::query_as::<_, (Uuid, Uuid, String, Option<String>, Option<String>, DateTime<Utc>)>(sqlx::AssertSqlSafe(format!(
        "select s.id, s.conversation_id, c.title, p.name, s.plan->>'title', coalesce(c.flagged_at, c.updated_at)
         from start_proposals s join conversations c on c.id = s.conversation_id left join projects p on p.id = c.project_id
         where {WAITING_CONVERSATION} and s.owner = $1 and s.state = 'proposed'"
    )))
    .bind(owner)
    .fetch_all(&state.pool)
    .await?;

    for (id, conversation_id, title, project, plan_title, waiting_since) in starts {
        busy.push(conversation_id);
        items.push(Item { key: proposal_key(state, conversation_id, id).await?, target: Target::StartProposal(id), conversation_id, title, project, text: format!("Start {}", plan_title.unwrap_or_default()), options: Vec::new(), waiting_since });
    }

    let gates = sqlx::query_as::<_, (Uuid, Uuid, String, Option<String>, Value, DateTime<Utc>)>(
        "select w.id, w.conversation_id, c.title, p.name, w.steps, w.updated_at
         from workflows w join conversations c on c.id = w.conversation_id left join projects p on p.id = c.project_id
         where w.owner = $1 and w.kind = 'mycelium'",
    )
    .bind(owner)
    .fetch_all(&state.pool)
    .await?;

    for (id, conversation_id, title, project, steps, waiting_since) in gates {
        let steps: Vec<WorkflowRunStep> = serde_json::from_value(steps).unwrap_or_default();
        let Some(step) = steps.into_iter().find(|step| step.status == "gate") else {
            continue;
        };

        items.push(Item { key: format!("{conversation_id}/workflow-{id}"), target: Target::WorkflowGate, conversation_id, title, project, text: format!("{} is waiting for a decision.", step.label.unwrap_or(step.key)), options: Vec::new(), waiting_since });
    }

    let conversations = sqlx::query_as::<_, (Uuid, String, Option<String>, Option<String>, Option<DateTime<Utc>>, DateTime<Utc>)>(sqlx::AssertSqlSafe(format!(
        "select c.id, c.title, p.name, c.flag_reason, c.flagged_at, c.updated_at
         from conversations c left join projects p on p.id = c.project_id where {WAITING_CONVERSATION}"
    )))
    .bind(owner)
    .fetch_all(&state.pool)
    .await?;

    for (conversation_id, title, project, flag_reason, flagged_at, updated_at) in conversations.into_iter().filter(|(id, ..)| !busy.contains(id)) {
        let flagged = flag_reason.filter(|reason| !reason.trim().is_empty());
        let text = match flagged {
            Some(reason) => reason,
            None => agent_text(state, conversation_id).await?,
        };
        let target = if flagged_at.is_some() { Target::Flagged } else { Target::Question };

        items.push(Item { key: format!("{conversation_id}/flag"), target, conversation_id, title, project, text, options: Vec::new(), waiting_since: flagged_at.unwrap_or(updated_at) });
    }

    items.sort_by(|first, second| second.waiting_since.cmp(&first.waiting_since));

    Ok(items)
}

fn listed(item: &Item) -> Value {
    json!({"key": item.key, "kind": item.target.kind(), "conversation": item.title, "project": item.project, "text": item.text, "options": item.options})
}

fn focus(state: &AppState, owner: Uuid, key: &str) {
    state.publish(Some(owner), StreamMessage::NeedsYouFocus { key: key.to_string() });
}

async fn resolve(state: &AppState, owner: Uuid, item: &Item, action: &str, answer: Option<String>) -> ApiResult<String> {
    let title = item.project.as_ref().map_or_else(|| item.title.clone(), |project| format!("{project}, {}", item.title));

    match (&item.target, action) {
        (Target::Permission, _) => Ok(PERMISSION_RULE.to_string()),
        (Target::WorkflowGate, _) => Ok(GATE_RULE.to_string()),
        (Target::Question | Target::Flagged, "answer" | "approve" | "decline") => {
            let word = match action {
                "approve" => Some("Approve".to_string()),
                "decline" => Some("Decline".to_string()),
                _ => None,
            };
            let text = answer.filter(|text| !text.trim().is_empty()).or(word).ok_or_else(|| ApiError::bad_request("answer_required", "Give the answer the person said."))?;
            prompts::send(state, owner, item.conversation_id, PromptRequest { text: text.clone(), ..Default::default() }).await?;

            Ok(format!("Answered in {title}: {text}"))
        }
        (Target::MessageProposal(id), "approve") => {
            message_proposals::approve_for(state, owner, *id).await?;

            Ok(format!("Approved in {title}: the message was sent."))
        }
        (Target::MessageProposal(id), "decline") => {
            message_proposals::decline_for(state, owner, *id).await?;

            Ok(format!("Declined in {title}: the message was not sent."))
        }
        (Target::StartProposal(id), "approve") => {
            start_proposals::approve_for(state, owner, *id, ApproveStartProposalRequest::default()).await?;

            Ok(format!("Approved in {title}: it has started."))
        }
        (Target::StartProposal(id), "decline") => {
            start_proposals::decline_for(state, owner, *id).await?;

            Ok(format!("Declined in {title}: it was not started."))
        }
        _ => Err(ApiError::bad_request("action_invalid", format!("A {} item cannot be given {action}.", item.target.kind().replace('_', " ")))),
    }
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, tool, arguments, .. } = call;
    let state = caller.state;
    let owner = caller.owner().await?;
    let conversation_id = caller.conversation_id().await?.ok_or_else(|| ApiError::not_found("conversation"))?;

    if !is_voice_conversation(state, conversation_id).await? {
        return Ok(ToolReply::text(NOT_VOICE, true));
    }

    let items = needs_you(state, owner).await?;

    if tool == "list_needs_you" {
        let listed: Vec<Value> = items.iter().map(listed).collect();

        if let Some(top) = items.first() {
            focus(state, owner, &top.key);
        }

        return Ok(ToolReply::text(&serde_json::to_string(&listed).map_err(ApiError::internal)?, false));
    }

    let key = arguments.get("key").and_then(Value::as_str).map(str::trim).unwrap_or_default();
    let Some(item) = items.iter().find(|item| item.key == key) else {
        return Ok(ToolReply::text("That item is no longer waiting. Call list_needs_you for what is.", true));
    };

    focus(state, owner, &item.key);

    if tool == "focus_needs_you" {
        return Ok(ToolReply::text(&format!("Outlined {} in the Inbox.", item.title), false));
    }

    let action = arguments.get("action").and_then(Value::as_str).unwrap_or_default();
    let answer = arguments.get("answer").and_then(Value::as_str).map(String::from);
    let resolved = resolve(state, owner, item, action, answer).await;

    let text = match resolved {
        Ok(text) => text,
        Err(error) => return Ok(ToolReply::text(error.message(), true)),
    };

    if matches!(item.target, Target::Permission | Target::WorkflowGate) {
        return Ok(ToolReply::text(&text, true));
    }

    let remaining = needs_you(state, owner).await?;
    let next = remaining.iter().find(|other| other.key != item.key);

    if let Some(next) = next {
        focus(state, owner, &next.key);

        return Ok(ToolReply::text(&format!("{text}\nNext: {} ({}), key {}.", next.title, next.target.kind(), next.key), false));
    }

    Ok(ToolReply::text(&format!("{text}\nNothing else is waiting."), false))
}
