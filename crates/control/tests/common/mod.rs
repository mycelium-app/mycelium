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

pub const WAIT: Duration = Duration::from_secs(60);

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
    let Ok(admin) = std::env::var("MYCELIUM_TEST_DATABASE_URL") else {
        eprintln!("SKIPPED: set MYCELIUM_TEST_DATABASE_URL to a Postgres admin URL, for example postgres://postgres:pw@127.0.0.1:5432/postgres, to run these tests.");
        std::process::exit(0);
    };
    let name = format!("test_{}_{}", std::process::id(), Uuid::new_v4().simple());
    let mut connection = PgConnection::connect(&admin).await.unwrap();
    let existing: Vec<String> = sqlx::query_scalar("select datname from pg_database where datname ~ '^test_[0-9]+_'").fetch_all(&mut connection).await.unwrap();
    let finished = existing.into_iter().filter(|database| {
        let creator = database.split('_').nth(1).unwrap_or_default();

        !std::path::Path::new("/proc").join(creator).exists()
    });

    for stale in finished {
        sqlx::query(sqlx::AssertSqlSafe(format!("drop database if exists {stale} with (force)"))).execute(&mut connection).await.ok();
    }

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

    pub fn put(&self, path: &str) -> reqwest::RequestBuilder {
        self.authorised(self.http.put(format!("{}{path}", self.base)))
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

            match message {
                ControlToNode::Ack { .. } => {}
                ControlToNode::Git { request_id, command: mycelium_proto::link::GitCommand::FindLogo, .. } => {
                    self.send(NodeToControl::GitResult { request_id, ok: true, output: Value::Null, error: None }).await;
                }
                other => return other,
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

    pub async fn close_code(&mut self) -> Option<u16> {
        loop {
            match tokio::time::timeout(WAIT, self.socket.next()).await {
                Ok(Some(Ok(Message::Close(frame)))) => return frame.map(|frame| u16::from(frame.code)),
                Ok(Some(Ok(_))) => {}
                _ => return None,
            }
        }
    }

    pub async fn hello(&mut self, sessions: Vec<(Uuid, i64)>) {
        self.hello_as(env!("CARGO_PKG_VERSION"), sessions).await;
    }

    pub async fn hello_as(&mut self, version: &str, sessions: Vec<(Uuid, i64)>) {
        self.send(NodeToControl::Hello {
            version: version.into(),
            hostname: "laptop".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            agents: vec![AgentInfo { agent: AgentId::Claude, version: Some("2.1.308".into()), available: true, update_available: None }],
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
        let _ = tracing_subscriber::fmt().with_test_writer().with_max_level(tracing::Level::WARN).try_init();
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
            allowed_emails: Vec::new(),
            web_dir: web_dir.clone(),
            editor_dir: directory.path().join("editor"),
            artifact_domain: None,
            artifact_host_token: None,
            browser_url: None,
            skip_migrations: false,
            trusted_proxies: Vec::new(),
            device_code_lifetime: mycelium_control::DEVICE_CODE_LIFETIME,
            release_url: "http://127.0.0.1:1/latest".into(),
            npm_registry_url: "http://127.0.0.1:1".into(),
            releases_url: "http://127.0.0.1:1".into(),
            version_check_interval: mycelium_control::VERSION_CHECK_INTERVAL,
            scheduler_tick: mycelium_control::SCHEDULER_TICK,
            forge_cache_ttl: mycelium_control::FORGE_CACHE_TTL,
            steer_timeout: mycelium_control::STEER_TIMEOUT,
            database_pool_size: 30,
            database_acquire_timeout: std::time::Duration::from_secs(30),
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
        self.online_node_as(name, env!("CARGO_PKG_VERSION")).await
    }

    pub async fn online_node_as(&self, name: &str, version: &str) -> NodeLink {
        self.online_node_with_token(name, version).await.0
    }

    pub async fn online_node_with_token(&self, name: &str, version: &str) -> (NodeLink, String) {
        let (id, token) = self.join("node", name).await;
        let mut link = self.connect_node(id, &token).await;

        link.hello_as(version, vec![]).await;

        let user = self.user.clone();
        eventually("node agents", || async {
            let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

            nodes.as_array().unwrap().iter().any(|node| node["id"] == json!(id) && node["online"] == true && node["agents"].as_array().is_some_and(|agents| !agents.is_empty())).then_some(())
        })
        .await;

        (link, token)
    }

    pub async fn create_conversation(&self, node_id: Uuid, prompt: Option<&str>) -> Conversation {
        let (status, body) = self.user.json(self.user.post("/api/v1/conversations").json(&json!({"node_id": node_id, "agent": "claude", "cwd": "/work/app", "prompt": prompt}))).await;
        assert_eq!(status, 201, "{body}");

        serde_json::from_value(body).unwrap()
    }

    pub async fn node_conversation(&self, token: &str) -> Uuid {
        let node = sqlx::query_scalar::<_, Uuid>("select id from nodes where token_hash = $1").bind(hash(token.strip_prefix("myc_node_").unwrap())).fetch_one(&self.pool).await.unwrap();

        self.create_conversation(node, None).await.id
    }

    pub async fn agent_mcp(&self, token: &str, conversation_id: Uuid, body: Value) -> Value {
        let anonymous = self.anonymous();
        let request = anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).header("x-mycelium-conversation", conversation_id.to_string()).json(&body);
        let (status, body) = anonymous.json(request).await;
        assert_eq!(status, 200, "{body}");

        body
    }

    pub async fn open_request(&self, conversation_id: Uuid) -> Value {
        eventually("a permission request", || async {
            let events = self.events(conversation_id).await;
            let open = events.iter().find(|event| event["kind"] == "permission_request" && !events.iter().any(|other| other["kind"] == "permission_resolved" && other["payload"]["request_id"] == event["payload"]["request_id"]));

            open.map(|event| event["payload"].clone())
        })
        .await
    }

    pub async fn answer(&self, conversation_id: Uuid, request: &Value, option: &str) {
        let (status, body) = self.user.json(self.user.post(&format!("/api/v1/conversations/{conversation_id}/permission")).json(&json!({"request_id": request["request_id"], "option_id": option}))).await;
        assert_eq!(status, 204, "{body}");
    }

    pub async fn approved_agent_mcp(&self, token: &str, body: Value) -> Value {
        let conversation_id = self.node_conversation(token).await;
        let (reply, ()) = tokio::join!(self.agent_mcp(token, conversation_id, body), async {
            let request = self.open_request(conversation_id).await;

            self.answer(conversation_id, &request, "allow_once").await;
        });

        reply
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

pub async fn serve(router: axum::Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move { axum::serve(listener, router).await });

    format!("http://127.0.0.1:{}", address.port())
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
