use crate::activity::record_turn;
use crate::auth::AuthNode;
use crate::device::client_address;
use crate::error::ApiResult;
use crate::prompts::pump;
use crate::state::{AppState, GitAnswer, ProcessAnswer};
use crate::store::{CONVERSATION_COLUMNS, ConversationRow, append_event, enum_text, highest_node_seq, lock_on_node, publish_node, set_status};
use crate::transcripts::{WriteError, write_chunk};
use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use std::net::SocketAddr;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use mycelium_proto::conversation::{AgentInfo, ConversationStatus, EventKind, LoginInfo, SessionPosition};
use mycelium_proto::link::{ControlToNode, NodeToControl, REPLACED_CLOSE_CODE};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{Instant, interval_at};
use uuid::Uuid;

const PREDICTED_TITLE_LIMIT: usize = 100;

const PING_INTERVAL: Duration = Duration::from_secs(25);
const SILENCE_LIMIT: Duration = Duration::from_secs(90);

pub async fn connect(State(state): State<AppState>, node: AuthNode, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, upgrade: WebSocketUpgrade) -> Response {
    let address = client_address(&state, &headers, peer).to_string();
    let _ = sqlx::query("update nodes set address = $2 where id = $1").bind(node.id).bind(address).execute(&state.pool).await;

    let (sender, outgoing) = mpsc::unbounded_channel::<ControlToNode>();
    state.finish_logins_of_node(node.id);
    let connection_id = state.hub.register(node.id, sender);

    upgrade.on_upgrade(move |socket| run_link(state, node.id, connection_id, outgoing, socket))
}

