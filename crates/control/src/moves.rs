use crate::assistants;
use crate::auth::AuthUser;
use crate::conversations::require_signed_in_account;
use crate::error::{ApiError, ApiResult};
use crate::forges;
use crate::git::relay;
use crate::instructions;
use crate::nodes::{node_has, node_offline};
use crate::projects::{self, CHECKOUT_COLUMNS, CheckoutRow, PROJECT_COLUMNS, ProjectRow};
use crate::prompts::pump;
use crate::state::AppState;
use crate::store::{CONVERSATION_COLUMNS, ConversationRow, TURN_HAS_ENDED, enum_text, find_owned, highest_node_seq, parse_enum};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use base64::Engine;
use mycelium_proto::api::{MovePlan, MoveProgress, MoveRequest, MoveState};
use mycelium_proto::conversation::AgentId;
use mycelium_proto::link::{ControlToNode, GitCommand, ResourceSample};
use mycelium_proto::stream::StreamMessage;
use serde::Deserialize;
use std::time::Duration;
use tokio::time::Instant;
use uuid::Uuid;

const RESOURCES_WAIT: Duration = Duration::from_secs(3);
const TRANSCRIPT_WAIT: Duration = Duration::from_secs(20);
const SESSION_START_WAIT: Duration = Duration::from_secs(90);
const POLL: Duration = Duration::from_millis(100);
const TRANSCRIPT_CHUNK_BYTES: usize = 192 * 1024;
const WATCH_SECONDS: u32 = 10;
const ASSISTANT_MOVE_SINCE: (u64, u64, u64) = (0, 31, 0);

#[derive(Deserialize)]
pub struct PlanQuery {
    node_id: Uuid,
}

struct Route {
    source: ConversationRow,
    source_node_name: String,
    source_project: Option<ProjectRow>,
    source_checkout: Option<CheckoutRow>,
    target_checkout: Option<CheckoutRow>,
    assistant_id: Option<Uuid>,
}

