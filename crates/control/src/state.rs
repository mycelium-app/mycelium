use crate::config::Config;
use crate::error::ApiResult;
use crate::forge_cache::ForgeCache;
use crate::rate_limit::RateLimiter;
use crate::store::publish_node;
use crate::versions::LatestVersions;
use mycelium_proto::api::{CpuPoint, MoveProgress, MoveState, NodeResources};
use mycelium_proto::link::{ControlToNode, FolderFile, ResourceSample};
use serde_json::Value;
use mycelium_proto::stream::StreamMessage;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Streamed {
    pub owner: Option<Uuid>,
    pub message: StreamMessage,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Readiness {
    Migrating,
    Ready,
    Failed,
}

struct NodeConnection {
    connection_id: Uuid,
    sender: mpsc::UnboundedSender<ControlToNode>,
    sessions: HashSet<Uuid>,
    greeted: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct LoginOwner {
    pub node_id: Uuid,
    pub user_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct UnacknowledgedPrompt {
    pub node_id: Uuid,
    pub conversation_id: Uuid,
    pub prompt_id: Uuid,
    pub text: String,
    pub attachments: Vec<mycelium_proto::conversation::Attachment>,
}

const RESOURCE_HISTORY: usize = 200;

#[derive(Default)]
struct WatchedResources {
    latest: Option<ResourceSample>,
    history: VecDeque<CpuPoint>,
}

pub struct GitAnswer {
    pub ok: bool,
    pub output: Value,
    pub error: Option<String>,
}

pub struct SteerAnswer {
    pub outcome: String,
}

const OAUTH_SIGN_IN_LIFETIME: std::time::Duration = std::time::Duration::from_secs(600);

pub struct PendingOAuth {
    pub owner: Uuid,
    pub name: String,
    pub url: String,
    pub settings: Value,
    pub client_id: String,
    pub token_endpoint: String,
    pub verifier: String,
    pub forge_id: Option<Uuid>,
    pub created: std::time::Instant,
}

pub struct PendingLastFm {
    pub owner: Uuid,
    pub server: Uuid,
    pub created: std::time::Instant,
}

pub struct PendingFreeAgent {
    pub owner: Uuid,
    pub server: Uuid,
    pub created: std::time::Instant,
}

pub struct ProcessAnswer {
    pub ok: bool,
    pub error: Option<String>,
}

pub struct FolderAnswer {
    pub files: Vec<FolderFile>,
    pub truncated: bool,
    pub error: Option<String>,
}

pub struct ResetAnswer {
    pub outcome: Option<String>,
    pub error: Option<String>,
}

pub struct PendingResponse {
    pub held: crate::response_editor::Held,
    pub answer: oneshot::Sender<Option<serde_json::Value>>,
}

pub struct PendingRequests<A> {
    waiting: Mutex<HashMap<Uuid, (Uuid, oneshot::Sender<A>)>>,
}

impl<A> Default for PendingRequests<A> {
    fn default() -> Self {
        PendingRequests { waiting: Mutex::new(HashMap::new()) }
    }
}

impl<A> PendingRequests<A> {
    fn register(&self, node_id: Uuid) -> (Uuid, oneshot::Receiver<A>) {
        let request_id = Uuid::new_v4();
        let (answer, answered) = oneshot::channel();

        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.insert(request_id, (node_id, answer));
        }

        (request_id, answered)
    }

    fn answer(&self, node_id: Uuid, request_id: Uuid, answer: A) {
        let waiting = self.waiting.lock().ok().and_then(|mut waiting| match waiting.get(&request_id) {
            Some((owner, _)) if *owner == node_id => waiting.remove(&request_id),
            _ => None,
        });

        if let Some((_, waiter)) = waiting {
            let _ = waiter.send(answer);
        }
    }

    fn forget(&self, request_id: Uuid) {
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.remove(&request_id);
        }
    }
}

pub trait NodeAnswer: Sized {
    fn pending(hub: &Hub) -> &PendingRequests<Self>;
}

macro_rules! node_answers {
    ($($answer:ty => $field:ident),* $(,)?) => {
        $(
            impl NodeAnswer for $answer {
                fn pending(hub: &Hub) -> &PendingRequests<Self> {
                    &hub.$field
                }
            }
        )*
    };
}

node_answers! {
    Vec<String> => file_requests,
    GitAnswer => git_requests,
    SteerAnswer => steer_requests,
    ProcessAnswer => stop_requests,
    FolderAnswer => folder_requests,
    ResetAnswer => reset_requests,
}

#[derive(Default)]
pub struct Hub {
    nodes: Mutex<HashMap<Uuid, NodeConnection>>,
    logins: Mutex<HashMap<Uuid, LoginOwner>>,
    terminals: Mutex<HashMap<Uuid, LoginOwner>>,
    send_now: Mutex<HashMap<Uuid, Uuid>>,
    stale_secrets: Mutex<HashSet<Uuid>>,
    unacknowledged: Mutex<Vec<UnacknowledgedPrompt>>,
    file_requests: PendingRequests<Vec<String>>,
    git_requests: PendingRequests<GitAnswer>,
    steer_requests: PendingRequests<SteerAnswer>,
    stop_requests: PendingRequests<ProcessAnswer>,
    folder_requests: PendingRequests<FolderAnswer>,
    workspace_asks: Mutex<std::collections::HashSet<Uuid>>,
    backups: Mutex<std::collections::HashSet<Uuid>>,
    oauth_sign_ins: Mutex<HashMap<String, PendingOAuth>>,
    lastfm_sign_ins: Mutex<HashMap<String, PendingLastFm>>,
    freeagent_sign_ins: Mutex<HashMap<String, PendingFreeAgent>>,
    reset_requests: PendingRequests<ResetAnswer>,
    resources: Mutex<HashMap<Uuid, WatchedResources>>,
    moves: Mutex<HashMap<Uuid, MoveProgress>>,
    chat_turns: Mutex<HashMap<Uuid, tokio::task::AbortHandle>>,
    chat_approvals: Mutex<HashMap<(Uuid, String), oneshot::Sender<String>>>,
    responses: Mutex<HashMap<(Uuid, String), PendingResponse>>,
    tool_streams: Mutex<HashMap<Uuid, broadcast::Sender<()>>>,
}

impl Hub {
    pub fn subscribe_tools(&self, user: Uuid) -> broadcast::Receiver<()> {
        let mut streams = self.tool_streams.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

        streams.entry(user).or_insert_with(|| broadcast::channel(16).0).subscribe()
    }

    pub fn tools_changed(&self, user: Uuid) {
        if let Ok(streams) = self.tool_streams.lock()
            && let Some(sender) = streams.get(&user)
        {
            let _ = sender.send(());
        }
    }

    pub fn track_chat_turn(&self, conversation_id: Uuid, turn: tokio::task::AbortHandle) {
        if let Ok(mut turns) = self.chat_turns.lock() {
            turns.insert(conversation_id, turn);
        }
    }

    pub fn forget_chat_turn(&self, conversation_id: Uuid) {
        if let Ok(mut turns) = self.chat_turns.lock() {
            turns.remove(&conversation_id);
        }

        self.drop_chat_approvals(conversation_id);
    }

    pub fn abort_chat_turn(&self, conversation_id: Uuid) -> bool {
        let turn = self.chat_turns.lock().ok().and_then(|mut turns| turns.remove(&conversation_id));
        self.drop_chat_approvals(conversation_id);

        turn.is_some_and(|turn| {
            turn.abort();

            true
        })
    }

    pub fn await_chat_approval(&self, conversation_id: Uuid, request_id: &str) -> oneshot::Receiver<String> {
        let (sender, receiver) = oneshot::channel();

        if let Ok(mut approvals) = self.chat_approvals.lock() {
            approvals.insert((conversation_id, request_id.to_string()), sender);
        }

        receiver
    }

    pub fn answer_chat_approval(&self, conversation_id: Uuid, request_id: &str, option_id: String) -> bool {
        let sender = self.chat_approvals.lock().ok().and_then(|mut approvals| approvals.remove(&(conversation_id, request_id.to_string())));

        sender.is_some_and(|sender| sender.send(option_id).is_ok())
    }

    fn drop_chat_approvals(&self, conversation_id: Uuid) {
        if let Ok(mut approvals) = self.chat_approvals.lock() {
            approvals.retain(|(conversation, _), _| *conversation != conversation_id);
        }

        if let Ok(mut responses) = self.responses.lock() {
            responses.retain(|(conversation, _), _| *conversation != conversation_id);
        }
    }

    pub fn await_response(&self, conversation_id: Uuid, request_id: &str, held: crate::response_editor::Held) -> oneshot::Receiver<Option<serde_json::Value>> {
        let (answer, receiver) = oneshot::channel();

        if let Ok(mut responses) = self.responses.lock() {
            responses.insert((conversation_id, request_id.to_string()), PendingResponse { held, answer });
        }

        receiver
    }

    pub fn held_response(&self, conversation_id: Uuid, request_id: &str) -> Option<crate::response_editor::Held> {
        self.responses.lock().ok().and_then(|responses| responses.get(&(conversation_id, request_id.to_string())).map(|pending| pending.held.clone()))
    }

    pub fn answer_response(&self, conversation_id: Uuid, request_id: &str, send: Option<serde_json::Value>) -> bool {
        let pending = self.responses.lock().ok().and_then(|mut responses| responses.remove(&(conversation_id, request_id.to_string())));

        pending.is_some_and(|pending| pending.answer.send(send).is_ok())
    }

    pub fn register(&self, node_id: Uuid, sender: mpsc::UnboundedSender<ControlToNode>) -> Uuid {
        let connection_id = Uuid::new_v4();
        self.forget_send_now_on(node_id);

        let replaced = self.nodes.lock().ok().and_then(|mut nodes| nodes.insert(node_id, NodeConnection { connection_id, sender, sessions: HashSet::new(), greeted: false }));

        if replaced.is_some() {
            tracing::warn!("node {node_id} connected again, so its earlier connection is closed. If this repeats, two processes are running with the same node credential.");
        }

        connection_id
    }

    pub fn unregister(&self, node_id: Uuid, connection_id: Uuid) -> bool {
        let Ok(mut nodes) = self.nodes.lock() else {
            return false;
        };

        if nodes.get(&node_id).is_some_and(|connection| connection.connection_id == connection_id) {
            nodes.remove(&node_id);
            drop(nodes);
            self.forget_send_now_on(node_id);
            self.forget_resources_of(node_id);

            return true;
        }

        false
    }

    pub fn disconnect(&self, node_id: Uuid) {
        if let Ok(mut nodes) = self.nodes.lock() {
            nodes.remove(&node_id);
        }

        self.forget_send_now_on(node_id);
        self.forget_resources_of(node_id);
    }

    pub fn begin_request<A: NodeAnswer>(&self, node_id: Uuid, message: impl FnOnce(Uuid) -> ControlToNode) -> Option<(Uuid, oneshot::Receiver<A>)> {
        let pending = A::pending(self);
        let (request_id, answered) = pending.register(node_id);

        if self.send(node_id, message(request_id)) {
            Some((request_id, answered))
        } else {
            pending.forget(request_id);
            None
        }
    }

    pub fn forget_request<A: NodeAnswer>(&self, request_id: Uuid) {
        A::pending(self).forget(request_id);
    }

    pub fn answer<A: NodeAnswer>(&self, node_id: Uuid, request_id: Uuid, answer: A) {
        A::pending(self).answer(node_id, request_id, answer);
    }

    pub fn begin_backup(&self, assistant_id: Uuid) -> bool {
        self.backups.lock().is_ok_and(|mut running| running.insert(assistant_id))
    }

    pub fn end_backup(&self, assistant_id: Uuid) {
        if let Ok(mut running) = self.backups.lock() {
            running.remove(&assistant_id);
        }
    }

    pub fn ask_workspace(&self, conversation_id: Uuid) {
        if let Ok(mut asks) = self.workspace_asks.lock() {
            asks.insert(conversation_id);
        }
    }

    pub fn take_workspace_ask(&self, conversation_id: Uuid) -> bool {
        self.workspace_asks.lock().is_ok_and(|mut asks| asks.remove(&conversation_id))
    }

    pub fn record_resources(&self, node_id: Uuid, sample: ResourceSample) {
        let Ok(mut resources) = self.resources.lock() else {
            return;
        };
        let watched = resources.entry(node_id).or_default();

        watched.history.push_back(CpuPoint { at_ms: sample.at_ms, cpu_percent: sample.cpu_percent });

        while watched.history.len() > RESOURCE_HISTORY {
            watched.history.pop_front();
        }

        watched.latest = Some(sample);
    }

    pub fn resources_of(&self, node_id: Uuid) -> NodeResources {
        let Ok(resources) = self.resources.lock() else {
            return NodeResources { latest: None, history: Vec::new() };
        };

        match resources.get(&node_id) {
            Some(watched) => NodeResources { latest: watched.latest.clone(), history: watched.history.iter().cloned().collect() },
            None => NodeResources { latest: None, history: Vec::new() },
        }
    }

    fn forget_resources_of(&self, node_id: Uuid) {
        if let Ok(mut resources) = self.resources.lock() {
            resources.remove(&node_id);
        }
    }

    pub fn begin_move(&self, progress: MoveProgress) -> bool {
        let Ok(mut moves) = self.moves.lock() else {
            return false;
        };

        if moves.get(&progress.conversation_id).is_some_and(|current| matches!(current.state, MoveState::Waiting | MoveState::Moving)) {
            return false;
        }

        moves.insert(progress.conversation_id, progress);

        true
    }

    pub fn set_move(&self, progress: MoveProgress) {
        if let Ok(mut moves) = self.moves.lock() {
            moves.insert(progress.conversation_id, progress);
        }
    }

    pub fn move_of(&self, conversation_id: Uuid) -> Option<MoveProgress> {
        self.moves.lock().ok().and_then(|moves| moves.get(&conversation_id).cloned())
    }

    pub fn is_moving(&self, conversation_id: Uuid) -> bool {
        self.move_of(conversation_id).is_some_and(|progress| progress.state == MoveState::Moving)
    }

    pub fn is_replaced(&self, node_id: Uuid, connection_id: Uuid) -> bool {
        self.nodes.lock().ok().and_then(|nodes| nodes.get(&node_id).map(|connection| connection.connection_id != connection_id)).unwrap_or(false)
    }

    pub fn is_online(&self, node_id: Uuid) -> bool {
        self.nodes.lock().is_ok_and(|nodes| nodes.get(&node_id).is_some_and(|connection| connection.greeted))
    }

    pub fn send(&self, node_id: Uuid, message: ControlToNode) -> bool {
        let Ok(nodes) = self.nodes.lock() else {
            return false;
        };

        nodes.get(&node_id).is_some_and(|connection| connection.sender.send(message).is_ok())
    }

    pub fn set_sessions(&self, node_id: Uuid, sessions: HashSet<Uuid>) {
        if let Ok(mut nodes) = self.nodes.lock()
            && let Some(connection) = nodes.get_mut(&node_id)
        {
            connection.sessions = sessions;
            connection.greeted = true;
        }
    }

    pub fn add_session(&self, node_id: Uuid, conversation_id: Uuid) -> bool {
        let Ok(mut nodes) = self.nodes.lock() else {
            return false;
        };

        nodes.get_mut(&node_id).is_some_and(|connection| connection.sessions.insert(conversation_id))
    }

    pub fn sessions_of(&self, node_id: Uuid) -> Vec<Uuid> {
        let mut sessions: Vec<Uuid> = self.nodes.lock().ok().and_then(|nodes| nodes.get(&node_id).map(|connection| connection.sessions.iter().copied().collect())).unwrap_or_default();

        sessions.sort();

        sessions
    }

    pub fn has_session(&self, node_id: Uuid, conversation_id: Uuid) -> bool {
        self.nodes.lock().is_ok_and(|nodes| nodes.get(&node_id).is_some_and(|connection| connection.sessions.contains(&conversation_id)))
    }

    pub fn remove_session(&self, node_id: Uuid, conversation_id: Uuid) -> bool {
        let Ok(mut nodes) = self.nodes.lock() else {
            return false;
        };

        nodes.get_mut(&node_id).is_some_and(|connection| connection.sessions.remove(&conversation_id))
    }

    pub fn track_oauth(&self, sign_in: String, pending: PendingOAuth) {
        if let Ok(mut sign_ins) = self.oauth_sign_ins.lock() {
            sign_ins.retain(|_, held| held.created.elapsed() < OAUTH_SIGN_IN_LIFETIME);
            sign_ins.insert(sign_in, pending);
        }
    }

    pub fn take_oauth(&self, sign_in: &str) -> Option<PendingOAuth> {
        let pending = self.oauth_sign_ins.lock().ok()?.remove(sign_in)?;

        (pending.created.elapsed() < OAUTH_SIGN_IN_LIFETIME).then_some(pending)
    }

    pub fn track_lastfm(&self, sign_in: String, pending: PendingLastFm) {
        if let Ok(mut sign_ins) = self.lastfm_sign_ins.lock() {
            sign_ins.retain(|_, held| held.created.elapsed() < OAUTH_SIGN_IN_LIFETIME);
            sign_ins.insert(sign_in, pending);
        }
    }

    pub fn take_lastfm(&self, sign_in: &str) -> Option<PendingLastFm> {
        let pending = self.lastfm_sign_ins.lock().ok()?.remove(sign_in)?;

        (pending.created.elapsed() < OAUTH_SIGN_IN_LIFETIME).then_some(pending)
    }

    pub fn track_freeagent(&self, sign_in: String, pending: PendingFreeAgent) {
        if let Ok(mut sign_ins) = self.freeagent_sign_ins.lock() {
            sign_ins.retain(|_, held| held.created.elapsed() < OAUTH_SIGN_IN_LIFETIME);
            sign_ins.insert(sign_in, pending);
        }
    }

    pub fn take_freeagent(&self, sign_in: &str) -> Option<PendingFreeAgent> {
        let pending = self.freeagent_sign_ins.lock().ok()?.remove(sign_in)?;

        (pending.created.elapsed() < OAUTH_SIGN_IN_LIFETIME).then_some(pending)
    }

    pub fn track_login(&self, login_id: Uuid, node_id: Uuid, user_id: Uuid) {
        if let Ok(mut logins) = self.logins.lock() {
            logins.insert(login_id, LoginOwner { node_id, user_id });
        }
    }

    pub fn login_owner(&self, login_id: Uuid) -> Option<LoginOwner> {
        self.logins.lock().ok().and_then(|logins| logins.get(&login_id).copied())
    }

    pub fn finish_login(&self, login_id: Uuid) {
        if let Ok(mut logins) = self.logins.lock() {
            logins.remove(&login_id);
        }
    }

    pub fn finish_logins_of(&self, node_id: Uuid) -> Vec<(Uuid, LoginOwner)> {
        let Ok(mut logins) = self.logins.lock() else {
            return Vec::new();
        };
        let finished: Vec<(Uuid, LoginOwner)> = logins.iter().filter(|(_, owner)| owner.node_id == node_id).map(|(login_id, owner)| (*login_id, *owner)).collect();

        logins.retain(|_, owner| owner.node_id != node_id);

        finished
    }

    pub fn track_terminal(&self, terminal_id: Uuid, node_id: Uuid, user_id: Uuid) {
        if let Ok(mut terminals) = self.terminals.lock() {
            terminals.insert(terminal_id, LoginOwner { node_id, user_id });
        }
    }

    pub fn terminal_owner(&self, terminal_id: Uuid) -> Option<LoginOwner> {
        self.terminals.lock().ok().and_then(|terminals| terminals.get(&terminal_id).copied())
    }

    pub fn finish_terminal(&self, terminal_id: Uuid) {
        if let Ok(mut terminals) = self.terminals.lock() {
            terminals.remove(&terminal_id);
        }
    }

    pub fn finish_terminals_of(&self, node_id: Uuid) -> Vec<(Uuid, LoginOwner)> {
        let Ok(mut terminals) = self.terminals.lock() else {
            return Vec::new();
        };
        let finished: Vec<(Uuid, LoginOwner)> = terminals.iter().filter(|(_, owner)| owner.node_id == node_id).map(|(terminal_id, owner)| (*terminal_id, *owner)).collect();

        terminals.retain(|_, owner| owner.node_id != node_id);

        finished
    }

    pub fn mark_secrets_stale(&self, conversation_id: Uuid) {
        if let Ok(mut stale) = self.stale_secrets.lock() {
            stale.insert(conversation_id);
        }
    }

    pub fn secrets_stale(&self, conversation_id: Uuid) -> bool {
        self.stale_secrets.lock().is_ok_and(|stale| stale.contains(&conversation_id))
    }

    pub fn secrets_fresh(&self, conversation_id: Uuid) {
        if let Ok(mut stale) = self.stale_secrets.lock() {
            stale.remove(&conversation_id);
        }
    }

    pub fn expect_send_now(&self, conversation_id: Uuid, node_id: Uuid) {
        if let Ok(mut send_now) = self.send_now.lock() {
            send_now.insert(conversation_id, node_id);
        }
    }

    pub fn take_send_now(&self, conversation_id: Uuid) -> bool {
        self.send_now.lock().is_ok_and(|mut send_now| send_now.remove(&conversation_id).is_some())
    }

    fn forget_send_now_on(&self, node_id: Uuid) {
        if let Ok(mut send_now) = self.send_now.lock() {
            send_now.retain(|_, node| *node != node_id);
        }
    }

    pub fn await_prompt_acknowledgement(&self, prompt: UnacknowledgedPrompt) {
        if let Ok(mut unacknowledged) = self.unacknowledged.lock() {
            unacknowledged.push(prompt);
        }
    }

    pub fn acknowledge_prompt(&self, node_id: Uuid, conversation_id: Uuid, prompt_id: Uuid) {
        if let Ok(mut unacknowledged) = self.unacknowledged.lock() {
            unacknowledged.retain(|prompt| !(prompt.node_id == node_id && prompt.conversation_id == conversation_id && prompt.prompt_id == prompt_id));
        }
    }

    pub fn forget_prompts_of(&self, conversation_id: Uuid) {
        if let Ok(mut unacknowledged) = self.unacknowledged.lock() {
            unacknowledged.retain(|prompt| prompt.conversation_id != conversation_id);
        }
    }

    pub fn unacknowledged_prompts_on(&self, node_id: Uuid, live: &HashSet<Uuid>) -> Vec<UnacknowledgedPrompt> {
        let Ok(mut unacknowledged) = self.unacknowledged.lock() else {
            return Vec::new();
        };

        unacknowledged.retain(|prompt| prompt.node_id != node_id || live.contains(&prompt.conversation_id));

        unacknowledged.iter().filter(|prompt| prompt.node_id == node_id).cloned().collect()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    pub hub: Arc<Hub>,
    pub stream: broadcast::Sender<Streamed>,
    pub rate_limiter: Arc<RateLimiter>,
    pub latest: Arc<LatestVersions>,
    pub readiness: Arc<Mutex<Readiness>>,
    pub http: reqwest::Client,
    pub forge_cache: Arc<ForgeCache>,
}

impl AppState {
    pub async fn ask_node<A: NodeAnswer>(&self, node_id: Uuid, timeout: Duration, message: impl FnOnce(Uuid) -> ControlToNode) -> Option<A> {
        let (request_id, answered) = self.hub.begin_request::<A>(node_id, message)?;
        let answer = tokio::time::timeout(timeout, answered).await.ok().and_then(Result::ok);
        self.hub.forget_request::<A>(request_id);

        answer
    }

    pub fn publish(&self, owner: Option<Uuid>, message: StreamMessage) {
        let _ = self.stream.send(Streamed { owner, message });
    }

    pub async fn add_session(&self, node_id: Uuid, conversation_id: Uuid) -> ApiResult<()> {
        if self.hub.add_session(node_id, conversation_id) {
            publish_node(self, node_id).await?;
        }

        Ok(())
    }

    pub async fn remove_session(&self, node_id: Uuid, conversation_id: Uuid) -> ApiResult<()> {
        if self.hub.remove_session(node_id, conversation_id) {
            publish_node(self, node_id).await?;
        }

        Ok(())
    }

    pub fn finish_logins_of_node(&self, node_id: Uuid) {
        for (login_id, owner) in self.hub.finish_logins_of(node_id) {
            self.publish(Some(owner.user_id), StreamMessage::LoginFinished { login_id, ok: false });
        }

        for (terminal_id, owner) in self.hub.finish_terminals_of(node_id) {
            self.publish(Some(owner.user_id), StreamMessage::TerminalExited { terminal_id });
        }
    }
}

pub fn new_token() -> String {
    use base64::Engine;

    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_from_another_node_is_ignored() {
        let pending = PendingRequests::<String>::default();
        let asked = Uuid::new_v4();
        let (request_id, mut answered) = pending.register(asked);

        pending.answer(Uuid::new_v4(), request_id, "wrong".into());

        assert!(answered.try_recv().is_err());

        pending.answer(asked, request_id, "right".into());

        assert_eq!(answered.try_recv().ok().as_deref(), Some("right"));
    }

    #[test]
    fn a_forgotten_request_is_dropped() {
        let pending = PendingRequests::<String>::default();
        let asked = Uuid::new_v4();
        let (request_id, mut answered) = pending.register(asked);

        pending.forget(request_id);
        pending.answer(asked, request_id, "late".into());

        assert!(matches!(answered.try_recv(), Err(oneshot::error::TryRecvError::Closed)));
    }
}
