use crate::activity::record_turn;
use crate::api_access;
use crate::auth::AuthNode;
use crate::device::client_address;
use crate::error::ApiResult;
use crate::imports;
use crate::prompts::pump;
use crate::ssh_access;
use crate::state::{AppState, FolderAnswer, GitAnswer, ProcessAnswer, ResetAnswer, SteerAnswer};
use crate::store::{CONVERSATION_COLUMNS, TURN_HAS_ENDED, ConversationRow, append_event, enum_text, has_live_workflow_agent, highest_node_seq, highest_node_seqs, lock_on_node, merge_queue, publish_node, set_status, without_nul};
use crate::transcripts::{WriteError, write_chunk};
use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use std::net::SocketAddr;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use mycelium_proto::api::HarnessModel;
use mycelium_proto::conversation::{AgentInfo, ConversationStatus, EventKind, LoginInfo, SessionPosition};
use mycelium_proto::link::{ControlToNode, NodeToControl, REPLACED_CLOSE_CODE};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{Instant, interval_at, sleep_until};
use uuid::Uuid;

const PREDICTED_TITLE_LIMIT: usize = 100;
const SESSION_STATE_UPDATES: [&str; 3] = ["available_commands_update", "config_option_update", "current_mode_update"];

const UPDATE_BATCH_WINDOW: Duration = Duration::from_millis(200);
const UPDATE_BATCH_LIMIT: usize = 256;
const TOOL_UPDATE_CACHE_LIMIT: usize = 2048;
const MERGEABLE_CHUNKS: [&str; 2] = ["agent_message_chunk", "agent_thought_chunk"];