async fn route(state: &AppState, owner: Uuid, id: Uuid, target_node_id: Uuid) -> ApiResult<Route> {
    let source = find_owned(&state.pool, id, owner).await?;
    let draining = sqlx::query_scalar::<_, bool>("select draining from nodes where id = $1 and revoked_at is null").bind(target_node_id).fetch_optional(&state.pool).await?;

    match draining {
        None => return Err(ApiError::not_found("That node")),
        Some(true) => return Err(ApiError::conflict("node_draining", "This node is finishing its running conversations and is not taking new ones. Choose another node.")),
        Some(false) => {}
    }

    if source.node_id == target_node_id {
        return Err(ApiError::conflict("same_node", "This conversation is already on that node. Choose another node."));
    }

    if !state.hub.is_online(target_node_id) || !state.hub.is_online(source.node_id) {
        return Err(node_offline());
    }

    let available = sqlx::query_scalar::<_, bool>("select available from agents where node_id = $1 and agent = $2").bind(target_node_id).bind(&source.agent).fetch_optional(&state.pool).await?;

    if available == Some(false) {
        return Err(ApiError::conflict("agent_unavailable", format!("{} is not installed on that node. Install it there or choose another node.", source.agent)));
    }

    if let Some(account) = &source.account {
        require_signed_in_account(state, target_node_id, parse_enum(&source.agent)?, account).await?;
    }

    let assistant_id = source.assistant_id.filter(|assistant_id| source.cwd == assistants::folder_of(*assistant_id));

    if assistant_id.is_some() {
        for node_id in [source.node_id, target_node_id] {
            let (name, version) = sqlx::query_as::<_, (String, String)>("select name, version from nodes where id = $1").bind(node_id).fetch_one(&state.pool).await?;

            if !node_has(&version, ASSISTANT_MOVE_SINCE) {
                return Err(ApiError::conflict("node_outdated", format!("Update {name} to move an assistant's conversation.")));
            }
        }
    }

    let source_node_name = sqlx::query_scalar::<_, String>("select name from nodes where id = $1").bind(source.node_id).fetch_one(&state.pool).await?;
    let recorded = sqlx::query_scalar::<_, Option<Uuid>>("select project_id from conversations where id = $1").bind(id).fetch_one(&state.pool).await?;
    let on_source = sqlx::query_as::<_, ProjectRow>(sqlx::AssertSqlSafe(format!("select {PROJECT_COLUMNS} from projects where owner = $1 and exists (select 1 from project_checkouts where project_id = projects.id and node_id = $2)")))
        .bind(owner)
        .bind(source.node_id)
        .fetch_all(&state.pool)
        .await?;
    let on_source = projects::with_checkouts(state, on_source).await?;
    let path_on_source = |project: &ProjectRow| project.checkout_on(source.node_id).map(|checkout| checkout.path.clone()).unwrap_or_default();
    let contains = |project: &ProjectRow| {
        let path = path_on_source(project);

        source.cwd == path || source.cwd.starts_with(&format!("{path}/")) || source.cwd.starts_with(&format!("{path}-worktrees/"))
    };
    let source_project = on_source.iter().find(|project| Some(project.id) == recorded).or_else(|| on_source.iter().filter(|project| contains(project)).max_by_key(|project| path_on_source(project).len())).cloned();
    let source_checkout = source_project.as_ref().and_then(|project| project.checkout_on(source.node_id)).cloned();

    let target_checkout = match source_project.as_ref().and_then(|project| project.forge_id.zip(project.repository.clone())) {
        Some((forge_id, repository)) => {
            sqlx::query_as::<_, CheckoutRow>(sqlx::AssertSqlSafe(format!(
                "select {CHECKOUT_COLUMNS} from project_checkouts where node_id = $2 and project_id in (select id from projects where owner = $1 and forge_id = $3 and lower(repository) = lower($4)) order by created_at, id limit 1",
            )))
            .bind(owner)
            .bind(target_node_id)
            .bind(forge_id)
            .bind(repository)
            .fetch_optional(&state.pool)
            .await?
        }
        None => None,
    };

    Ok(Route { source, source_node_name, source_project, source_checkout, target_checkout, assistant_id })
}

fn stays_behind(sample: &ResourceSample, conversation_id: Uuid, cwd: &str) -> (Vec<u16>, Vec<String>) {
    let inside = |owner: Option<Uuid>, folder: &Option<String>| owner == Some(conversation_id) || folder.as_deref().is_some_and(|folder| folder == cwd || folder.starts_with(&format!("{cwd}/")));
    let mut ports: Vec<u16> = sample.ports.iter().filter(|port| inside(port.conversation_id, &port.cwd)).map(|port| port.port).collect();
    let mut processes: Vec<String> = sample.processes.iter().filter(|process| !process.agent && inside(process.conversation_id, &process.cwd)).map(|process| process.name.clone()).collect();

    ports.sort();
    ports.dedup();
    processes.sort();
    processes.dedup();

    (ports, processes)
}

