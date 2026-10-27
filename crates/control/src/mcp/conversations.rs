use super::call::{ToolCall, ToolReply};
use crate::conversation_connections;
use crate::error::ApiResult;
use crate::group_connections;
use crate::message_proposals;
use crate::store::CONVERSATION_GROUP_SQL;
use serde_json::{Value, json};
use uuid::Uuid;

const DEFAULT_FIND_LIMIT: i64 = 10;
const MAX_FIND_LIMIT: i64 = 50;

pub fn definitions() -> Value {
    json!([
        {
            "name": "find_conversations",
            "description": "Look up the person's other conversations, most recent first, to find the one to message with send_to_conversation. Archived conversations are left out. It only reads and needs no approval.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Words that must each appear in the conversation's title, project name or folder."},
                    "group": {"type": "string", "description": "Only conversations in the group with this name, or none for conversations that are in no group."},
                    "limit": {"type": "number", "description": "How many conversations to return. Defaults to 10."},
                },
            },
        },
        {
            "name": "start_conversation",
            "description": "Start a new conversation in a project of another group and give it a first message. The person must have connected your group to that group in Settings, Agent connections; starting in your own group needs no connection. The new conversation is connected to this one, so it can reply with send_to_conversation. Name the project, or a folder together with its node.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "group": {"type": "string", "description": "The name of the group to start the conversation in."},
                    "project": {"type": "string", "description": "The name or id of a project in that group."},
                    "folder": {"type": "string", "description": "A folder in that group, instead of a project. Needs node."},
                    "node": {"type": "string", "description": "The name of the node the folder is on."},
                    "prompt": {"type": "string", "description": "The first message. Write it so it makes sense without this conversation."},
                    "title": {"type": "string", "description": "A title for the new conversation."},
                    "agent": {"type": "string", "description": "claude or codex. Defaults to this conversation's agent."},
                    "reason": {"type": "string", "description": "A short reason, shown in this conversation's transcript."},
                },
                "required": ["group", "prompt", "reason"],
            },
        },
        {
            "name": "send_to_conversation",
            "description": "Propose a message to send to another of the person's conversations. An approved connection, or a connection between the two conversations' groups, allows immediate delivery; otherwise the message waits for the person to approve it in a card in this conversation. Messages are prefixed with the source conversation title. Find the target with find_conversations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "conversation": {"type": "string", "description": "The id of the target conversation, or its exact title."},
                    "message": {"type": "string", "description": "The message to send. Write it so it makes sense without this conversation."},
                    "reason": {"type": "string", "description": "A short reason the person should approve it."},
                },
                "required": ["conversation", "message", "reason"],
            },
        },
    ])
}

