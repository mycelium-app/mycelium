use crate::config::Config;
use crate::error::ApiResult;
use crate::forge_cache::ForgeCache;
use crate::rate_limit::RateLimiter;
use crate::store::publish_node;
use crate::versions::LatestVersions;
use mycelium_proto::api::{CpuPoint, MoveProgress, MoveState, NodeResources};
use mycelium_proto::link::{ControlToNode, GitCommand, ResourceSample};
use serde_json::Value;
use mycelium_proto::stream::StreamMessage;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
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

pub struct ProcessAnswer {
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Hub {
    nodes: Mutex<HashMap<Uuid, NodeConnection>>,
    logins: Mutex<HashMap<Uuid, LoginOwner>>,
    terminals: Mutex<HashMap<Uuid, LoginOwner>>,
    send_now: Mutex<HashMap<Uuid, Uuid>>,
    unacknowledged: Mutex<Vec<UnacknowledgedPrompt>>,
    file_requests: Mutex<HashMap<Uuid, (Uuid, oneshot::Sender<Vec<String>>)>>,
    git_requests: Mutex<HashMap<Uuid, (Uuid, oneshot::Sender<GitAnswer>)>>,
    stop_requests: Mutex<HashMap<Uuid, (Uuid, oneshot::Sender<ProcessAnswer>)>>,
    resources: Mutex<HashMap<Uuid, WatchedResources>>,
    moves: Mutex<HashMap<Uuid, MoveProgress>>,
    chat_turns: Mutex<HashMap<Uuid, tokio::task::AbortHandle>>,
    chat_approvals: Mutex<HashMap<(Uuid, String), oneshot::Sender<String>>>,
}

impl Hub {
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

    pub fn request_files(&self, node_id: Uuid, cwd: String, query: String) -> Option<(Uuid, oneshot::Receiver<Vec<String>>)> {
        let request_id = Uuid::new_v4();
        let (answer, answered) = oneshot::channel();

        if let Ok(mut requests) = self.file_requests.lock() {
            requests.insert(request_id, (node_id, answer));
        }

        if self.send(node_id, ControlToNode::ListFiles { request_id, cwd, query }) {
            Some((request_id, answered))
        } else {
            self.forget_file_request(request_id);
            None
        }
    }

    pub fn answer_files(&self, node_id: Uuid, request_id: Uuid, paths: Vec<String>) {
        let waiting = self.file_requests.lock().ok().and_then(|mut requests| match requests.get(&request_id) {
            Some((owner, _)) if *owner == node_id => requests.remove(&request_id),
            _ => None,
        });

        if let Some((_, answer)) = waiting {
            let _ = answer.send(paths);
        }
    }

    pub fn forget_file_request(&self, request_id: Uuid) {
        if let Ok(mut requests) = self.file_requests.lock() {
            requests.remove(&request_id);
        }
    }

    pub fn request_git(&self, node_id: Uuid, cwd: String, command: GitCommand) -> Option<(Uuid, oneshot::Receiver<GitAnswer>)> {
        let request_id = Uuid::new_v4();
        let (answer, answered) = oneshot::channel();

        if let Ok(mut requests) = self.git_requests.lock() {
            requests.insert(request_id, (node_id, answer));
        }

        if self.send(node_id, ControlToNode::Git { request_id, cwd, command }) {
            Some((request_id, answered))
        } else {
            self.forget_git_request(request_id);
            None
        }
    }

    pub fn answer_git(&self, node_id: Uuid, request_id: Uuid, answer: GitAnswer) {
        let waiting = self.git_requests.lock().ok().and_then(|mut requests| match requests.get(&request_id) {
            Some((owner, _)) if *owner == node_id => requests.remove(&request_id),
            _ => None,
        });

        if let Some((_, waiter)) = waiting {
            let _ = waiter.send(answer);
        }
    }

    pub fn forget_git_request(&self, request_id: Uuid) {
        if let Ok(mut requests) = self.git_requests.lock() {
            requests.remove(&request_id);
        }
    }

    pub fn request_stop(&self, node_id: Uuid, pid: u32, name: String) -> Option<(Uuid, oneshot::Receiver<ProcessAnswer>)> {
        let request_id = Uuid::new_v4();
        let (answer, answered) = oneshot::channel();

        if let Ok(mut requests) = self.stop_requests.lock() {
            requests.insert(request_id, (node_id, answer));
        }

        if self.send(node_id, ControlToNode::StopProcess { request_id, pid, name }) {
            Some((request_id, answered))
        } else {
            self.forget_stop_request(request_id);
            None
        }
    }

    pub fn answer_stop(&self, node_id: Uuid, request_id: Uuid, answer: ProcessAnswer) {
        let waiting = self.stop_requests.lock().ok().and_then(|mut requests| match requests.get(&request_id) {
            Some((owner, _)) if *owner == node_id => requests.remove(&request_id),
            _ => None,
        });

        if let Some((_, waiter)) = waiting {
            let _ = waiter.send(answer);
        }
    }

    pub fn forget_stop_request(&self, request_id: Uuid) {
        if let Ok(mut requests) = self.stop_requests.lock() {
            requests.remove(&request_id);
        }
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