pub async fn plan(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Query(query): Query<PlanQuery>) -> ApiResult<Json<MovePlan>> {
    let route = route(&state, user.id, id, query.node_id).await?;
    let source = &route.source;

    let latest = match state.hub.resources_of(source.node_id).latest {
        Some(sample) => Some(sample),
        None => {
            state.hub.send(source.node_id, ControlToNode::Watch { seconds: WATCH_SECONDS });
            let deadline = Instant::now() + RESOURCES_WAIT;
            let mut sample = None;

            while sample.is_none() && Instant::now() < deadline {
                tokio::time::sleep(POLL).await;
                sample = state.hub.resources_of(source.node_id).latest;
            }

            sample
        }
    };
    let (stays_ports, stays_processes) = latest.map(|sample| stays_behind(&sample, id, &source.cwd)).unwrap_or_default();

    let repository = route.source_project.as_ref().and_then(|project| project.repository.as_ref());
    let reason = if repository.is_none() && route.assistant_id.is_none() {
        Some("This folder is not a forge repository, so Mycelium cannot find the same project on the other node. Set the project up from a forge first.".to_string())
    } else {
        None
    };

    let planned = match (&reason, route.assistant_id) {
        (None, None) => relay(&state, source.node_id, source.cwd.clone(), GitCommand::MovePlan).await,
        _ => Ok(serde_json::Value::Null),
    };
    let (branch, changed_files, reason) = match (planned, reason) {
        (_, Some(reason)) => (None, 0, Some(reason)),
        (Ok(plan), None) => (plan["branch"].as_str().map(str::to_string), plan["changed_files"].as_u64().unwrap_or(0) as u32, None),
        (Err(error), None) => (None, 0, Some(error.message().to_string())),
    };

    Ok(Json(MovePlan {
        movable: reason.is_none(),
        reason,
        branch,
        changed_files,
        project_on_target: route.target_checkout.is_some(),
        clones_project: route.target_checkout.is_none() && repository.is_some(),
        agent_session: source.acp_session_id.is_some(),
        stays_ports,
        stays_processes,
    }))
}

async fn between_turns(state: &AppState, id: Uuid, status: &str) -> ApiResult<bool> {
    if matches!(status, "review" | "done" | "failed") {
        return Ok(true);
    }

    if status != "input" {
        return Ok(false);
    }

    let turn_ended = sqlx::query_scalar::<_, bool>(sqlx::AssertSqlSafe(format!("select {TURN_HAS_ENDED}"))).bind(id).fetch_one(&state.pool).await?;

    Ok(turn_ended)
}

