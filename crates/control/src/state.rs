use crate::config::Config;
use mycelium_proto::link::ControlToNode;
use mycelium_proto::stream::StreamMessage;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, mpsc};
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
    Failed(String),
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

#[derive(Default)]
pub struct Hub {
    nodes: Mutex<HashMap<Uuid, NodeConnection>>,
    logins: Mutex<HashMap<Uuid, LoginOwner>>,
    send_now: Mutex<HashSet<Uuid>>,
}

impl Hub {
    pub fn register(&self, node_id: Uuid, sender: mpsc::UnboundedSender<ControlToNode>) -> Uuid {
        let connection_id = Uuid::new_v4();
        if let Ok(mut nodes) = self.nodes.lock() {
            nodes.insert(node_id, NodeConnection { connection_id, sender, sessions: HashSet::new(), greeted: false });
        }

        connection_id
    }

    pub fn unregister(&self, node_id: Uuid, connection_id: Uuid) -> bool {
        let Ok(mut nodes) = self.nodes.lock() else {
            return false;
        };

        if nodes.get(&node_id).is_some_and(|connection| connection.connection_id == connection_id) {
            nodes.remove(&node_id);

            return true;
        }

        false
    }

    pub fn disconnect(&self, node_id: Uuid) {
        if let Ok(mut nodes) = self.nodes.lock() {
            nodes.remove(&node_id);
        }
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

    pub fn add_session(&self, node_id: Uuid, conversation_id: Uuid) {
        if let Ok(mut nodes) = self.nodes.lock()
            && let Some(connection) = nodes.get_mut(&node_id)
        {
            connection.sessions.insert(conversation_id);
        }
    }

    pub fn has_session(&self, node_id: Uuid, conversation_id: Uuid) -> bool {
        self.nodes.lock().is_ok_and(|nodes| nodes.get(&node_id).is_some_and(|connection| connection.sessions.contains(&conversation_id)))
    }

    pub fn remove_session(&self, node_id: Uuid, conversation_id: Uuid) {
        if let Ok(mut nodes) = self.nodes.lock()
            && let Some(connection) = nodes.get_mut(&node_id)
        {
            connection.sessions.remove(&conversation_id);
        }
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

    pub fn expect_send_now(&self, conversation_id: Uuid) {
        if let Ok(mut send_now) = self.send_now.lock() {
            send_now.insert(conversation_id);
        }
    }

    pub fn take_send_now(&self, conversation_id: Uuid) -> bool {
        self.send_now.lock().is_ok_and(|mut send_now| send_now.remove(&conversation_id))
    }
}

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    pub hub: Arc<Hub>,
    pub stream: broadcast::Sender<Streamed>,
    pub readiness: Arc<Mutex<Readiness>>,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn publish(&self, owner: Option<Uuid>, message: StreamMessage) {
        let _ = self.stream.send(Streamed { owner, message });
    }
}

pub fn new_token() -> String {
    use base64::Engine;

    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}