const AGENT_ACTIVITY: [&str; 5] = ["agent_message_chunk", "agent_thought_chunk", "tool_call", "tool_call_update", "plan"];
const ACTIVITY_REFRESH_SECONDS: i32 = 5;
const TRAILING_OUTPUT_SECONDS: i32 = 3;

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
    let mut batch = UpdateBatch::default();
    let mut icons_checked = false;

    loop {
        let flush_at = batch.deadline;
        let flush_due = async move {
            match flush_at {
                Some(at) => sleep_until(at).await,
                None => std::future::pending().await,
            }
        };

        tokio::select! {
            _ = flush_due => batch.flush(&state, node_id).await,
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
                        Ok(NodeToControl::AcpUpdate { conversation_id, seq, update }) => {
                            batch.push(conversation_id, seq, update);

                            if batch.len >= UPDATE_BATCH_LIMIT {
                                batch.flush(&state, node_id).await;
                            }
                        }
                        Ok(message) => {
                            batch.flush(&state, node_id).await;

                            let first_hello = matches!(&message, NodeToControl::Hello { .. }) && !icons_checked;

                            if let Err(cause) = handle(&state, node_id, message).await {
                                tracing::warn!("node {node_id} message failed: {cause:?}");
                            } else if first_hello {
                                icons_checked = true;
                                let background = state.clone();

                                tokio::spawn(async move {
                                    let projects = sqlx::query_as::<_, (Uuid, Uuid)>("select owner, id from projects where node_id = $1 and icon is null and avatar_sha256 is null order by created_at")
                                        .bind(node_id)
                                        .fetch_all(&background.pool)
                                        .await;
                                    let Ok(projects) = projects else {
                                        return;
                                    };

                                    for (owner, project_id) in projects {
                                        if background.hub.is_replaced(node_id, connection_id) || !background.hub.is_online(node_id) {
                                            break;
                                        }

                                        crate::projects::find_automatic_icon(&background, owner, project_id).await;
                                    }
                                });
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

    batch.flush(&state, node_id).await;

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
            let mut batch = UpdateBatch::default();
            batch.push(conversation_id, seq, update);
            batch.flush(state, node_id).await;

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
        NodeToControl::SessionClosed { conversation_id } => {
            sqlx::query("update conversations set agent_active = false where id = $1 and node_id = $2 and agent_active").bind(conversation_id).bind(node_id).execute(&state.pool).await?;

            state.remove_session(node_id, conversation_id).await
        }
        NodeToControl::SessionFailed { conversation_id, seq, message } => {
            state.remove_session(node_id, conversation_id).await?;
            state.hub.forget_prompts_of(conversation_id);
            state.hub.take_send_now(conversation_id);
            let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_optional(&state.pool).await?;

            if let Some(owner) = owner {
                crate::moves::session_failed(state, owner, conversation_id);
            }

            let limit = crate::usage_limits::inspect(state, node_id, conversation_id, &message).await?;

            stop_for_failure(state, node_id, conversation_id, owner, Some(seq), &message, limit).await
        }
        NodeToControl::Workflow { conversation_id, run_id, line } => {
            let limit = crate::usage_limits::inspect_workflow_line(state, node_id, conversation_id, &line).await?;
            let mut payload = json!({"run_id": run_id, "line": line, "at": chrono::Utc::now()});

            if let Some(limit) = limit {
                payload["usage_limit"] = limit;
            }

            record(state, node_id, conversation_id, None, EventKind::Workflow, payload, None).await?;

            if !matches!(line["type"].as_str(), Some("result" | "failed" | "ended")) {
                return Ok(());
            }

            let idle = sqlx::query_scalar::<_, bool>(sqlx::AssertSqlSafe(format!("select {TURN_HAS_ENDED} and not coalesce((select agent_active from conversations where id = $1), false)")))
            .bind(conversation_id)
            .fetch_one(&state.pool)
            .await?;

            if idle && !has_agents_out(state, conversation_id).await? {
                settle_background_work(state, conversation_id, ConversationStatus::Working, ConversationStatus::Review).await?;
            }

            Ok(())
        }
        NodeToControl::Logins { logins } => store_logins(state, node_id, logins).await,
        NodeToControl::OrchestratorUsage { agent, account, fine, coarse } => {
            crate::usage_history::store_orchestrator_readings(state, node_id, agent, account.as_deref().unwrap_or_default(), coarse.iter().chain(fine.iter())).await?;

            Ok(())
        }
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
        NodeToControl::Transcript { path, offset, data, size } => store_transcript(state, node_id, path, offset, data, size).await,
        NodeToControl::Resources { sample } => {
            state.hub.record_resources(node_id, sample.clone());
            state.publish(None, StreamMessage::NodeResources { node_id, sample });

            Ok(())
        }
        NodeToControl::Files { request_id, paths } => {
            state.hub.answer(node_id, request_id, paths);

            Ok(())
        }
        NodeToControl::FolderFiles { request_id, files, truncated, error } => {
            state.hub.answer(node_id, request_id, FolderAnswer { files, truncated, error });

            Ok(())
        }
        NodeToControl::GitResult { request_id, ok, output, error } => {
            state.hub.answer(node_id, request_id, GitAnswer { ok, output, error });

            Ok(())
        }
        NodeToControl::Steered { request_id, outcome, error } => {
            if let Some(error) = error {
                tracing::warn!("node {node_id} could not steer the turn: {error}");
            }

            state.hub.answer(node_id, request_id, SteerAnswer { outcome });

            Ok(())
        }
        NodeToControl::UpdateResult { message, .. } => crate::node_updates::record_result(state, node_id, message).await,
        NodeToControl::ProcessStopped { request_id, ok, error } => {
            state.hub.answer(node_id, request_id, ProcessAnswer { ok, error });

            Ok(())
        }
        NodeToControl::ResetUsed { request_id, outcome, error } => {
            state.hub.answer(node_id, request_id, ResetAnswer { outcome, error });

            Ok(())
        }
        NodeToControl::InstructionsHeld { revision, unmanaged } => {
            sqlx::query("update nodes set instructions_revision = $2, instructions_unmanaged = $3 where id = $1").bind(node_id).bind(revision).bind(unmanaged).execute(&state.pool).await?;

            publish_node(state, node_id).await
        }
        NodeToControl::ApiTokenSaved { conversation_id, path } => api_access::token_saved(state, node_id, conversation_id, path).await,
        NodeToControl::SshTokenSaved { request_id, path } => ssh_access::token_saved(state, node_id, request_id, path).await,
        NodeToControl::SshData { session_id, data } => ssh_access::node_data(node_id, session_id, data),
        NodeToControl::SshClosed { session_id } => ssh_access::node_closed(node_id, session_id),
        NodeToControl::Pong => {
            sqlx::query("update nodes set last_seen_at = now() where id = $1 and (last_seen_at is null or last_seen_at < now() - interval '30 seconds')").bind(node_id).execute(&state.pool).await?;

            Ok(())
        }
    }
}

async fn stop_for_failure(state: &AppState, node_id: Uuid, conversation_id: Uuid, owner: Option<Uuid>, seq: Option<i64>, message: &str, limit: Option<crate::usage_limits::Detected>) -> ApiResult<()> {
    let resume_at = limit.as_ref().and_then(|limit| limit.resume_at);
    let switch_to = limit.as_ref().and_then(|limit| limit.switch_to.clone());
    let limited_account = limit.as_ref().and_then(|limit| limit.account.clone());
    let limited_window = limit.as_ref().and_then(|limit| limit.payload["usage_limit"]["window"].as_str().map(str::to_string));

    let hit_limit = limit.is_some();
    let goal_handled = if hit_limit { crate::goals::hold(state, conversation_id).await } else { crate::goals::session_failed(state, conversation_id).await };

    if let Err(error) = goal_handled {
        tracing::warn!("goal handling after a failed session failed for {conversation_id}: {}", error.message());
    }

    let payload = limit.map_or_else(|| json!({"message": message}), |limit| limit.payload);
    let recorded = record(state, node_id, conversation_id, seq, EventKind::Error, payload, Some(ConversationStatus::Failed)).await?;

    let mut switched = false;

    if let Some(owner) = owner.filter(|_| recorded && hit_limit) {
        let until = resume_at.unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::hours(1));

        if crate::assistants::fall_back(state, owner, conversation_id, "usage_limit", Some(until)).await {
            let text = format!("The usage limit was reached on {}. Continue from where you stopped.", limited_account.as_deref().unwrap_or("the account in use"));

            match crate::assistants::carry_on_after_limit(state, owner, conversation_id, &text).await {
                Ok(()) => switched = true,
                Err(error) => tracing::warn!("carrying on {conversation_id} after its fallback failed: {}", error.message()),
            }
        }
    }

    if let Some(owner) = owner.filter(|_| recorded && hit_limit && !switched) {
        let window = limited_window.as_deref().unwrap_or("usage");

        if crate::routing::switch_after_limit(state, owner, conversation_id, window).await {
            let text = format!("The usage limit was reached on {}. Continue from where you stopped.", limited_account.as_deref().unwrap_or("the account in use"));

            match crate::routing::carry_on(state, owner, conversation_id, &text).await {
                Ok(()) => switched = true,
                Err(error) => tracing::warn!("carrying on {conversation_id} after it moved account failed: {}", error.message()),
            }
        }
    }

    if let Some((owner, account)) = owner.zip(switch_to).filter(|_| recorded && !switched) {
        let agent = sqlx::query_scalar::<_, String>("select agent from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await?;
        let target = crate::continuation::Target { agent, account: Some(account), provider_id: None, model: None, node_id: None, cwd: None };
        let moved = crate::continuation::continue_on(state, owner, conversation_id, target, Some("usage_limit")).await;

        match moved {
            Ok(_) => {
                let text = format!("The usage limit was reached on {}. Continue from where you stopped.", limited_account.as_deref().unwrap_or("the account in use"));

                crate::prompts::retry_failed(state, owner, conversation_id).await?;

                if !crate::goals::resumed(state, owner, conversation_id).await? {
                    crate::prompts::continue_unless_queued(state, owner, conversation_id, &text).await?;
                }

                switched = true;
            }
            Err(error) => {
                tracing::warn!("switching {conversation_id} to another account after its usage limit failed: {}", error.message());
            }
        }
    }

    if let Some(at) = resume_at.filter(|_| recorded && !switched) {
        crate::usage_limits::schedule(state, conversation_id, at).await?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn hello(state: &AppState, node_id: Uuid, version: String, hostname: String, os: String, arch: String, agents: Vec<AgentInfo>, sessions: Vec<SessionPosition>) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    sqlx::query("update nodes set version = $2, hostname = $3, os = $4, arch = $5, last_seen_at = now() where id = $1")
        .bind(node_id)
        .bind(&version)
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

    let highest = highest_node_seqs(&state.pool, &conversations).await?;

    for conversation_id in conversations {
        let seq = highest.get(&conversation_id).copied().unwrap_or(0);

        if seq > 0 || live.contains(&conversation_id) {
            state.hub.send(node_id, ControlToNode::Ack { conversation_id, seq });
        }
    }

    for prompt in state.hub.unacknowledged_prompts_on(node_id, &live) {
        let attachments = crate::attachments::for_node(state, &prompt.attachments).await?;
        state.hub.send(node_id, ControlToNode::Prompt { conversation_id: prompt.conversation_id, prompt_id: prompt.prompt_id, text: prompt.text, attachments });
    }

    crate::instructions::push_to(state, node_id, false).await?;
    crate::node_updates::offer_to_an_older_node(state, node_id, &version);
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

        let message = "The agent stopped while this conversation was running, so the last message may not have been handled. Send a message to carry on; it resumes from where it stopped.";
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

fn reported_models(models: Option<&Value>, config_options: Option<&Value>) -> Vec<HarnessModel> {
    let advertised = models.and_then(|models| models["availableModels"].as_array()).into_iter().flatten().filter_map(|model| Some(HarnessModel { id: model["modelId"].as_str()?.to_string(), name: model["name"].as_str().unwrap_or_else(|| model["modelId"].as_str().unwrap_or_default()).to_string() }));
    let mut found: Vec<HarnessModel> = advertised.collect();

    if found.is_empty() {
        let option = config_options.and_then(Value::as_array).into_iter().flatten().find(|option| option["id"] == "model" || option["category"] == "model");
        let choices = option.and_then(|option| option["options"].as_array()).into_iter().flatten();
        found = choices.filter_map(|choice| Some(HarnessModel { id: choice["value"].as_str()?.to_string(), name: choice["name"].as_str().unwrap_or_else(|| choice["value"].as_str().unwrap_or_default()).to_string() })).collect();
    }

    found
}

async fn session_started(state: &AppState, node_id: Uuid, conversation_id: Uuid, acp_session_id: String, modes: Option<Value>, models: Option<Value>, config_options: Option<Value>) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    let Some(before) = lock_on_node(&mut tx, conversation_id, node_id).await? else {
        return Ok(());
    };

    let mode = modes.as_ref().and_then(|modes| modes["currentModeId"].as_str()).map(str::to_string);
    let model = models.as_ref().and_then(|models| models["currentModelId"].as_str()).map(str::to_string);
    let models_value = models.clone();
    let config_value = config_options.clone();
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
    let reported = reported_models(models_value.as_ref(), config_value.as_ref());

    if !reported.is_empty() {
        sqlx::query(
            "insert into harness_models (node_id, agent, account, models, default_model, reported_at) values ($1, $2, $3, $4, $5, now())
             on conflict (node_id, agent, account) do update set models = excluded.models, default_model = excluded.default_model, reported_at = now()",
        )
        .bind(node_id)
        .bind(&updated.agent)
        .bind(updated.account.clone().unwrap_or_default())
        .bind(sqlx::types::Json(&reported))
        .bind(models_value.as_ref().and_then(|models| models["currentModelId"].as_str()))
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    if !reported.is_empty() {
        publish_node(state, node_id).await?;
    }

    state.add_session(node_id, conversation_id).await?;
    state.publish(Some(updated.owner), StreamMessage::Conversation(updated.to_proto()?));
    crate::conversations::spawn_branch_refresh(state, conversation_id);

    if before.acp_session_id.is_none() {
        crate::assistants::session_started(state, updated.owner, conversation_id).await;

        if let Some(chosen) = before.model.as_deref() && updated.model.as_deref() != Some(chosen) && let Err(error) = crate::prompts::apply_model(state, updated.owner, conversation_id, Some(chosen), None).await {
            tracing::warn!("could not set the model {chosen} chosen for conversation {conversation_id}: {}", error.message());
        }

        if before.mode.is_none() && let Err(error) = crate::agent_settings::apply_default_mode(state, updated.owner, conversation_id).await {
            tracing::warn!("could not set the default mode of conversation {conversation_id}: {}", error.message());
        }
    }

    pump(state, conversation_id, before.acp_session_id.is_none()).await
}

async fn record(state: &AppState, node_id: Uuid, conversation_id: Uuid, seq: Option<i64>, kind: EventKind, payload: Value, transition: Option<ConversationStatus>) -> ApiResult<bool> {
    let mut tx = state.pool.begin().await?;
    let Some(row) = lock_on_node(&mut tx, conversation_id, node_id).await? else {
        return Ok(false);
    };

    let event = append_event(&mut tx, conversation_id, kind, &payload, seq).await?;
    let mut updated = None;

    if event.is_some() && kind == EventKind::TurnEnd {
        sqlx::query("update conversations set agent_active = false where id = $1").bind(conversation_id).execute(&mut *tx).await?;
    }

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

#[derive(Default)]
struct UpdateBatch {
    pending: HashMap<Uuid, Vec<(i64, Value)>>,
    last_tool_updates: HashMap<(Uuid, String), Value>,
    deadline: Option<Instant>,
    len: usize,
}

impl UpdateBatch {
    fn push(&mut self, conversation_id: Uuid, seq: i64, update: Value) {
        self.pending.entry(conversation_id).or_default().push((seq, update));
        self.len += 1;
        self.deadline.get_or_insert_with(|| Instant::now() + UPDATE_BATCH_WINDOW);
    }

    async fn flush(&mut self, state: &AppState, node_id: Uuid) {
        self.deadline = None;
        self.len = 0;

        for (conversation_id, items) in std::mem::take(&mut self.pending) {
            if let Err(cause) = self.write(state, node_id, conversation_id, items).await {
                tracing::warn!("node {node_id} update batch failed: {cause:?}");
            }
        }
    }

    async fn write(&mut self, state: &AppState, node_id: Uuid, conversation_id: Uuid, items: Vec<(i64, Value)>) -> ApiResult<()> {
        let (updates, newest, tool_updates) = coalesce_updates(conversation_id, items, &self.last_tool_updates);
        let mut tx = state.pool.begin().await?;
        let Some(row) = lock_on_node(&mut tx, conversation_id, node_id).await? else {
            return Ok(());
        };

        let mut stored = Vec::with_capacity(updates.len());

        for (seq, update) in updates {
            let event = append_event(&mut tx, conversation_id, EventKind::AcpUpdate, &update, Some(seq)).await?;

            if let Some(event) = event {
                stored.push((event, update));
            }
        }

        let active = if stored.iter().any(|(_, update)| is_agent_activity(update)) { note_agent_activity(&mut tx, conversation_id).await? } else { None };
        tx.commit().await?;

        for (_, update) in &stored {
            if let Err(cause) = crate::skills::count_agent(&state.pool, row.owner, conversation_id, &row.cwd, update, chrono::Utc::now().date_naive()).await {
                tracing::warn!("counting skill lines for {conversation_id} failed: {cause:?}");
            }
        }

        if self.last_tool_updates.len() > TOOL_UPDATE_CACHE_LIMIT {
            self.last_tool_updates.clear();
        }

        for (tool_call_id, update) in tool_updates {
            self.last_tool_updates.insert((conversation_id, tool_call_id), update);
        }

        let highest = highest_node_seq(&state.pool, conversation_id).await?;
        state.hub.send(node_id, ControlToNode::Ack { conversation_id, seq: highest.max(newest) });

        if let Some(active) = active.filter(|active| !row.agent_active || active.status != row.status) {
            state.publish(Some(row.owner), StreamMessage::Conversation(active.to_proto()?));
        }

        for (event, update) in stored {
            state.publish(Some(row.owner), StreamMessage::Event(event));
            after_update(state, node_id, conversation_id, update).await?;
        }

        Ok(())
    }
}

fn is_agent_activity(update: &Value) -> bool {
    update["update"]["sessionUpdate"].as_str().is_some_and(|kind| AGENT_ACTIVITY.contains(&kind))
}

async fn note_agent_activity(tx: &mut crate::store::Tx<'_>, conversation_id: Uuid) -> ApiResult<Option<ConversationRow>> {
    let updated = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
        "update conversations set agent_active = true, last_activity_at = now(),
             updated_at = case when status in ('review', 'done') then now() else updated_at end,
             status = case when status in ('review', 'done') then 'working' else status end
         where id = $1 and (not agent_active or last_activity_at is null or last_activity_at < now() - make_interval(secs => {ACTIVITY_REFRESH_SECONDS}))
           and (agent_active or not exists (
               select 1 from events where conversation_id = $1 and kind = 'turn_end' and created_at > now() - make_interval(secs => {TRAILING_OUTPUT_SECONDS})
                 and seq = (select max(seq) from events where conversation_id = $1 and kind in ('turn_end', 'user_message'))))
         returning {CONVERSATION_COLUMNS}",
    )))
    .bind(conversation_id)
    .fetch_optional(&mut **tx)
    .await?;

    Ok(updated)
}

fn coalesce_updates(conversation_id: Uuid, items: Vec<(i64, Value)>, last_tool_updates: &HashMap<(Uuid, String), Value>) -> (Vec<(i64, Value)>, i64, HashMap<String, Value>) {
    let mut kept: Vec<(i64, Value)> = Vec::new();
    let mut seen = HashSet::new();
    let mut tool_updates: HashMap<String, Value> = HashMap::new();
    let mut newest = 0;

    for (seq, update) in items {
        newest = newest.max(seq);

        if !seen.insert(seq) {
            continue;
        }

        if update["update"]["sessionUpdate"] == "tool_call_update"
            && let Some(tool_call_id) = update["update"]["toolCallId"].as_str()
        {
            let previous = tool_updates.get(tool_call_id).or_else(|| last_tool_updates.get(&(conversation_id, tool_call_id.to_string())));

            if previous == Some(&update["update"]) {
                continue;
            }

            tool_updates.insert(tool_call_id.to_string(), update["update"].clone());
        }

        if let Some((last_seq, last)) = kept.last_mut()
            && seq > *last_seq
            && merge_chunk(last, &update)
        {
            *last_seq = seq;
            continue;
        }

        kept.push((seq, update));
    }

    (kept, newest, tool_updates)
}

fn merge_chunk(last: &mut Value, next: &Value) -> bool {
    let kind = &last["update"]["sessionUpdate"];

    if kind != &next["update"]["sessionUpdate"] || !kind.as_str().is_some_and(|kind| MERGEABLE_CHUNKS.contains(&kind)) {
        return false;
    }

    let (Some(head), Some(tail)) = (last.pointer("/update/content/text").and_then(Value::as_str), next.pointer("/update/content/text").and_then(Value::as_str)) else {
        return false;
    };

    let joined = format!("{head}{tail}");

    if without_text(last) != without_text(next) {
        return false;
    }

    if let Some(text) = last.pointer_mut("/update/content/text") {
        *text = Value::String(joined);
    }

    true
}

fn without_text(update: &Value) -> Value {
    let mut copy = update.clone();

    if let Some(text) = copy.pointer_mut("/update/content/text") {
        *text = Value::Null;
    }

    copy
}

async fn after_update(state: &AppState, node_id: Uuid, conversation_id: Uuid, update: Value) -> ApiResult<()> {
    if update["update"]["sessionUpdate"] == "usage_update" {
        crate::goals::refresh(state, conversation_id).await?;
    }

    let predicted = update["update"]["title"].as_str().filter(|_| update["update"]["sessionUpdate"] == "session_info_update").map(|title| title.trim().chars().take(PREDICTED_TITLE_LIMIT).collect::<String>()).filter(|title| !title.is_empty());
    let mut session_update = update["update"].clone();
    without_nul(&mut session_update);

    if SESSION_STATE_UPDATES.contains(&session_update["sessionUpdate"].as_str().unwrap_or_default()) {
        sqlx::query("insert into conversation_session_state (conversation_id, update_kind, update) values ($1, $2, $3) on conflict (conversation_id, update_kind) do update set update = excluded.update")
            .bind(conversation_id)
            .bind(session_update["sessionUpdate"].as_str())
            .bind(&session_update)
            .execute(&state.pool)
            .await?;
    }

    let Some(title) = predicted else {
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

async fn has_agents_out(state: &AppState, conversation_id: Uuid) -> ApiResult<bool> {
    let out = sqlx::query_scalar::<_, bool>(sqlx::AssertSqlSafe(format!("select {}", has_live_workflow_agent("$1")))).bind(conversation_id).fetch_one(&state.pool).await?;

    Ok(out)
}

async fn settle_background_work(state: &AppState, conversation_id: Uuid, from: ConversationStatus, to: ConversationStatus) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    let current = sqlx::query_scalar::<_, String>("select status from conversations where id = $1 for update").bind(conversation_id).fetch_one(&mut *tx).await?;

    if current != enum_text(&from) {
        return Ok(());
    }

    let updated = set_status(&mut tx, conversation_id, to).await?;
    tx.commit().await?;
    state.publish(Some(updated.owner), StreamMessage::Conversation(updated.to_proto()?));

    Ok(())
}

async fn turn_end(state: &AppState, node_id: Uuid, conversation_id: Uuid, seq: i64, stop_reason: String, usage: Option<Value>) -> ApiResult<()> {
    let keeps_working = stop_reason != "cancelled" && has_agents_out(state, conversation_id).await?;
    let status = if keeps_working { ConversationStatus::Working } else { ConversationStatus::Review };
    let inserted = record(state, node_id, conversation_id, Some(seq), EventKind::TurnEnd, json!({"stop_reason": stop_reason}), Some(status)).await?;

    if !inserted {
        return Ok(());
    }

    if let Err(error) = record_turn(state, conversation_id, usage.as_ref()).await {
        tracing::warn!("could not record the turn of {conversation_id}: {}", error.message());
    }

    crate::conversations::spawn_branch_refresh(state, conversation_id);

    if stop_reason != "cancelled"
        && let Some((message, limit)) = crate::usage_limits::inspect_reply(state, node_id, conversation_id).await?
    {
        let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_optional(&state.pool).await?;

        state.hub.forget_prompts_of(conversation_id);

        return stop_for_failure(state, node_id, conversation_id, owner, None, &message, Some(limit)).await;
    }

    if let Err(error) = sqlx::query("update conversations set limit_resumes = 0 where id = $1 and limit_resumes > 0").bind(conversation_id).execute(&state.pool).await {
        tracing::warn!("could not reset the limit resumes of {conversation_id}: {error}");
    }

    state.hub.forget_prompts_of(conversation_id);

    match sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(conversation_id).fetch_one(&state.pool).await {
        Ok(owner) => {
            crate::moves::turn_ended(state, owner, conversation_id).await;
            crate::assistants::workspace_turn_ended(state, owner, node_id, conversation_id);
        }
        Err(error) => tracing::warn!("could not find the owner of {conversation_id} after its turn: {error}"),
    }

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

    if !send_now {
        let mut connection = state.pool.acquire().await?;

        if let Err(error) = merge_queue(&mut connection, conversation_id).await {
            tracing::warn!("could not combine the queued messages of {conversation_id}: {error:?}");
        }
    }

    pump(state, conversation_id, false).await
}

async fn store_logins(state: &AppState, node_id: Uuid, logins: Vec<LoginInfo>) -> ApiResult<()> {
    let mut tx = state.pool.begin().await?;
    sqlx::query("delete from logins where node_id = $1").bind(node_id).execute(&mut *tx).await?;

    for mut login in logins {
        crate::account_usage::cache(&mut tx, node_id, &mut login).await?;
        sqlx::query(
            "insert into logins (node_id, agent, account, method, state, usage_left, usage_windows, usage_read_at, reset_credits) values ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             on conflict (node_id, agent, coalesce(account, '')) do update set method = excluded.method, state = excluded.state, usage_left = excluded.usage_left, usage_windows = excluded.usage_windows, usage_read_at = excluded.usage_read_at, reset_credits = excluded.reset_credits, updated_at = now()",
        )
        .bind(node_id)
        .bind(enum_text(&login.agent))
        .bind(&login.account)
        .bind(&login.method)
        .bind(enum_text(&login.state))
        .bind(login.usage_left.map(i16::from))
        .bind(sqlx::types::Json(&login.windows))
        .bind(login.usage_read_at)
        .bind(login.reset_credits.as_ref().map(sqlx::types::Json))
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    let goals: Vec<Uuid> = sqlx::query_scalar("select g.conversation_id from goals g join conversations c on c.id = g.conversation_id where c.node_id = $1").bind(node_id).fetch_all(&state.pool).await?;

    for conversation in goals {
        crate::goals::refresh(state, conversation).await?;
    }

    publish_node(state, node_id).await
}

const FULL_TRANSCRIPT_CHUNK_BYTES: i64 = 256 * 1024;
const TRANSCRIPT_ROW_REFRESH_SECONDS: f64 = 3.0;

async fn store_transcript(state: &AppState, node_id: Uuid, path: String, offset: i64, data: String, size: Option<i64>) -> ApiResult<()> {
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else {
        tracing::warn!("node {node_id} sent a transcript chunk that is not base64");

        return Ok(());
    };

    let chunk_bytes = bytes.len() as i64;
    let stored = match write_chunk(&state.config.data_dir, node_id, &path, offset, &bytes).await {
        Ok(stored) => stored,
        Err(WriteError::UnsafePath) => {
            tracing::warn!("node {node_id} sent a transcript path outside its folder: {path:?}");

            return Ok(());
        }
        Err(WriteError::Io(cause)) => return Err(crate::error::ApiError::internal(cause)),
    };

    sqlx::query(
        "insert into transcripts (node_id, path, bytes, size_bytes) values ($1, $2, $3, $7)
         on conflict (node_id, path) do update set bytes = excluded.bytes, size_bytes = excluded.size_bytes, updated_at = now()
         where $4 < $5 or transcripts.updated_at < now() - make_interval(secs => $6)",
    )
    .bind(node_id)
    .bind(&path)
    .bind(stored)
    .bind(chunk_bytes)
    .bind(FULL_TRANSCRIPT_CHUNK_BYTES)
    .bind(TRANSCRIPT_ROW_REFRESH_SECONDS)
    .bind(size)
    .execute(&state.pool)
    .await?;

    state.hub.send(node_id, ControlToNode::TranscriptAck { path: path.clone(), bytes: stored });

    if let Err(error) = crate::live_usage::record(state, node_id, &path, offset, stored).await {
        tracing::warn!("live transcript usage could not be read: {}", error.message());
    }


    if size.is_none_or(|size| stored >= size) {
        imports::sync_subagent(state, node_id, &path).await;
    }

    Ok(())
}