pub async fn current(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Option<MoveProgress>>> {
    find_owned(&state.pool, id, user.id).await?;

    Ok(Json(state.hub.move_of(id)))
}

pub async fn request(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, Json(request): Json<MoveRequest>) -> ApiResult<(StatusCode, Json<MoveProgress>)> {
    let route = route(&state, user.id, id, request.node_id).await?;
    let busy = !between_turns(&state, id, &route.source.status).await?;
    let state_now = if busy { MoveState::Waiting } else { MoveState::Moving };
    let progress = MoveProgress { conversation_id: id, node_id: request.node_id, fork: request.fork, state: state_now, message: None, result_id: None };

    if !state.hub.begin_move(progress.clone()) {
        return Err(ApiError::conflict("move_in_progress", "This conversation is already moving or waiting to move. Cancel that first to choose another node."));
    }

    state.publish(Some(user.id), StreamMessage::Move(progress.clone()));

    if !busy {
        tokio::spawn(carry_out(state.clone(), user.id, id, progress.clone()));
    }

    Ok((StatusCode::ACCEPTED, Json(progress)))
}

pub async fn cancel(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    find_owned(&state.pool, id, user.id).await?;

    let waiting = state.hub.move_of(id).filter(|progress| progress.state == MoveState::Waiting);

    let Some(waiting) = waiting else {
        return Err(ApiError::conflict("move_not_waiting", "This conversation is not waiting to move, so there is nothing to cancel."));
    };

    announce(&state, user.id, MoveProgress { state: MoveState::Cancelled, ..waiting });

    Ok(StatusCode::NO_CONTENT)
}

pub async fn turn_ended(state: &AppState, owner: Uuid, conversation_id: Uuid) {
    let waiting = state.hub.move_of(conversation_id).filter(|progress| progress.state == MoveState::Waiting);

    let Some(waiting) = waiting else {
        return;
    };

    let moving = MoveProgress { state: MoveState::Moving, ..waiting };
    announce(state, owner, moving.clone());
    tokio::spawn(carry_out(state.clone(), owner, conversation_id, moving));
}

pub fn session_failed(state: &AppState, owner: Uuid, conversation_id: Uuid) {
    let waiting = state.hub.move_of(conversation_id).filter(|progress| progress.state == MoveState::Waiting);

    if let Some(waiting) = waiting {
        announce(state, owner, MoveProgress { state: MoveState::Failed, message: Some("The agent stopped before the turn ended, so nothing moved. Start the move again once the conversation is running.".to_string()), ..waiting });
    }
}

fn announce(state: &AppState, owner: Uuid, progress: MoveProgress) {
    state.hub.set_move(progress.clone());
    state.publish(Some(owner), StreamMessage::Move(progress));
}

async fn carry_out(state: AppState, owner: Uuid, conversation_id: Uuid, progress: MoveProgress) {
    let outcome = transfer(&state, owner, conversation_id, &progress).await;

    let finished = match outcome {
        Ok(result_id) => MoveProgress { state: MoveState::Done, result_id: Some(result_id), ..progress },
        Err(reason) => MoveProgress { state: MoveState::Failed, message: Some(reason), ..progress },
    };

    announce(&state, owner, finished);

    if let Err(error) = pump(&state, conversation_id, false).await {
        tracing::warn!("conversation {conversation_id} could not continue after a move: {}", error.message());
    }
}

async fn transfer(state: &AppState, owner: Uuid, conversation_id: Uuid, progress: &MoveProgress) -> Result<Uuid, String> {
    let route = route(state, owner, conversation_id, progress.node_id).await.map_err(|error| error.message().to_string())?;
    let source_name = route.source_node_name.clone();
    let retry = format!("Nothing has changed: the conversation is still on {source_name}. Choose Move to try again.");
    let reason = |text: &str| format!("{text} {retry}");

    let idle = between_turns(state, conversation_id, &route.source.status).await.map_err(|error| reason(error.message()))?;

    if !idle {
        return Err(reason("The conversation is not between turns."));
    }

    let source = &route.source;
    let target_node_id = progress.node_id;
    let source_project_id = route.source_project.as_ref().map(|project| project.id);

    let (snapshot, target_cwd, transcript_cwd, target_project_id, applied) = match route.assistant_id {
        Some(assistant_id) => {
            let located = relay(state, source.node_id, source.cwd.clone(), GitCommand::Folder { session_id: source.acp_session_id.clone() }).await.map_err(|error| reason(error.message()))?;
            assistants::copy_up(state, owner, source.node_id, assistant_id).await.map_err(|error| reason(error.message()))?;
            assistants::copy_down(state, target_node_id, assistant_id).await.map_err(|error| reason(error.message()))?;
            let target_folder = relay(state, target_node_id, source.cwd.clone(), GitCommand::Folder { session_id: None }).await.map_err(|error| reason(error.message()))?;
            let Some(path) = target_folder["path"].as_str() else {
                return Err(reason("The other node did not say where the assistant's folder is."));
            };

            (located, source.cwd.clone(), path.to_string(), None, None)
        }
        None => {
            let held = route.source_project.clone().zip(route.source_checkout.clone()).and_then(|(project, checkout)| {
                let (forge_id, repository) = project.forge_id.zip(project.repository.clone())?;

                Some((project, checkout, forge_id, repository))
            });
            let Some((source_project, source_checkout, forge_id, repository)) = held else {
                return Err(reason("This folder is not a forge repository, so Mycelium cannot find the same project on the other node."));
            };

            let snapshot = relay(state, source.node_id, source.cwd.clone(), GitCommand::Snapshot { session_id: source.acp_session_id.clone() }).await.map_err(|error| reason(error.message()))?;

            let forge = forges::find_owned(state, forge_id, owner).await.map_err(|error| reason(error.message()))?;
            let target_checkout = match route.target_checkout.clone() {
                Some(checkout) => checkout,
                None => {
                    let path = projects::clone_repository(state, &forge, &repository, target_node_id, None).await.map_err(|error| reason(error.message()))?;

                    projects::add_checkout(state, source_project.id, target_node_id, &path).await.map_err(|error| reason(error.message()))?
                }
            };

            let target_cwd = match source.cwd.strip_prefix(&source_checkout.path) {
                Some(inside) if inside.is_empty() || inside.starts_with('/') => format!("{}{inside}", target_checkout.path),
                _ => target_checkout.path.clone(),
            };

            let apply = GitCommand::Apply {
                branch: snapshot["branch"].as_str().unwrap_or_default().to_string(),
                head: snapshot["head"].as_str().unwrap_or_default().to_string(),
                patch: snapshot["patch"].as_str().unwrap_or_default().to_string(),
                bundle: snapshot["bundle"].as_str().map(str::to_string),
                credentials: Some(forge.git_credentials().map_err(|error| reason(error.message()))?),
                approved_urls: forge.approved_clone_urls(&state.http, &repository).await.map_err(|error| reason(error.message()))?,
            };
            let applied = relay(state, target_node_id, target_checkout.path.clone(), apply).await.map_err(|error| reason(error.message()))?;

            (snapshot, target_cwd.clone(), target_cwd, Some(target_checkout.project_id), Some((target_checkout.path.clone(), applied)))
        }
    };

    let target_name = sqlx::query_scalar::<_, String>("select name from nodes where id = $1").bind(target_node_id).fetch_one(&state.pool).await.map_err(|error| reason(ApiError::from(error).message()))?;

    let rest = async {
        let reason = |text: &str| text.to_string();


        if let Some(session_id) = &source.acp_session_id {
            copy_transcript(state, source, target_node_id, session_id, &snapshot["transcript"], &transcript_cwd).await.map_err(|text| reason(&text))?;
        }

        let agent: AgentId = parse_enum(&source.agent).map_err(|error| reason(error.message()))?;
        let last_event = sqlx::query_scalar::<_, i64>("select coalesce(max(id), 0) from events where conversation_id = $1").bind(conversation_id).fetch_one(&state.pool).await.map_err(|error| reason(ApiError::from(error).message()))?;

        let started = if progress.fork {
            let title = format!("{} (fork)", source.title);
            let forked = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
                "insert into conversations (owner, node_id, agent, cwd, title, status, account, project_id, assistant_id, archived_at) values ($1, $2, $3, $4, $5, 'done', $6, $7, $8, now())
                 returning {CONVERSATION_COLUMNS}",
            )))
            .bind(owner)
            .bind(target_node_id)
            .bind(&source.agent)
            .bind(&target_cwd)
            .bind(&title)
            .bind(&source.account)
            .bind(target_project_id)
            .bind(route.assistant_id)
            .fetch_one(&state.pool)
            .await
            .map_err(|error| reason(ApiError::from(error).message()))?;

            let natively = agent == AgentId::Claude && source.acp_session_id.is_some();
            let instructions = instructions::session_layer(state, forked.id).await.map_err(|error| reason(error.message()))?;
            let env = assistants::secret_env(state, forked.id).await.map_err(|error| reason(error.message()))?;
            let mcp_token = crate::mcp_tokens::mint(state, target_node_id, forked.id).await.map_err(|error| reason(error.message()))?;
            state.hub.send(target_node_id, ControlToNode::StartSession { conversation_id: forked.id, agent, cwd: target_cwd.clone(), resume: source.acp_session_id.clone(), fork: natively, account: source.account.clone(), instructions, reload: false, env, mcp_token });

            match wait_for_session(state, target_node_id, forked.id, &target_name).await {
                Ok(()) => {
                    let shown = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!("update conversations set archived_at = null where id = $1 returning {CONVERSATION_COLUMNS}"))).bind(forked.id).fetch_one(&state.pool).await.map_err(|error| reason(ApiError::from(error).message()))?;
                    state.publish(Some(owner), StreamMessage::Conversation(shown.to_proto().map_err(|error| reason(error.message()))?));

                    Ok(forked.id)
                }
                Err(text) => {
                    state.hub.send(target_node_id, ControlToNode::CloseSession { conversation_id: forked.id });
                    let _ = state.remove_session(target_node_id, forked.id).await;
                    let _ = sqlx::query("delete from conversations where id = $1").bind(forked.id).execute(&state.pool).await;

                    Err(text)
                }
            }
        } else {
            let after = highest_node_seq(&state.pool, conversation_id).await.map_err(|error| reason(ApiError::from(error).message()))?;
            state.hub.send(target_node_id, ControlToNode::ContinueSequence { conversation_id, after });

            sqlx::query("update conversations set node_id = $2, cwd = $3, project_id = $4, updated_at = now() where id = $1").bind(conversation_id).bind(target_node_id).bind(&target_cwd).bind(target_project_id).execute(&state.pool).await.map_err(|error| reason(ApiError::from(error).message()))?;
            let instructions = instructions::session_layer(state, conversation_id).await.map_err(|error| reason(error.message()))?;
            let env = assistants::secret_env(state, conversation_id).await.map_err(|error| reason(error.message()))?;
            let mcp_token = crate::mcp_tokens::mint(state, target_node_id, conversation_id).await.map_err(|error| reason(error.message()))?;
            state.hub.send(target_node_id, ControlToNode::StartSession { conversation_id, agent, cwd: target_cwd.clone(), resume: source.acp_session_id.clone(), fork: false, account: source.account.clone(), instructions, reload: false, env, mcp_token });

            match wait_for_session(state, target_node_id, conversation_id, &target_name).await {
                Ok(()) => {
                    state.hub.send(source.node_id, ControlToNode::CloseSession { conversation_id });
                    let _ = state.remove_session(source.node_id, conversation_id).await;

                    Ok(conversation_id)
                }
                Err(text) => {
                    state.hub.send(target_node_id, ControlToNode::CloseSession { conversation_id });
                    let _ = state.remove_session(target_node_id, conversation_id).await;
                    let _ = sqlx::query("delete from events where conversation_id = $1 and id > $2").bind(conversation_id).bind(last_event).execute(&state.pool).await;
                    let restored = sqlx::query_as::<_, ConversationRow>(sqlx::AssertSqlSafe(format!(
                        "update conversations set node_id = $2, cwd = $3, project_id = $4, status = $5, updated_at = $6 where id = $1 returning {CONVERSATION_COLUMNS}",
                    )))
                    .bind(conversation_id)
                    .bind(source.node_id)
                    .bind(&source.cwd)
                    .bind(source_project_id)
                    .bind(&source.status)
                    .bind(source.updated_at)
                    .fetch_one(&state.pool)
                    .await;

                    match restored.map_err(ApiError::from).and_then(|restored| restored.to_proto()) {
                        Ok(proto) => state.publish(Some(owner), StreamMessage::Conversation(proto)),
                        Err(error) => tracing::warn!("could not put conversation {conversation_id} back after a failed move: {}", error.message()),
                    }

                    Err(text)
                }
            }
        };

        started
    }
    .await;

    let Err(text) = rest else {
        return rest;
    };

    let Some((target_path, applied)) = applied else {
        return Err(reason(&text));
    };

    let undo = GitCommand::Undo {
        branch: snapshot["branch"].as_str().unwrap_or_default().to_string(),
        previous: applied["previous"].as_str().unwrap_or_default().to_string(),
        branch_before: applied["branch_before"].as_str().map(str::to_string),
    };
    let previous = applied["previous"].as_str().unwrap_or("its previous branch");

    match relay(state, target_node_id, target_path.clone(), undo).await {
        Ok(_) => Err(reason(&text)),
        Err(error) => Err(format!("{text} The work from {source_name} could not be removed from the project folder on {target_name}: {}. In {}, run git reset --hard, then git clean -fd, then git checkout {previous}, and delete the branch {} if it did not exist there before. The conversation is still on {source_name}.", error.message(), target_path, snapshot["branch"].as_str().unwrap_or_default())),
    }
}

