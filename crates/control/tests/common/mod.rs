#![allow(dead_code)]

use futures_util::{SinkExt, StreamExt};
use mycelium_control::Config;
use mycelium_proto::conversation::{AgentId, AgentInfo, Conversation};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection};
use std::future::Future;
use std::net::SocketAddr;
use std::time::Duration;
use tempfile::TempDir;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

pub const WAIT: Duration = Duration::from_secs(15);

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub async fn eventually<T, F, Fut>(what: &str, mut attempt: F) -> T
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<T>>,
{
    let deadline = tokio::time::Instant::now() + WAIT;

    loop {
        if let Some(found) = attempt().await {
            return found;
        }

        assert!(tokio::time::Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub async fn create_database() -> String {
    let admin = std::env::var("MYCELIUM_TEST_DATABASE_URL").expect("Set MYCELIUM_TEST_DATABASE_URL to a Postgres admin URL, for example postgres://postgres:pw@127.0.0.1:5432/postgres");
    let name = format!("test_{}", Uuid::new_v4().simple());
    let mut connection = PgConnection::connect(&admin).await.unwrap();

    sqlx::query(sqlx::AssertSqlSafe(format!("create database {name}"))).execute(&mut connection).await.unwrap();

    let (base, _) = admin.rsplit_once('/').unwrap();

    format!("{base}/{name}")
}

pub fn hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

#[derive(Clone)]
pub struct Client {
    pub base: String,
    pub cookie: String,
    pub http: reqwest::Client,
}

impl Client {
    fn authorised(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.cookie.is_empty() { request } else { request.header("cookie", &self.cookie) }
    }

    pub fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.authorised(self.http.get(format!("{}{path}", self.base)))
    }

    pub fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.authorised(self.http.post(format!("{}{path}", self.base)))
    }

    pub fn patch(&self, path: &str) -> reqwest::RequestBuilder {
        self.authorised(self.http.patch(format!("{}{path}", self.base)))
    }

    pub fn delete(&self, path: &str) -> reqwest::RequestBuilder {
        self.authorised(self.http.delete(format!("{}{path}", self.base)))
    }

    pub async fn json(&self, response: reqwest::RequestBuilder) -> (u16, Value) {
        let response = response.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();

        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    pub async fn stream(&self) -> StreamClient {
        let url = format!("{}/api/v1/stream", self.base.replacen("http", "ws", 1));
        let mut request = url.into_client_request().unwrap();

        request.headers_mut().insert("cookie", self.cookie.parse().unwrap());

        let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();

        StreamClient { socket }
    }
}

pub struct StreamClient {
    socket: Socket,
}

impl StreamClient {
    pub async fn next(&mut self) -> StreamMessage {
        loop {
            let frame = tokio::time::timeout(WAIT, self.socket.next()).await.expect("timed out waiting for a stream message").unwrap().unwrap();

            if let Message::Text(text) = frame {
                return serde_json::from_str(&text).unwrap();
            }
        }
    }

    pub async fn until<T>(&mut self, mut pick: impl FnMut(StreamMessage) -> Option<T>) -> T {
        loop {
            let message = self.next().await;

            if let Some(found) = pick(message) {
                return found;
            }
        }
    }

    pub async fn conversation_with_status(&mut self, status: &str) -> Conversation {
        let status = status.to_string();

        self.until(|message| match message {
            StreamMessage::Conversation(conversation) if serde_json::to_value(conversation.status).unwrap() == json!(status) => Some(conversation),
            _ => None,
        })
        .await
    }
}

pub struct NodeLink {
    pub id: Uuid,
    pub token: String,
    socket: Socket,
}

impl NodeLink {
    pub async fn send(&mut self, message: NodeToControl) {
        self.socket.send(Message::Text(serde_json::to_string(&message).unwrap().into())).await.unwrap();
    }

    pub async fn next(&mut self) -> ControlToNode {
        loop {
            let message = self.raw().await;

            if !matches!(message, ControlToNode::Ack { .. }) {
                return message;
            }
        }
    }

    pub async fn raw(&mut self) -> ControlToNode {
        loop {
            let frame = tokio::time::timeout(WAIT, self.socket.next()).await.expect("timed out waiting for a node message").expect("link closed").expect("link error");

            if let Message::Text(text) = frame {
                let message: ControlToNode = serde_json::from_str(&text).unwrap();

                if message != ControlToNode::Ping {
                    return message;
                }
            }
        }
    }

    pub async fn closed(&mut self) -> bool {
        loop {
            match tokio::time::timeout(WAIT, self.socket.next()).await {
                Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Ok(Some(Err(_))) => return true,
                Ok(Some(Ok(_))) => {}
                Err(_) => return false,
            }
        }
    }

    pub async fn hello(&mut self, sessions: Vec<(Uuid, i64)>) {
        self.send(NodeToControl::Hello {
            version: "0.1.0".into(),
            hostname: "laptop".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            agents: vec![AgentInfo { agent: AgentId::Claude, version: Some("2.1.308".into()), available: true }],
            sessions: sessions.into_iter().map(|(conversation_id, last_seq)| mycelium_proto::conversation::SessionPosition { conversation_id, last_seq, live: true }).collect(),
        })
        .await;
    }

    pub async fn expect_start_session(&mut self) -> Uuid {
        match self.next().await {
            ControlToNode::StartSession { conversation_id, .. } => conversation_id,
            other => panic!("expected start_session, got {other:?}"),
        }
    }

    pub async fn start_session(&mut self, conversation_id: Uuid, acp_session_id: &str) {
        self.send(NodeToControl::SessionStarted { conversation_id, acp_session_id: acp_session_id.into(), modes: None, models: None, config_options: None }).await;
    }

    pub async fn expect_prompt(&mut self) -> (Uuid, String) {
        match self.next().await {
            ControlToNode::Prompt { prompt_id, text, .. } => (prompt_id, text),
            other => panic!("expected prompt, got {other:?}"),
        }
    }

    pub async fn expect_ack(&mut self) -> (Uuid, i64) {
        match self.raw().await {
            ControlToNode::Ack { conversation_id, seq } => (conversation_id, seq),
            other => panic!("expected ack, got {other:?}"),
        }
    }

    pub async fn barrier(&mut self, conversation_id: Uuid, seq: i64) {
        self.send(NodeToControl::AcpUpdate { conversation_id, seq, update: json!({"sessionUpdate": "barrier"}) }).await;

        loop {
            if let ControlToNode::Ack { seq: acknowledged, .. } = self.raw().await
                && acknowledged >= seq
            {
                return;
            }
        }
    }
}

pub struct Server {
    pub user: Client,
    pub base: String,
    pub address: SocketAddr,
    pub database_url: String,
    pub data_dir: std::path::PathBuf,
    pub web_dir: std::path::PathBuf,
    pub pool: sqlx::PgPool,
    _directory: TempDir,
}

impl Server {
    pub async fn start() -> Server {
        Server::start_with(|_| {}).await
    }

    pub async fn start_with(configure: impl FnOnce(&mut Config)) -> Server {
        let database_url = create_database().await;

        Server::start_on(database_url, configure).await
    }

    pub async fn start_on(database_url: String, configure: impl FnOnce(&mut Config)) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let directory = TempDir::new().unwrap();
        let web_dir = directory.path().join("web");
        let data_dir = directory.path().join("data");
        std::fs::create_dir_all(&web_dir).unwrap();

        let base = format!("http://127.0.0.1:{}", address.port());
        let mut config = Config {
            database_url: database_url.clone(),
            listen: address,
            public_url: base.clone(),
            data_dir: data_dir.clone(),
            oidc: None,
            dev_login: Some("dev@example.com".into()),
            web_dir: web_dir.clone(),
            skip_migrations: false,
        };
        configure(&mut config);
        let has_dev_login = config.dev_login.is_some();

        tokio::spawn(mycelium_control::run(config, listener));

        let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
        let anonymous = Client { base: base.clone(), cookie: String::new(), http: http.clone() };

        let probe = anonymous.clone();
        eventually("readyz", || async { (probe.get("/readyz").send().await.unwrap().status() == 200).then_some(()) }).await;

        let cookie = if has_dev_login { session_cookie(&anonymous.get("/auth/login").send().await.unwrap()) } else { String::new() };
        let pool = sqlx::PgPool::connect(&database_url).await.unwrap();

        Server { user: Client { base: base.clone(), cookie, http }, base, address, database_url, data_dir, web_dir, pool, _directory: directory }
    }

    pub fn anonymous(&self) -> Client {
        Client { base: self.base.clone(), cookie: String::new(), http: self.user.http.clone() }
    }

    pub async fn second_user(&self, email: &str) -> Client {
        let user_id = sqlx::query_scalar::<_, Uuid>("insert into users (subject, email, name) values ($1, $2, $2) returning id").bind(format!("test:{email}")).bind(email).fetch_one(&self.pool).await.unwrap();
        let token = Uuid::new_v4().to_string();

        sqlx::query("insert into web_sessions (user_id, token_hash, expires_at) values ($1, $2, now() + interval '1 day')").bind(user_id).bind(hash(&token)).execute(&self.pool).await.unwrap();

        Client { base: self.base.clone(), cookie: format!("myc_session={token}"), http: self.user.http.clone() }
    }

    pub async fn join(&self, kind: &str, label: &str) -> (Uuid, String) {
        let anonymous = self.anonymous();
        let (_, started) = anonymous.json(anonymous.post("/api/v1/device").json(&json!({"kind": kind, "label": label, "hostname": label, "os": "linux", "arch": "x86_64"}))).await;
        let user_code = started["user_code"].as_str().unwrap().to_string();

        let (status, _) = self.user.json(self.user.post(&format!("/api/v1/device/{user_code}/approve"))).await;
        assert_eq!(status, 204);

        let (status, token) = anonymous.json(anonymous.post("/api/v1/device/token").json(&json!({"device_code": started["device_code"]}))).await;
        assert_eq!(status, 200, "{token}");

        (token["id"].as_str().unwrap().parse().unwrap(), token["token"].as_str().unwrap().to_string())
    }

    pub async fn connect_node(&self, id: Uuid, token: &str) -> NodeLink {
        let socket = self.try_connect_node(token).await.unwrap();

        NodeLink { id, token: token.to_string(), socket }
    }

    pub async fn try_connect_node(&self, token: &str) -> Result<Socket, tokio_tungstenite::tungstenite::Error> {
        let url = format!("ws://127.0.0.1:{}/api/v1/node/connect", self.address.port());
        let mut request = url.into_client_request().unwrap();

        request.headers_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());

        tokio_tungstenite::connect_async(request).await.map(|(socket, _)| socket)
    }

    pub async fn online_node(&self, name: &str) -> NodeLink {
        let (id, token) = self.join("node", name).await;
        let mut link = self.connect_node(id, &token).await;

        link.hello(vec![]).await;

        let user = self.user.clone();
        eventually("node agents", || async {
            let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

            nodes.as_array().unwrap().iter().any(|node| node["id"] == json!(id) && node["agents"].as_array().is_some_and(|agents| !agents.is_empty())).then_some(())
        })
        .await;

        link
    }

    pub async fn create_conversation(&self, node_id: Uuid, prompt: Option<&str>) -> Conversation {
        let (status, body) = self.user.json(self.user.post("/api/v1/conversations").json(&json!({"node_id": node_id, "agent": "claude", "cwd": "/work/app", "prompt": prompt}))).await;
        assert_eq!(status, 201, "{body}");

        serde_json::from_value(body).unwrap()
    }

    pub async fn events(&self, conversation_id: Uuid) -> Vec<Value> {
        let (_, events) = self.user.json(self.user.get(&format!("/api/v1/conversations/{conversation_id}/events"))).await;

        events.as_array().unwrap().clone()
    }

    pub async fn conversation(&self, conversation_id: Uuid) -> Value {
        let (_, conversation) = self.user.json(self.user.get(&format!("/api/v1/conversations/{conversation_id}"))).await;

        conversation
    }
}

pub fn session_cookie(response: &reqwest::Response) -> String {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|value| value.starts_with("myc_session=") && !value.starts_with("myc_session=;"))
        .map(|value| value.split(';').next().unwrap().to_string())
        .expect("no session cookie was set")
}
