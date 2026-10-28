use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::nodes::node_offline;
use crate::state::{AppState, Hub};
use crate::store::find_owned;
use axum::Json;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRequestParts, Path, Request, State};
use axum::response::Response;
use base64::Engine;
use chrono::Utc;
use mycelium_proto::api::{Environment, EnvironmentAction, EnvironmentCapability, EnvironmentController, EnvironmentSize, EnvironmentState, StartEnvironmentRequest};
use mycelium_proto::link::ControlToNode;
use mycelium_proto::stream::StreamMessage;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

const ACTION_LOG_LIMIT: usize = 50;
const DEFAULT_SIZE: EnvironmentSize = EnvironmentSize { width: 1280, height: 800 };
const WIDTH_LIMITS: (u32, u32) = (320, 3840);
const HEIGHT_LIMITS: (u32, u32) = (200, 2160);
const CAPABILITY_WAIT: Duration = Duration::from_secs(5);
const FALLBACK_PERSON: &str = "The person";

#[derive(Clone)]
pub struct EnvironmentRecord {
    pub user_id: Uuid,
    pub person: String,
    pub environment: Environment,
    pub actions: VecDeque<EnvironmentAction>,
    pub handed_back: bool,
    pub interrupted: bool,
}

pub enum StreamEvent {
    Data(String),
    Closed,
}

struct EnvironmentStream {
    node_id: Uuid,
    conversation_id: Uuid,
    events: mpsc::UnboundedSender<StreamEvent>,
}

pub struct EnvironmentRegistry {
    capabilities: Mutex<HashMap<Uuid, EnvironmentCapability>>,
    records: Mutex<HashMap<Uuid, EnvironmentRecord>>,
    streams: Mutex<HashMap<Uuid, EnvironmentStream>>,
    tickets: Mutex<HashMap<Uuid, StreamTicket>>,
    capability_arrived: broadcast::Sender<Uuid>,
}

struct StreamTicket {
    conversation_id: Uuid,
    user_id: Uuid,
    expires: std::time::Instant,
}

const TICKET_LIFETIME: std::time::Duration = std::time::Duration::from_secs(60);

impl Default for EnvironmentRegistry {
    fn default() -> Self {
        EnvironmentRegistry { capabilities: Mutex::default(), records: Mutex::default(), streams: Mutex::default(), tickets: Mutex::default(), capability_arrived: broadcast::channel(16).0 }
    }
}

impl EnvironmentRecord {
    pub fn is_live(&self) -> bool {
        matches!(self.environment.state, EnvironmentState::Starting | EnvironmentState::Running)
    }
}

impl Hub {
    pub fn environment_capability_of(&self, node_id: Uuid) -> Option<EnvironmentCapability> {
        self.environments.capabilities.lock().ok().and_then(|capabilities| capabilities.get(&node_id).cloned())
    }

    pub fn record_environment_capability(&self, node_id: Uuid, capability: EnvironmentCapability) -> Vec<EnvironmentRecord> {
        let running: HashMap<Uuid, EnvironmentController> = capability.running.iter().map(|running| (running.conversation_id, running.controller)).collect();

        if let Ok(mut capabilities) = self.environments.capabilities.lock() {
            capabilities.insert(node_id, capability);
        }

        let _ = self.environments.capability_arrived.send(node_id);

        let Ok(mut records) = self.environments.records.lock() else {
            return Vec::new();
        };
        let mut changed = Vec::new();

        for record in records.values_mut().filter(|record| record.environment.node_id == node_id) {
            let reported = running.get(&record.environment.conversation_id);

            match (record.environment.state, reported) {
                (EnvironmentState::Stopped, Some(controller)) if record.interrupted => {
                    record.environment.state = EnvironmentState::Running;
                    record.environment.stopped_at = None;
                    record.environment.controller = *controller;
                    record.interrupted = false;
                    changed.push(record.clone());
                }
                (EnvironmentState::Running, None) => {
                    record.environment.state = EnvironmentState::Stopped;
                    record.environment.stopped_at = Some(Utc::now());
                    changed.push(record.clone());
                }
                _ => {}
            }
        }

        changed
    }

    pub fn subscribe_environment_capability(&self) -> broadcast::Receiver<Uuid> {
        self.environments.capability_arrived.subscribe()
    }

    pub fn begin_environment(&self, record: EnvironmentRecord) {
        if let Ok(mut records) = self.environments.records.lock() {
            records.insert(record.environment.conversation_id, record);
        }
    }

    pub fn environment_of(&self, conversation_id: Uuid) -> Option<EnvironmentRecord> {
        self.environments.records.lock().ok().and_then(|records| records.get(&conversation_id).cloned())
    }

