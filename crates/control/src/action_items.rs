use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::nodes::{COMMAND_TERMINAL_SINCE, node_has, node_offline};
use crate::state::AppState;
use crate::store::{find_owned, resolve_action_item};
use axum::Json;
use axum::extract::{Path, State};
use mycelium_proto::api::StartTerminalRequest;
use mycelium_proto::conversation::Conversation;
use mycelium_proto::link::ControlToNode;
use mycelium_proto::stream::StreamMessage;
use uuid::Uuid;

const STORED_OUTPUT_CHARACTERS: usize = 16 * 1024;
const EVENT_OUTPUT_CHARACTERS: usize = 4000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunEnd {
    Exited(Option<i32>),
    Stopped,
    Lost,
}

struct RunningItem {
    id: Uuid,
    conversation_id: Uuid,
    owner: Uuid,
    text: String,
    command: String,
    folder: String,
    node_name: String,
}

pub async fn run(State(state): State<AppState>, user: AuthUser, Path((id, item_id)): Path<(Uuid, Uuid)>, Json(request): Json<StartTerminalRequest>) -> ApiResult<Json<Conversation>> {
    find_owned(&state.pool, id, user.id).await?;

    if request.cols == 0 || request.rows == 0 {
        return Err(ApiError::bad_request("size_required", "The terminal needs at least one column and one row."));
    }

    let item = sqlx::query_as::<_, (Option<String>, Option<Uuid>, Option<String>, Option<String>)>("select command, node_id, folder, run_status from conversation_action_items where id = $1 and conversation_id = $2 and done_at is null")
        .bind(item_id)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    let Some((command, node_id, folder, run_status)) = item else {
        return Err(ApiError::not_found("That action item"));
    };
    let (Some(command), Some(folder)) = (command, folder) else {
        return Err(ApiError::bad_request("no_command", "This action item has no command to run. Mark it done once you have done it yourself."));
    };

    if run_status.as_deref() == Some("running") {
        return Err(ApiError::conflict("already_running", "This command is already running. Stop it before running it again."));
    }

    let Some(node_id) = node_id else {
        return Err(ApiError::conflict("node_removed", "The node this command was for has been removed. Mark the item done, or ask the agent to add it again."));
    };
    let (node_name, node_version) = sqlx::query_as::<_, (String, String)>("select name, version from nodes where id = $1").bind(node_id).fetch_one(&state.pool).await?;

    if !state.hub.is_online(node_id) {
        return Err(node_offline());
    }

    if !node_has(&node_version, COMMAND_TERMINAL_SINCE) {
        return Err(ApiError::conflict("node_outdated", format!("Update {node_name} to Mycelium v{}.{}.{} or later to run this command, then press Run again.", COMMAND_TERMINAL_SINCE.0, COMMAND_TERMINAL_SINCE.1, COMMAND_TERMINAL_SINCE.2)));
    }

    let terminal_id = Uuid::new_v4();
    sqlx::query("update conversation_action_items set run_terminal_id = $2, run_status = 'running', run_started_at = now(), run_exit_code = null, run_output = null, run_reason = null where id = $1").bind(item_id).bind(terminal_id).execute(&state.pool).await?;
    state.hub.track_terminal(terminal_id, node_id, user.id, id, 0);
    state.hub.capture_terminal_output(terminal_id);

    let sent = state.hub.send(node_id, ControlToNode::StartTerminal { terminal_id, conversation_id: id, cwd: folder, cols: request.cols, rows: request.rows, command: Some(command) });

    if !sent {
        state.hub.finish_terminal(terminal_id);
        sqlx::query("update conversation_action_items set run_terminal_id = null, run_status = null, run_started_at = null where id = $1").bind(item_id).execute(&state.pool).await?;

        return Err(node_offline());
    }

    let conversation = find_owned(&state.pool, id, user.id).await?.to_proto()?;
    state.publish(Some(user.id), StreamMessage::Conversation(conversation.clone()));

    Ok(Json(conversation))
}

pub fn run_ended_in_background(state: &AppState, terminal_id: Uuid, end: RunEnd, captured: Option<Vec<u8>>) {
    let captured = captured.unwrap_or_default();
    let state = state.clone();

    tokio::spawn(async move {
        if let Err(error) = run_ended(&state, terminal_id, end, captured).await {
            tracing::warn!("could not record the end of an action item run: {}", error.message());
        }
    });
}