async fn run_link(state: AppState, node_id: Uuid, connection_id: Uuid, mut outgoing: mpsc::UnboundedReceiver<ControlToNode>, socket: WebSocket) {
    let (mut sink, mut source) = socket.split();
    let _ = publish_node(&state, node_id).await;

    let mut ping = interval_at(Instant::now() + PING_INTERVAL, PING_INTERVAL);
    let mut last_frame = Instant::now();

    loop {
        tokio::select! {
            message = outgoing.recv() => {
                let Some(message) = message else {
                    if state.hub.is_replaced(node_id, connection_id) {
                        let close = CloseFrame { code: REPLACED_CLOSE_CODE, reason: "replaced by a newer connection for this node".into() };
                        let _ = sink.send(Message::Close(Some(close))).await;
                    }

                    break;
                };
                let Ok(text) = serde_json::to_string(&message) else {
                    continue;
                };

                if sink.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            incoming = source.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    last_frame = Instant::now();

                    match serde_json::from_str::<NodeToControl>(&text) {
                        Ok(message) => {
                            if let Err(cause) = handle(&state, node_id, message).await {
                                tracing::warn!("node {node_id} message failed: {cause:?}");
                            }
                        }
                        Err(cause) => tracing::warn!("node {node_id} sent an unreadable frame: {cause}"),
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => last_frame = Instant::now(),
            },
            _ = ping.tick() => {
                if last_frame.elapsed() > SILENCE_LIMIT {
                    break;
                }

                let Ok(text) = serde_json::to_string(&ControlToNode::Ping) else {
                    continue;
                };

                if sink.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    }

    if state.hub.unregister(node_id, connection_id) {
        state.finish_logins_of_node(node_id);
        let _ = publish_node(&state, node_id).await;
    }
}

async fn handle(state: &AppState, node_id: Uuid, message: NodeToControl) -> ApiResult<()> {
    match message {
        NodeToControl::Hello { version, hostname, os, arch, agents, sessions } => hello(state, node_id, version, hostname, os, arch, agents, sessions).await,
        NodeToControl::SessionStarted { conversation_id, acp_session_id, modes, models, config_options } => session_started(state, node_id, conversation_id, acp_session_id, modes, models, config_options).await,
        NodeToControl::PromptReceived { conversation_id, prompt_id } => {
            state.hub.acknowledge_prompt(node_id, conversation_id, prompt_id);

            Ok(())
        }
        NodeToControl::AcpUpdate { conversation_id, seq, update } => {
            let predicted = update["update"]["title"].as_str().filter(|_| update["update"]["sessionUpdate"] == "session_info_update").map(|title| title.trim().chars().take(PREDICTED_TITLE_LIMIT).collect::<String>()).filter(|title| !title.is_empty());
            let inserted = record(state, node_id, conversation_id, Some(seq), EventKind::AcpUpdate, update, None).await?;

            let (true, Some(title)) = (inserted, predicted) else {
                return Ok(());
            };

            let renamed = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
                "update conversations set predicted_title = $3, title = case when title_user_set then title else $3 end where id = $1 and node_id = $2 returning {CONVERSATION_COLUMNS}"
            )))
            .bind(conversation_id)
            .bind(node_id)
            .bind(title)
            .fetch_optional(&state.pool)
            .await?;

            if let Some(renamed) = renamed {
                state.publish(Some(renamed.owner), StreamMessage::Conversation(renamed.to_proto()?));
            }

            Ok(())
        }
        NodeToControl::PermissionRequest { conversation_id, seq, request_id, request } => {
            let mut payload = request.clone();

            match payload.as_object_mut() {
                Some(fields) => {
                    fields.insert("request_id".into(), Value::String(request_id));
                }
                None => payload = json!({"request_id": request_id, "request": request}),
            }

            record(state, node_id, conversation_id, Some(seq), EventKind::PermissionRequest, payload, Some(ConversationStatus::Input)).await.map(|_| ())
        }
        NodeToControl::TurnEnd { conversation_id, seq, stop_reason, usage } => turn_end(state, node_id, conversation_id, seq, stop_reason, usage).await,
        NodeToControl::SessionClosed { conversation_id } => state.remove_session(node_id, conversation_id).await,
        NodeToControl::SessionFailed { conversation_id, seq, message } => {
            state.remove_session(node_id, conversation_id).await?;
            state.hub.forget_prompts_of(conversation_id);
            state.hub.take_send_now(conversation_id);
            let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_optional(&state.pool).await?;

            if let Some(owner) = owner {
                crate::moves::session_failed(state, owner, conversation_id);
            }

            let limit = crate::usage_limits::inspect(state, node_id, conversation_id, &message).await?;
            let resume_at = limit.as_ref().and_then(|limit| limit.resume_at);

            if resume_at.is_none()
                && let Err(error) = crate::goals::session_failed(state, conversation_id).await
            {
                tracing::warn!("goal handling after a failed session failed for {conversation_id}: {}", error.message());
            }

            let payload = limit.map_or_else(|| json!({"message": message}), |limit| limit.payload);
            let recorded = record(state, node_id, conversation_id, Some(seq), EventKind::Error, payload, Some(ConversationStatus::Failed)).await?;

            if let Some(at) = resume_at.filter(|_| recorded) {
                crate::usage_limits::schedule(state, conversation_id, at).await?;
            }

            Ok(())
        }
        NodeToControl::Workflow { conversation_id, run_id, line } => {
            record(state, node_id, conversation_id, None, EventKind::Workflow, json!({"run_id": run_id, "line": line}), None).await.map(|_| ())
        }
        NodeToControl::Logins { logins } => store_logins(state, node_id, logins).await,
        NodeToControl::LoginOutput { login_id, data } => {
            if let Some(owner) = state.hub.login_owner(login_id).filter(|owner| owner.node_id == node_id) {
                state.publish(Some(owner.user_id), StreamMessage::LoginOutput { login_id, data });
            }

            Ok(())
        }
        NodeToControl::LoginFinished { login_id, ok } => {
            if let Some(owner) = state.hub.login_owner(login_id).filter(|owner| owner.node_id == node_id) {
                state.hub.finish_login(login_id);
                state.publish(Some(owner.user_id), StreamMessage::LoginFinished { login_id, ok });
            }

            Ok(())
        }
        NodeToControl::TerminalOutput { terminal_id, data } => {
            if let Some(owner) = state.hub.terminal_owner(terminal_id).filter(|owner| owner.node_id == node_id) {
                state.publish(Some(owner.user_id), StreamMessage::TerminalOutput { terminal_id, data });
            }

            Ok(())
        }
        NodeToControl::TerminalExited { terminal_id } => {
            if let Some(owner) = state.hub.terminal_owner(terminal_id).filter(|owner| owner.node_id == node_id) {
                state.hub.finish_terminal(terminal_id);
                state.publish(Some(owner.user_id), StreamMessage::TerminalExited { terminal_id });
            }

            Ok(())
        }
        NodeToControl::Transcript { path, offset, data } => store_transcript(state, node_id, path, offset, data).await,
        NodeToControl::Resources { sample } => {
            state.hub.record_resources(node_id, sample.clone());
            state.publish(None, StreamMessage::NodeResources { node_id, sample });

            Ok(())
        }
        NodeToControl::Files { request_id, paths } => {
            state.hub.answer_files(node_id, request_id, paths);

            Ok(())
        }
        NodeToControl::GitResult { request_id, ok, output, error } => {
            state.hub.answer_git(node_id, request_id, GitAnswer { ok, output, error });

            Ok(())
        }
        NodeToControl::ProcessStopped { request_id, ok, error } => {
            state.hub.answer_stop(node_id, request_id, ProcessAnswer { ok, error });

            Ok(())
        }
        NodeToControl::InstructionsHeld { revision, unmanaged } => {
            sqlx::query("update nodes set instructions_revision = $2, instructions_unmanaged = $3 where id = $1").bind(node_id).bind(revision).bind(unmanaged).execute(&state.pool).await?;

            publish_node(state, node_id).await
        }
        NodeToControl::Pong => {
            sqlx::query("update nodes set last_seen_at = now() where id = $1 and (last_seen_at is null or last_seen_at < now() - interval '30 seconds')").bind(node_id).execute(&state.pool).await?;

            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn hello(state: &AppState, node_id: Uuid, version: String, hostname: String, os: String, arch: String, agents: Vec<AgentInfo>, sessions: Vec<SessionPosition>) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    sqlx::query("update nodes set version = $2, hostname = $3, os = $4, arch = $5, last_seen_at = now() where id = $1")
        .bind(node_id)
        .bind(version)
        .bind(hostname)
        .bind(os)
        .bind(arch)
        .execute(&mut *tx)
        .await?;
    sqlx::query("delete from agents where node_id = $1").bind(node_id).execute(&mut *tx).await?;

    for agent in &agents {
        sqlx::query("insert into agents (node_id, agent, version, available) values ($1, $2, $3, $4)")
            .bind(node_id)
            .bind(enum_text(&agent.agent))
            .bind(&agent.version)
            .bind(agent.available)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;

    let live: HashSet<Uuid> = sessions.iter().filter(|session| session.live).map(|session| session.conversation_id).collect();
    fail_conversations_without_a_session(state, node_id, &live).await?;
    state.hub.set_sessions(node_id, live.clone());

    let conversations = sqlx::query_scalar::<_, Uuid>("select id from conversations where node_id = $1").bind(node_id).fetch_all(&state.pool).await?;

    for conversation_id in conversations {
        let seq = highest_node_seq(&state.pool, conversation_id).await?;

        if seq > 0 || live.contains(&conversation_id) {
            state.hub.send(node_id, ControlToNode::Ack { conversation_id, seq });
        }
    }

    for prompt in state.hub.unacknowledged_prompts_on(node_id, &live) {
        let attachments = crate::attachments::for_node(state, &prompt.attachments).await?;
        state.hub.send(node_id, ControlToNode::Prompt { conversation_id: prompt.conversation_id, prompt_id: prompt.prompt_id, text: prompt.text, attachments });
    }

    crate::instructions::push_to(state, node_id, false).await?;
    publish_node(state, node_id).await?;

    let waiting = sqlx::query_scalar::<_, Uuid>("select distinct q.conversation_id from queued_prompts q join conversations c on c.id = q.conversation_id where c.node_id = $1")
        .bind(node_id)
        .fetch_all(&state.pool)
        .await?;

    for conversation_id in waiting {
        pump(state, conversation_id, false).await?;
    }

    Ok(())
}

async fn fail_conversations_without_a_session(state: &AppState, node_id: Uuid, live: &HashSet<Uuid>) -> ApiResult<()> {
    let running = sqlx::query_scalar::<_, Uuid>("select id from conversations where node_id = $1 and status in ('working', 'input')").bind(node_id).fetch_all(&state.pool).await?;

    for conversation_id in running.into_iter().filter(|id| !live.contains(id)) {
        let mut tx = state.pool.begin().await?;
        let Some(row) = lock_on_node(&mut tx, conversation_id, node_id).await? else {
            continue;
        };

        if !matches!(row.status.as_str(), "working" | "input") {
            continue;
        }

        let message = "The agent stopped while this conversation was running, so the last message may not have been handled. Start a new conversation to continue.";
        let event = append_event(&mut tx, conversation_id, EventKind::Error, &json!({"message": message}), None).await?;
        let updated = set_status(&mut tx, conversation_id, ConversationStatus::Failed).await?;
        tx.commit().await?;

        if let Some(event) = event {
            state.publish(Some(row.owner), StreamMessage::Event(event));
        }

        state.publish(Some(row.owner), StreamMessage::Conversation(updated.to_proto()?));
    }

    Ok(())
}

async fn session_started(state: &AppState, node_id: Uuid, conversation_id: Uuid, acp_session_id: String, modes: Option<Value>, models: Option<Value>, config_options: Option<Value>) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    let Some(before) = lock_on_node(&mut tx, conversation_id, node_id).await? else {
        return Ok(());
    };

    let mode = modes.as_ref().and_then(|modes| modes["currentModeId"].as_str()).map(str::to_string);
    let model = models.as_ref().and_then(|models| models["currentModelId"].as_str()).map(str::to_string);
    let options = json!({"modes": modes, "models": models, "config_options": config_options});
    let updated = sqlx::query_as::<_, crate::store::ConversationRow>(sqlx::AssertSqlSafe(format!(
        "update conversations set
            mode = case when acp_session_id = $2 then coalesce(mode, $3) else coalesce($3, mode) end,
            model = case when acp_session_id = $2 then coalesce(model, $4) else coalesce($4, model) end,
            acp_session_id = $2, options = $5, updated_at = now() where id = $1 returning {}",
        crate::store::CONVERSATION_COLUMNS
    )))
    .bind(conversation_id)
    .bind(acp_session_id)
    .bind(mode)
    .bind(model)
    .bind(options)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    state.add_session(node_id, conversation_id).await?;
    state.publish(Some(updated.owner), StreamMessage::Conversation(updated.to_proto()?));
    pump(state, conversation_id, before.acp_session_id.is_none()).await
}

async fn record(state: &AppState, node_id: Uuid, conversation_id: Uuid, seq: Option<i64>, kind: EventKind, payload: Value, transition: Option<ConversationStatus>) -> ApiResult<bool> {
    let mut tx = state.pool.begin().await?;
    let Some(row) = lock_on_node(&mut tx, conversation_id, node_id).await? else {
        return Ok(false);
    };

    let event = append_event(&mut tx, conversation_id, kind, &payload, seq).await?;
    let mut updated = None;

    if event.is_some()
        && let Some(status) = transition
        && row.status != enum_text(&ConversationStatus::Failed)
    {
        updated = Some(set_status(&mut tx, conversation_id, status).await?);
    }

    tx.commit().await?;

    let acknowledged = highest_node_seq(&state.pool, conversation_id).await?;
    state.hub.send(node_id, ControlToNode::Ack { conversation_id, seq: acknowledged });

    let inserted = event.is_some();

    if let Some(event) = event {
        state.publish(Some(row.owner), StreamMessage::Event(event));
    }

    if let Some(updated) = updated {
        state.publish(Some(row.owner), StreamMessage::Conversation(updated.to_proto()?));
    }

    Ok(inserted)
}

async fn turn_end(state: &AppState, node_id: Uuid, conversation_id: Uuid, seq: i64, stop_reason: String, usage: Option<Value>) -> ApiResult<()> {
    let inserted = record(state, node_id, conversation_id, Some(seq), EventKind::TurnEnd, json!({"stop_reason": stop_reason}), Some(ConversationStatus::Review)).await?;

    if !inserted {
        return Ok(());
    }

    record_turn(state, conversation_id, usage.as_ref()).await?;
    sqlx::query("update conversations set limit_resumes = 0 where id = $1 and limit_resumes > 0").bind(conversation_id).execute(&state.pool).await?;
    state.hub.forget_prompts_of(conversation_id);
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
    crate::moves::turn_ended(state, owner, conversation_id).await;

    if let Err(error) = crate::goals::turn_ended(state, conversation_id, &stop_reason).await {
        tracing::warn!("goal handling after a turn failed for {conversation_id}: {}", error.message());
    }


    let send_now = state.hub.take_send_now(conversation_id);
    let archived = sqlx::query_scalar::<_, bool>("select archived_at is not null from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;

    if archived {
        state.hub.send(node_id, ControlToNode::CloseSession { conversation_id });
        state.remove_session(node_id, conversation_id).await?;

        return Ok(());
    }

    if stop_reason == "cancelled" && !send_now {
        return Ok(());
    }

    pump(state, conversation_id, false).await
}

async fn store_logins(state: &AppState, node_id: Uuid, logins: Vec<LoginInfo>) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    sqlx::query("delete from logins where node_id = $1").bind(node_id).execute(&mut *tx).await?;

    for login in logins {
        sqlx::query(
            "insert into logins (node_id, agent, account, method, state, usage_left, usage_windows, usage_read_at) values ($1, $2, $3, $4, $5, $6, $7, $8)
             on conflict (node_id, agent, coalesce(account, '')) do update set method = excluded.method, state = excluded.state, usage_left = excluded.usage_left, usage_windows = excluded.usage_windows, usage_read_at = excluded.usage_read_at, updated_at = now()",
        )
        .bind(node_id)
        .bind(enum_text(&login.agent))
        .bind(&login.account)
        .bind(&login.method)
        .bind(enum_text(&login.state))
        .bind(login.usage_left.map(i16::from))
        .bind(sqlx::types::Json(&login.windows))
        .bind(login.usage_read_at)
        .execute(&mut *tx)
        .await?;
        crate::account_usage::record(&mut tx, node_id, &login).await?;
    }

    tx.commit().await?;

    publish_node(state, node_id).await
}

async fn store_transcript(state: &AppState, node_id: Uuid, path: String, offset: i64, data: String) -> ApiResult<()> {
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else {
        tracing::warn!("node {node_id} sent a transcript chunk that is not base64");

        return Ok(());
    };

    let stored = match write_chunk(&state.config.data_dir, node_id, &path, offset, &bytes).await {
        Ok(stored) => stored,
        Err(WriteError::UnsafePath) => {
            tracing::warn!("node {node_id} sent a transcript path outside its folder: {path:?}");

            return Ok(());
        }
        Err(WriteError::Io(cause)) => return Err(crate::error::ApiError::internal(cause)),
    };

    sqlx::query(
        "insert into transcripts (node_id, path, bytes) values ($1, $2, $3)
         on conflict (node_id, path) do update set bytes = excluded.bytes, updated_at = now()",
    )
    .bind(node_id)
    .bind(&path)
    .bind(stored)
    .execute(&state.pool)
    .await?;

    state.hub.send(node_id, ControlToNode::TranscriptAck { path, bytes: stored });

    Ok(())
}