    pub fn change_environment(&self, conversation_id: Uuid, change: impl FnOnce(&mut EnvironmentRecord)) -> Option<EnvironmentRecord> {
        let mut records = self.environments.records.lock().ok()?;
        let record = records.get_mut(&conversation_id)?;

        change(record);

        Some(record.clone())
    }

    pub fn report_environment(&self, node_id: Uuid, conversation_id: Uuid, reported: EnvironmentState, failure: Option<String>, browser: Option<String>) -> Option<EnvironmentRecord> {
        let changed = {
            let mut records = self.environments.records.lock().ok()?;
            let record = records.get_mut(&conversation_id).filter(|record| record.environment.node_id == node_id)?;

            if record.environment.state == EnvironmentState::Stopped {
                return None;
            }

            record.environment.state = reported;

            if reported == EnvironmentState::Stopped {
                record.environment.stopped_at = Some(Utc::now());
            }

            if failure.is_some() {
                record.environment.failure = failure;
            }

            if browser.is_some() {
                record.environment.browser = browser;
            }

            record.clone()
        };

        if matches!(reported, EnvironmentState::Stopped | EnvironmentState::Failed) {
            self.close_environment_streams(|stream| stream.conversation_id == conversation_id);
        }

        Some(changed)
    }

    pub fn lose_environments_of(&self, node_id: Uuid) -> Vec<EnvironmentRecord> {
        if let Ok(mut capabilities) = self.environments.capabilities.lock() {
            capabilities.remove(&node_id);
        }

        self.close_environment_streams(|stream| stream.node_id == node_id);

        let Ok(mut records) = self.environments.records.lock() else {
            return Vec::new();
        };
        let mut lost = Vec::new();

        for record in records.values_mut().filter(|record| record.environment.node_id == node_id && record.is_live()) {
            record.environment.state = EnvironmentState::Stopped;
            record.environment.stopped_at = Some(Utc::now());
            record.interrupted = true;
            lost.push(record.clone());
        }

        lost
    }

    fn issue_environment_ticket(&self, conversation_id: Uuid, user_id: Uuid) -> Uuid {
        let ticket = Uuid::new_v4();

        if let Ok(mut tickets) = self.environments.tickets.lock() {
            let now = std::time::Instant::now();

            tickets.retain(|_, held| held.expires > now);
            tickets.insert(ticket, StreamTicket { conversation_id, user_id, expires: now + TICKET_LIFETIME });
        }

        ticket
    }

    fn redeem_environment_ticket(&self, ticket: Uuid, conversation_id: Uuid) -> Option<Uuid> {
        let held = self.environments.tickets.lock().ok()?.remove(&ticket)?;

        (held.conversation_id == conversation_id && held.expires > std::time::Instant::now()).then_some(held.user_id)
    }

    pub fn open_environment_stream(&self, stream_id: Uuid, node_id: Uuid, conversation_id: Uuid) -> mpsc::UnboundedReceiver<StreamEvent> {
        let (events, received) = mpsc::unbounded_channel();

        if let Ok(mut streams) = self.environments.streams.lock() {
            streams.insert(stream_id, EnvironmentStream { node_id, conversation_id, events });
        }

        received
    }

    pub fn environment_stream_data(&self, node_id: Uuid, stream_id: Uuid, data: String) {
        if let Ok(streams) = self.environments.streams.lock()
            && let Some(stream) = streams.get(&stream_id).filter(|stream| stream.node_id == node_id)
        {
            let _ = stream.events.send(StreamEvent::Data(data));
        }
    }

    pub fn finish_environment_stream(&self, node_id: Uuid, stream_id: Uuid) {
        if let Ok(mut streams) = self.environments.streams.lock()
            && streams.get(&stream_id).is_some_and(|stream| stream.node_id == node_id)
            && let Some(stream) = streams.remove(&stream_id)
        {
            let _ = stream.events.send(StreamEvent::Closed);
        }
    }

    pub fn forget_environment_stream(&self, stream_id: Uuid) {
        if let Ok(mut streams) = self.environments.streams.lock() {
            streams.remove(&stream_id);
        }
    }

    fn close_environment_streams(&self, matching: impl Fn(&EnvironmentStream) -> bool) -> Vec<(Uuid, Uuid)> {
        let Ok(mut streams) = self.environments.streams.lock() else {
            return Vec::new();
        };
        let closing: Vec<Uuid> = streams.iter().filter(|(_, stream)| matching(stream)).map(|(stream_id, _)| *stream_id).collect();
        let mut closed = Vec::new();

        for stream_id in closing {
            if let Some(stream) = streams.remove(&stream_id) {
                let _ = stream.events.send(StreamEvent::Closed);
                closed.push((stream_id, stream.node_id));
            }
        }

        closed
    }
}