#[derive(sqlx::FromRow)]
struct FoundConversation {
    id: Uuid,
    title: String,
    project_name: Option<String>,
    node_name: String,
    agent: String,
    status: String,
    updated_at: chrono::DateTime<chrono::Utc>,
    group_name: Option<String>,
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, tool, arguments, .. } = call;
    let state = caller.state;

    Ok(match tool {
        "find_conversations" => {
            let Some((conversation_id, _)) = caller.conversation().await? else {
                return Ok(ToolReply::text("The conversations belong to a person, and this call did not come from one of them.", true));
            };
            let limit = arguments.get("limit").and_then(Value::as_i64).unwrap_or(DEFAULT_FIND_LIMIT).clamp(1, MAX_FIND_LIMIT);
            let words: Vec<String> = arguments.get("query").and_then(Value::as_str).unwrap_or_default().split_whitespace().map(String::from).collect();
            let group_filter = arguments.get("group").and_then(Value::as_str).map(str::trim).filter(|name| !name.is_empty());
            let rows = sqlx::query_as::<_, FoundConversation>(sqlx::AssertSqlSafe(format!(
                "select * from (select c.id, c.title, p.name as project_name, coalesce(n.name, '') as node_name, c.agent, c.status, c.updated_at,
                        (select g.name from conversations x join groups g on g.id = {CONVERSATION_GROUP_SQL} where x.id = c.id) as group_name
                 from conversations c left join projects p on p.id = c.project_id left join nodes n on n.id = c.node_id
                 where c.owner = (select owner from conversations where id = $1) and c.archived_at is null
                   and not exists (select 1 from unnest($2::text[]) as word where not (strpos(lower(c.title), lower(word)) > 0 or strpos(lower(coalesce(p.name, '')), lower(word)) > 0 or strpos(lower(c.cwd), lower(word)) > 0))) found
                 where $4::text is null or (lower($4) = 'none' and group_name is null) or lower(group_name) = lower($4)
                 order by updated_at desc, id limit $3"
            )))
            .bind(conversation_id)
            .bind(&words)
            .bind(limit)
            .bind(group_filter)
            .fetch_all(&state.pool)
            .await?;

            if rows.is_empty() {
                ToolReply::text("No conversations match that query.", false)
            } else {
                let lines: Vec<String> = rows
                    .iter()
                    .map(|found| {
                        format!("{}: {} (project {}, group {}, node {}, agent {}, status {}, last activity {})", found.id, message_proposals::label(found.id, &found.title).replace('\n', " "), found.project_name.as_deref().unwrap_or("none"), found.group_name.as_deref().unwrap_or("none"), found.node_name, found.agent, found.status, found.updated_at.to_rfc3339())
                    })
                    .collect();

                ToolReply::text(&lines.join("\n"), false)
            }
        }
        "start_conversation" => {
            let field = |name: &str| arguments.get(name).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty());
            let (Some(group_name), Some(prompt), Some(reason)) = (field("group"), field("prompt"), field("reason")) else {
                return Ok(ToolReply::invalid("start_conversation needs group, prompt and reason, strings."));
            };
            let nothing = |message: String| ToolReply::text(&format!("Nothing was started. {message}"), true);

            let Some((conversation_id, caller_title)) = caller.conversation().await? else {
                return Ok(nothing("A new conversation is started from a conversation, and this call did not come from one.".to_string()));
            };
            let owner = caller.owner().await?;
            let Some((target_group, target_name)) = sqlx::query_as::<_, (Uuid, String)>("select id, name from groups where owner = $1 and lower(name) = lower($2)").bind(owner).bind(group_name).fetch_optional(&state.pool).await? else {
                return Ok(nothing(format!("None of the person's groups is named {group_name}.")));
            };

            let caller_group = group_connections::group_of(state, conversation_id).await?;
            let allowed = match caller_group {
                Some(own) => own == target_group || group_connections::active(state, owner, own, target_group).await?,
                None => false,
            };

            if !allowed {
                let message = match caller_group {
                    Some(own) => {
                        let own_name = sqlx::query_scalar::<_, String>("select name from groups where id = $1").bind(own).fetch_one(&state.pool).await?;

                        format!("The {own_name} and {target_name} groups are not connected. Ask the person to connect them in Settings, Agent connections.")
                    }
                    None => format!("This conversation is not in a group, so it cannot start one in {target_name}. Ask the person to put it in a group and connect that group to {target_name} in Settings, Agent connections."),
                };

                return Ok(nothing(message));
            }

            let place = match (field("project"), field("folder"), field("node")) {
                (Some(project), _, _) => {
                    let found = sqlx::query_as::<_, (Uuid, Uuid, Option<Uuid>)>("select id, node_id, group_id from projects where owner = $1 and (id::text = lower($2) or name = $2)").bind(owner).bind(project).fetch_optional(&state.pool).await?;

                    match found {
                        None => return Ok(nothing(format!("None of the person's projects is {project}."))),
                        Some((_, _, project_group)) if project_group != Some(target_group) => return Ok(nothing(format!("The project {project} is not in the {target_name} group."))),
                        Some((project_id, node_id, _)) => (node_id, String::new(), Some(project_id)),
                    }
                }
                (None, Some(folder), Some(node_name)) => {
                    let Some(node_id) = sqlx::query_scalar::<_, Uuid>("select id from nodes where created_by = $1 and name = $2 and revoked_at is null").bind(owner).bind(node_name).fetch_optional(&state.pool).await? else {
                        return Ok(nothing(format!("None of the person's nodes is named {node_name}.")));
                    };
                    let folder_group = sqlx::query_scalar::<_, Option<Uuid>>("select conversation_group($1, $2, null)").bind(folder).bind(node_id).fetch_one(&state.pool).await?;

                    if folder_group != Some(target_group) {
                        return Ok(nothing(format!("The folder {folder} is not in the {target_name} group.")));
                    }

                    (node_id, folder.to_string(), None)
                }
                _ => return Ok(nothing("Name a project in that group, or a folder together with its node.".to_string())),
            };

            let agent = match field("agent") {
                Some(agent) => agent.to_string(),
                None => sqlx::query_scalar::<_, String>("select agent from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?,
            };
            let Ok(agent) = crate::store::parse_enum(&agent) else {
                return Ok(nothing("The agent is claude or codex.".to_string()));
            };
            let user = crate::auth::AuthUser { id: owner, email: String::new(), name: String::new(), session: None, scopes: None, expires_at: None };
            let request = mycelium_proto::api::CreateConversationRequest {
                node_id: place.0,
                agent,
                account: None,
                cwd: place.1,
                project_id: place.2,
                branch: None,
                title: field("title").map(String::from),
                prompt: Some(format!("From {}:\n\n{prompt}", message_proposals::label(conversation_id, &caller_title))),
                parent_id: None,
                side_kind: None,
                model: None,
            };
            let started = match crate::conversations::start(state, owner, request).await {
                Ok(started) => started,
                Err(error) => return Ok(nothing(error.message().to_string())),
            };

            conversation_connections::connect(state, &user, conversation_id, started.id).await?;
            let node_name = sqlx::query_scalar::<_, String>("select name from nodes where id = $1").bind(started.node_id).fetch_one(&state.pool).await?;

            ToolReply::text(&format!("Started {} ({}) in {target_name} on {node_name}. The reason was: {reason}", message_proposals::label(started.id, &started.title), started.id), false)
        }
        _ => {
            let field = |name: &str| arguments.get(name).and_then(Value::as_str).map(str::trim);
            let (Some(target), Some(message), Some(reason)) = (field("conversation"), field("message"), field("reason")) else {
                return Ok(ToolReply::invalid("send_to_conversation needs conversation, message and reason, strings."));
            };

            if target.is_empty() || message.is_empty() || reason.is_empty() {
                return Ok(ToolReply::text("Nothing was proposed. Give the conversation, the message and the reason.", true));
            }

            let Some((conversation_id, _)) = caller.conversation().await? else {
                return Ok(ToolReply::text("Nothing was proposed. A message proposal belongs to a conversation, and this call did not come from one.", true));
            };

            match message_proposals::propose(state, conversation_id, target, message, reason).await? {
                Ok(proposal) => {
                    let message = if proposal.state == mycelium_proto::api::MessageProposalState::Sent {
                        format!("Sent to {} (message {}).", proposal.target_title, proposal.id)
                    } else {
                        format!("Waiting for the person to approve sending this to {} (proposal {}).", proposal.target_title, proposal.id)
                    };

                    ToolReply::text(&message, false)
                }
                Err(message) => ToolReply::text(&message, true),
            }
        }
    })
}