async fn copy_transcript(state: &AppState, source: &ConversationRow, target_node_id: Uuid, session_id: &str, saved: &serde_json::Value, target_cwd: &str) -> Result<(), String> {
    let (Some(path), Some(bytes)) = (saved["path"].as_str(), saved["bytes"].as_i64()) else {
        return Err(format!("The agent's session {session_id} was not found on its node, so it cannot resume elsewhere."));
    };

    let mirrored = state.config.data_dir.join("transcripts").join(source.node_id.to_string()).join(path);
    let deadline = Instant::now() + TRANSCRIPT_WAIT;

    loop {
        let copied = sqlx::query_scalar::<_, i64>("select bytes from transcripts where node_id = $1 and path = $2").bind(source.node_id).bind(path).fetch_optional(&state.pool).await.map_err(|error| ApiError::from(error).message().to_string())?;

        if copied.is_some_and(|copied| copied >= bytes) {
            break;
        }

        if Instant::now() >= deadline {
            return Err("The conversation's transcript is still being copied from its node. Try again in a moment.".to_string());
        }

        tokio::time::sleep(POLL).await;
    }

    let content = tokio::fs::read(&mirrored).await.map_err(|error| format!("The copy of the transcript could not be read: {error}."))?;
    let target_path = match parse_enum::<AgentId>(&source.agent) {
        Ok(AgentId::Claude) => {
            let file = path.rsplit('/').next().unwrap_or(path);
            let folder: String = target_cwd.chars().map(|character| if character.is_ascii_alphanumeric() { character } else { '-' }).collect();

            format!(".claude/projects/{folder}/{file}")
        }
        Ok(AgentId::Codex) => match path.rsplit_once("/sessions/") {
            Some((_, dated)) => format!(".codex/sessions/{dated}"),
            None => path.to_string(),
        },
        _ => path.to_string(),
    };
    let engine = base64::engine::general_purpose::STANDARD;
    let mut offset = 0usize;

    loop {
        let end = (offset + TRANSCRIPT_CHUNK_BYTES).min(content.len());
        let sent = state.hub.send(target_node_id, ControlToNode::RestoreTranscript { path: target_path.clone(), offset: offset as i64, data: engine.encode(&content[offset..end]) });

        if !sent {
            return Err("The other node went offline while the transcript was being copied.".to_string());
        }

        offset = end;

        if offset >= content.len() {
            return Ok(());
        }
    }
}

async fn wait_for_session(state: &AppState, node_id: Uuid, conversation_id: Uuid, node_name: &str) -> Result<(), String> {
    let deadline = Instant::now() + SESSION_START_WAIT;

    loop {
        if state.hub.has_session(node_id, conversation_id) {
            return Ok(());
        }

        let failed = sqlx::query_scalar::<_, String>("select status from conversations where id = $1").bind(conversation_id).fetch_optional(&state.pool).await.map_err(|error| ApiError::from(error).message().to_string())?;

        if failed.as_deref() == Some(enum_text(&mycelium_proto::conversation::ConversationStatus::Failed).as_str()) {
            let said = sqlx::query_scalar::<_, serde_json::Value>("select payload from events where conversation_id = $1 and kind = 'error' order by id desc limit 1").bind(conversation_id).fetch_optional(&state.pool).await.ok().flatten();
            let message = said.and_then(|payload| payload["message"].as_str().map(str::to_string)).unwrap_or_else(|| "The agent did not say why.".to_string());

            return Err(format!("The agent could not resume on {node_name}: {message}"));
        }

        if Instant::now() >= deadline {
            return Err(format!("The agent did not start on {node_name} in time."));
        }

        tokio::time::sleep(POLL).await;
    }
}