pub fn publish_environment(state: &AppState, record: &EnvironmentRecord) {
    state.publish(Some(record.user_id), StreamMessage::Environment(record.environment.clone()));
}

pub fn record_action(state: &AppState, conversation_id: Uuid, label: &str, update: impl FnOnce(&mut EnvironmentRecord)) -> Option<EnvironmentRecord> {
    let action = EnvironmentAction { at: Utc::now(), label: label.to_string() };
    let logged = action.clone();
    let record = state.hub.change_environment(conversation_id, |record| {
        update(record);
        record.actions.push_front(logged);
        record.actions.truncate(ACTION_LOG_LIMIT);
    })?;

    state.publish(Some(record.user_id), StreamMessage::EnvironmentAction { conversation_id, action });
    publish_environment(state, &record);

    Some(record)
}

pub fn finish_environments_of_node(state: &AppState, node_id: Uuid) {
    for record in state.hub.lose_environments_of(node_id) {
        publish_environment(state, &record);
    }
}

fn owned_environment(state: &AppState, conversation_id: Uuid, user_id: Uuid) -> ApiResult<EnvironmentRecord> {
    state.hub.environment_of(conversation_id).filter(|record| record.user_id == user_id).ok_or_else(|| ApiError::not_found("An environment"))
}

fn live_environment(state: &AppState, conversation_id: Uuid, user_id: Uuid) -> ApiResult<EnvironmentRecord> {
    let record = owned_environment(state, conversation_id, user_id)?;

    if !record.is_live() {
        return Err(ApiError::conflict("environment_not_running", "No environment is running for this conversation. Start one first."));
    }

    Ok(record)
}

pub async fn start(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>, request: Option<Json<StartEnvironmentRequest>>) -> ApiResult<Json<Environment>> {
    let row = find_owned(&state.pool, id, user.id).await?;

    if let Some(existing) = state.hub.environment_of(id).filter(EnvironmentRecord::is_live) {
        return Ok(Json(existing.environment));
    }

    if !state.hub.is_online(row.node_id) {
        return Err(node_offline());
    }

    let node_name = sqlx::query_scalar::<_, String>("select name from nodes where id = $1").bind(row.node_id).fetch_one(&state.pool).await?;
    let capability = state.hub.environment_capability_of(row.node_id);

    match capability {
        None => return Err(ApiError::conflict("environment_unavailable", format!("{node_name} has not said whether it can run an environment. Check again from the node page."))),
        Some(capability) if !capability.available => {
            let reason = capability.note.unwrap_or_else(|| format!("Missing on {node_name}: {}.", capability.missing.join(", ")));

            return Err(ApiError::conflict("environment_unavailable", format!("{node_name} cannot run an environment. {reason}")));
        }
        Some(_) => {}
    }

    let requested = request.map(|Json(request)| request).unwrap_or(StartEnvironmentRequest { width: None, height: None });
    let size = EnvironmentSize {
        width: requested.width.map_or(DEFAULT_SIZE.width, |width| width.clamp(WIDTH_LIMITS.0, WIDTH_LIMITS.1)),
        height: requested.height.map_or(DEFAULT_SIZE.height, |height| height.clamp(HEIGHT_LIMITS.0, HEIGHT_LIMITS.1)),
    };
    let person = Some(user.name.trim()).filter(|name| !name.is_empty()).unwrap_or(FALLBACK_PERSON).to_string();
    let record = EnvironmentRecord {
        user_id: user.id,
        person,
        environment: Environment { conversation_id: id, state: EnvironmentState::Starting, controller: EnvironmentController::Agent, node_id: row.node_id, node_name, size, started_at: Utc::now(), stopped_at: None, failure: None, browser: None, target: None },
        actions: VecDeque::new(),
        handed_back: false,
        interrupted: false,
    };

    state.hub.begin_environment(record.clone());

    if !state.hub.send(row.node_id, ControlToNode::StartEnvironment { conversation_id: id, width: size.width, height: size.height }) {
        state.hub.change_environment(id, |record| record.environment.state = EnvironmentState::Stopped);

        return Err(node_offline());
    }

    publish_environment(&state, &record);

    Ok(Json(record.environment))
}