pub async fn run_ended(state: &AppState, terminal_id: Uuid, end: RunEnd, captured: Vec<u8>) -> ApiResult<()> {
    let output = tail(&clean_terminal_output(&captured), STORED_OUTPUT_CHARACTERS);
    let (status, exit_code, reason) = match end {
        RunEnd::Exited(Some(0)) => ("succeeded", Some(0), None),
        RunEnd::Exited(Some(code)) => ("failed", Some(code), None),
        RunEnd::Exited(None) => ("failed", None, Some("The command could not be started, or ended without an exit code.")),
        RunEnd::Stopped => ("stopped", None, Some("Stopped before the command finished.")),
        RunEnd::Lost => ("failed", None, Some("The node lost its connection before the command finished.")),
    };
    let item = sqlx::query_as::<_, (Uuid, Uuid, Uuid, String, String, String, String)>(
        "update conversation_action_items i set run_status = $2, run_exit_code = $3, run_output = $4, run_reason = $5
         from conversations c, nodes n
         where i.run_terminal_id = $1 and i.run_status = 'running' and c.id = i.conversation_id and n.id = i.node_id
         returning i.id, i.conversation_id, c.owner, i.text, i.command, i.folder, n.name",
    )
    .bind(terminal_id)
    .bind(status)
    .bind(exit_code)
    .bind(&output)
    .bind(reason)
    .fetch_optional(&state.pool)
    .await?;
    let Some((id, conversation_id, owner, text, command, folder, node_name)) = item else {
        return Ok(());
    };
    let item = RunningItem { id, conversation_id, owner, text, command, folder, node_name };

    if status == "succeeded" {
        resolve_action_item(&state.pool, item.conversation_id, item.id, item.owner).await?;
    }

    let conversation = find_owned(&state.pool, item.conversation_id, item.owner).await?.to_proto()?;
    state.publish(Some(item.owner), StreamMessage::Conversation(conversation));

    tell_the_agent(state, &item, status, exit_code, reason, &output).await
}

async fn tell_the_agent(state: &AppState, item: &RunningItem, status: &str, exit_code: Option<i32>, reason: Option<&str>, output: &str) -> ApiResult<()> {
    let (archived, chat) = sqlx::query_as::<_, (bool, bool)>("select archived_at is not null, agent = $2 from conversations where id = $1").bind(item.conversation_id).bind(crate::chats::CHAT_AGENT).fetch_one(&state.pool).await?;

    if archived {
        return Ok(());
    }

    let result = match (status, exit_code, reason) {
        ("succeeded", _, _) => "It succeeded with exit code 0, so the action item is resolved.".to_string(),
        (_, Some(code), _) => format!("It failed with exit code {code}. The action item is still open for the person. If a different command would work, add a corrected action item."),
        (_, None, Some(reason)) => format!("{reason} The action item is still open for the person. If a different command would work, add a corrected action item."),
        (_, None, None) => "It did not finish.".to_string(),
    };
    let shown = tail(output.trim_end(), EVENT_OUTPUT_CHARACTERS);
    let shown = if shown.is_empty() { "It printed nothing.".to_string() } else { format!("Output, last {EVENT_OUTPUT_CHARACTERS} characters at most:\n{shown}") };
    let text = format!("The person ran the command of your action item \"{}\" on {} in {}.\nCommand: {}\n{result}\n{shown}", item.text, item.node_name, item.folder, item.command);

    crate::watches::send_event(state, item.owner, item.conversation_id, chat, text).await
}

pub fn clean_terminal_output(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut cleaned = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();

    while let Some(character) = characters.next() {
        match character {
            '\u{1b}' => match characters.next() {
                Some('[') => {
                    while characters.next_if(|next| !('\u{40}'..='\u{7e}').contains(next)).is_some() {}
                    characters.next();
                }
                Some(']') => {
                    while let Some(next) = characters.next() {
                        if next == '\u{7}' || (next == '\u{1b}' && characters.next_if_eq(&'\\').is_some()) {
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\r' => {
                if characters.peek() != Some(&'\n') {
                    let line_start = cleaned.rfind('\n').map(|at| at + 1).unwrap_or(0);
                    cleaned.truncate(line_start);
                }
            }
            other => cleaned.push(other),
        }
    }

    cleaned
}

fn tail(text: &str, most: usize) -> String {
    let count = text.chars().count();

    if count <= most {
        return text.to_string();
    }

    text.chars().skip(count - most).collect()
}