pub async fn get(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Environment>> {
    let record = owned_environment(&state, id, user.id)?;

    Ok(Json(record.environment))
}

pub async fn actions(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<EnvironmentAction>>> {
    let record = owned_environment(&state, id, user.id)?;

    Ok(Json(record.actions.into_iter().collect()))
}

pub async fn take_over(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Environment>> {
    live_environment(&state, id, user.id)?;

    let record = record_action(&state, id, "You took over", |record| record.environment.controller = EnvironmentController::Person);

    record.map(|record| Json(record.environment)).ok_or_else(|| ApiError::not_found("An environment"))
}

pub async fn hand_back(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Environment>> {
    live_environment(&state, id, user.id)?;

    let record = record_action(&state, id, "You handed back", |record| {
        record.environment.controller = EnvironmentController::Agent;
        record.handed_back = true;
    });

    record.map(|record| Json(record.environment)).ok_or_else(|| ApiError::not_found("An environment"))
}

pub async fn stop(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Environment>> {
    let record = owned_environment(&state, id, user.id)?;

    if !record.is_live() {
        return Ok(Json(record.environment));
    }

    let stopped = state.hub.change_environment(id, |record| {
        record.environment.state = EnvironmentState::Stopped;
        record.environment.stopped_at = Some(Utc::now());
    });

    state.hub.send(record.environment.node_id, ControlToNode::StopEnvironment { conversation_id: id });

    for (stream_id, node_id) in state.hub.close_environment_streams(|stream| stream.conversation_id == id) {
        state.hub.send(node_id, ControlToNode::CloseEnvironmentStream { stream_id });
    }

    let stopped = stopped.unwrap_or(record);
    publish_environment(&state, &stopped);

    Ok(Json(stopped.environment))
}

pub async fn check(State(state): State<AppState>, _user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<Option<EnvironmentCapability>>> {
    if !state.hub.is_online(id) {
        return Err(node_offline());
    }

    let mut arrived = state.hub.subscribe_environment_capability();

    if !state.hub.send(id, ControlToNode::CheckEnvironment) {
        return Err(node_offline());
    }

    let waiting = async {
        loop {
            match arrived.recv().await {
                Ok(node_id) if node_id == id => break,
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    let _ = tokio::time::timeout(CAPABILITY_WAIT, waiting).await;

    Ok(Json(state.hub.environment_capability_of(id)))
}

pub async fn stream_ticket(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> ApiResult<Json<serde_json::Value>> {
    live_environment(&state, id, user.id)?;

    Ok(Json(serde_json::json!({"ticket": state.hub.issue_environment_ticket(id, user.id)})))
}

pub async fn stream(State(state): State<AppState>, Path(id): Path<Uuid>, request: Request) -> ApiResult<Response> {
    let (mut parts, _) = request.into_parts();
    let ticket = parts.uri.query().and_then(|query| url::form_urlencoded::parse(query.as_bytes()).find(|(key, _)| key == "ticket").and_then(|(_, value)| Uuid::parse_str(&value).ok()));
    let user_id = match ticket {
        Some(ticket) => state.hub.redeem_environment_ticket(ticket, id).ok_or_else(|| ApiError::new(axum::http::StatusCode::UNAUTHORIZED, "ticket_expired", "The live view link has expired. Close the panel and open it again."))?,
        None => AuthUser::from_request_parts(&mut parts, &state).await?.id,
    };

    crate::stream::check_origin(&state, &parts.headers)?;

    let upgrade = WebSocketUpgrade::from_request_parts(&mut parts, &state).await.map_err(|_| ApiError::bad_request("websocket_required", "Open this address as a WebSocket to watch the environment."))?;
    let record = live_environment(&state, id, user_id)?;
    let node_id = record.environment.node_id;

    if !state.hub.is_online(node_id) {
        return Err(node_offline());
    }

    let stream_id = Uuid::new_v4();
    let events = state.hub.open_environment_stream(stream_id, node_id, id);

    if !state.hub.send(node_id, ControlToNode::OpenEnvironmentStream { stream_id, conversation_id: id }) {
        state.hub.forget_environment_stream(stream_id);

        return Err(node_offline());
    }

    Ok(upgrade.protocols(["binary"]).on_upgrade(move |socket| proxy(state, node_id, stream_id, events, socket)))
}

async fn proxy(state: AppState, node_id: Uuid, stream_id: Uuid, mut events: mpsc::UnboundedReceiver<StreamEvent>, mut socket: WebSocket) {
    let encoding = base64::engine::general_purpose::STANDARD;

    loop {
        tokio::select! {
            event = events.recv() => match event {
                Some(StreamEvent::Data(data)) => {
                    let Ok(bytes) = encoding.decode(data) else {
                        continue;
                    };

                    if socket.send(Message::Binary(bytes.into())).await.is_err() {
                        break;
                    }
                }
                Some(StreamEvent::Closed) | None => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Binary(bytes))) => {
                    let data = encoding.encode(&bytes);

                    if !state.hub.send(node_id, ControlToNode::EnvironmentStreamInput { stream_id, data }) {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
    }

    state.hub.forget_environment_stream(stream_id);
    state.hub.send(node_id, ControlToNode::CloseEnvironmentStream { stream_id });
}
