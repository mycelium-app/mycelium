mod common;

use futures_util::StreamExt;
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};
use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(20);

async fn eventually<T, F, Fut>(what: &str, mut attempt: F) -> T
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
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn mycelium() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mycelium"));
    command.kill_on_drop(true);

    command
}

fn sessions_dir(node_home: &Path) -> PathBuf {
    node_home.join("state").join("mycelium").join("sessions")
}

fn keeper_is_running(node_home: &Path) -> bool {
    !common::keeper_pids(&sessions_dir(node_home)).is_empty()
}

fn fake_agent() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../node/tests/fake-acp").canonicalize().unwrap()
}

async fn create_database() -> Option<String> {
    let Ok(admin) = std::env::var("MYCELIUM_TEST_DATABASE_URL") else {
        eprintln!("SKIPPED: set MYCELIUM_TEST_DATABASE_URL to a Postgres admin URL, for example postgres://postgres:pw@127.0.0.1:5432/postgres, to run this test.");

        return None;
    };
    let name = format!("test_{}", Uuid::new_v4().simple());
    let mut connection = PgConnection::connect(&admin).await.unwrap();

    sqlx::query(sqlx::AssertSqlSafe(format!("create database {name}"))).execute(&mut connection).await.unwrap();

    let (base, _) = admin.rsplit_once('/').unwrap();

    Some(format!("{base}/{name}"))
}

struct LinkProxy {
    address: std::net::SocketAddr,
    swallow_downstream: Arc<AtomicBool>,
    connections: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    accepting: tokio::task::JoinHandle<()>,
}

impl LinkProxy {
    async fn start(upstream: std::net::SocketAddr) -> LinkProxy {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let swallow_downstream = Arc::new(AtomicBool::new(false));
        let connections: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::new()));
        let accepting = {
            let swallow = swallow_downstream.clone();
            let connections = connections.clone();

            tokio::spawn(async move {
                while let Ok((mut client, _)) = listener.accept().await {
                    let swallow = swallow.clone();
                    let handle = tokio::spawn(async move {
                        let Ok(mut server) = tokio::net::TcpStream::connect(upstream).await else {
                            return;
                        };
                        let (mut client_read, mut client_write) = client.split();
                        let (mut server_read, mut server_write) = server.split();
                        let upstream_copy = tokio::io::copy(&mut client_read, &mut server_write);
                        let downstream_copy = async {
                            let mut buffer = [0u8; 8192];

                            loop {
                                let Ok(read) = tokio::io::AsyncReadExt::read(&mut server_read, &mut buffer).await else {
                                    return;
                                };

                                if read == 0 {
                                    return;
                                }

                                if swallow.load(Ordering::SeqCst) {
                                    continue;
                                }

                                if tokio::io::AsyncWriteExt::write_all(&mut client_write, &buffer[..read]).await.is_err() {
                                    return;
                                }
                            }
                        };

                        tokio::select! {
                            _ = upstream_copy => {},
                            _ = downstream_copy => {},
                        }
                    });

                    connections.lock().unwrap().push(handle);
                }
            })
        };

        LinkProxy { address, swallow_downstream, connections, accepting }
    }

    fn swallow_what_the_control_plane_sends(&self) {
        self.swallow_downstream.store(true, Ordering::SeqCst);
    }

    fn drop_every_connection(&self) {
        self.swallow_downstream.store(false, Ordering::SeqCst);

        for handle in self.connections.lock().unwrap().drain(..) {
            handle.abort();
        }
    }
}

impl Drop for LinkProxy {
    fn drop(&mut self) {
        self.accepting.abort();
        self.drop_every_connection();
    }
}

struct Api {
    base: String,
    cookie: String,
    http: reqwest::Client,
}

impl Api {
    async fn get(&self, path: &str) -> Value {
        let response = self.http.get(format!("{}{path}", self.base)).header("cookie", &self.cookie).send().await.unwrap();
        assert!(response.status().is_success(), "GET {path} answered {}", response.status());

        response.json().await.unwrap()
    }

    async fn patch(&self, path: &str, body: Value) -> u16 {
        let response = self.http.patch(format!("{}{path}", self.base)).header("cookie", &self.cookie).json(&body).send().await.unwrap();

        response.status().as_u16()
    }

    async fn post(&self, path: &str, body: Value) -> u16 {
        let response = self.http.post(format!("{}{path}", self.base)).header("cookie", &self.cookie).json(&body).send().await.unwrap();

        response.status().as_u16()
    }

    async fn status_of(&self, conversation: Uuid) -> String {
        self.get(&format!("/api/v1/conversations/{conversation}")).await["status"].as_str().unwrap().to_string()
    }

    async fn wait_for_status(&self, conversation: Uuid, wanted: &str) {
        eventually(&format!("status {wanted}"), || async { (self.status_of(conversation).await == wanted).then_some(()) }).await;
    }

    async fn wait_for_node_offline(&self, node: Uuid) {
        eventually("the node to go offline", || async {
            let nodes = self.get("/api/v1/nodes").await;

            nodes.as_array().unwrap().iter().any(|candidate| candidate["id"] == json!(node) && candidate["online"] == false).then_some(())
        })
        .await;
    }

    async fn events(&self, conversation: Uuid) -> Vec<Value> {
        self.get(&format!("/api/v1/conversations/{conversation}/events")).await.as_array().unwrap().clone()
    }

    async fn wait_for_event_text(&self, conversation: Uuid, text: &str, occurrences: usize) {
        eventually(&format!("{occurrences} event(s) saying {text}"), || async {
            let events = self.events(conversation).await;
            let found = events.iter().filter(|event| event["kind"] == "acp_update" && event["payload"].to_string().contains(&format!("\"{text}\""))).count();

            (found >= occurrences).then_some(())
        })
        .await;
    }
}

struct Environment {
    api: Api,
    node_config: PathBuf,
    node_home: PathBuf,
    projects: PathBuf,
    data_dir: PathBuf,
    logs: PathBuf,
    streamed: Arc<Mutex<Vec<Value>>>,
    _control: Child,
    _directory: TempDir,
}

impl Drop for Environment {
    fn drop(&mut self) {
        common::kill_keepers(&sessions_dir(&self.node_home));

        if std::thread::panicking() {
            eprintln!("control plane and node logs kept in {}", self.logs.display());
        } else {
            std::fs::remove_dir_all(&self.logs).ok();
        }
    }
}

impl Environment {
    fn node_command(&self, arguments: &[&str]) -> Command {
        let mut command = mycelium();
        command
            .args(arguments)
            .env("HOME", &self.node_home)
            .env("XDG_STATE_HOME", self.node_home.join("state"))
            .env("XDG_CONFIG_HOME", self.node_home.join("config"))
            .env("MYCELIUM_NODE_CONFIG", &self.node_config)
            .env("MYCELIUM_CLAUDE_ACP", fake_agent())
            .env_remove("MYCELIUM_JOIN_TOKEN_FILE");

        command
    }

    async fn start() -> Option<Environment> {
        let database = create_database().await?;
        let directory = TempDir::new().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let base = format!("http://{address}");
        let web_dir = directory.path().join("web");
        std::fs::create_dir_all(&web_dir).unwrap();
        let node_home = directory.path().join("node");
        let projects = node_home.join("project");
        std::fs::create_dir_all(&node_home).unwrap();
        std::fs::create_dir_all(&projects).unwrap();

        let logs = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("end-to-end").join(database.rsplit('/').next().unwrap());
        std::fs::create_dir_all(&logs).unwrap();
        let control = mycelium()
            .args(["control", "serve"])
            .env("MYCELIUM_PUBLIC_URL", &base)
            .env("MYCELIUM_LISTEN", address.to_string())
            .env("MYCELIUM_DATABASE_URL", database)
            .env("MYCELIUM_DATA_DIR", directory.path().join("data"))
            .env("MYCELIUM_WEB_DIR", &web_dir)
            .env("MYCELIUM_DEV_LOGIN", "dev@example.com")
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(logs.join("control.log")).unwrap())
            .spawn()
            .unwrap();

        let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
        eventually("readyz", || async { http.get(format!("{base}/readyz")).send().await.ok().filter(|response| response.status() == 200).map(|_| ()) }).await;

        let login = http.get(format!("{base}/auth/login")).send().await.unwrap();
        let cookie = login
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with("myc_session="))
            .map(|value| value.split(';').next().unwrap().to_string())
            .expect("the dev login sets a session cookie");

        let api = Api { base, cookie, http };
        let streamed = Arc::new(Mutex::new(Vec::new()));

        Some(Environment { api, node_config: directory.path().join("node.toml"), node_home, projects, data_dir: directory.path().join("data"), logs, streamed, _control: control, _directory: directory })
    }

    async fn join_node(&self) {
        let mut join = self.node_command(&["node", "join", &self.api.base]).stdout(Stdio::piped()).spawn().unwrap();
        let mut lines = BufReader::new(join.stdout.take().unwrap()).lines();

        let user_code = loop {
            let line = tokio::time::timeout(WAIT, lines.next_line()).await.expect("join printed no code").unwrap().expect("join ended without a code");

            if let Some(code) = line.split_whitespace().map(|word| word.trim_end_matches('.')).find(|word| word.len() == 9 && word.chars().nth(4) == Some('-') && word.chars().all(|c| c == '-' || c.is_ascii_uppercase() || c.is_ascii_digit())) {
                break code.to_string();
            }
        };

        assert_eq!(self.api.post(&format!("/api/v1/device/{user_code}/approve"), json!({})).await, 204);

        tokio::spawn(async move { while lines.next_line().await.ok().flatten().is_some() {} });
        let status = tokio::time::timeout(WAIT, join.wait()).await.expect("join did not finish").unwrap();
        assert!(status.success());
    }

    fn run_daemon(&self) -> Child {
        let log = std::fs::OpenOptions::new().create(true).append(true).open(self.logs.join("node.log")).unwrap();

        self.node_command(&["node", "run"]).stdout(Stdio::null()).stderr(log).spawn().unwrap()
    }

    async fn wait_for_node_with_claude(&self) -> Uuid {
        eventually("the node online with claude", || async {
            let nodes = self.api.get("/api/v1/nodes").await;
            let node = nodes.as_array().unwrap().iter().find(|node| node["online"] == true && node["agents"].to_string().contains("claude"))?;

            Some(node["id"].as_str().unwrap().parse().unwrap())
        })
        .await
    }

    async fn follow_stream(&self) {
        let url = format!("{}/api/v1/stream", self.api.base.replacen("http", "ws", 1));
        let mut request = url.into_client_request().unwrap();
        request.headers_mut().insert("cookie", self.api.cookie.parse().unwrap());
        let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let streamed = self.streamed.clone();

        tokio::spawn(async move {
            while let Some(Ok(frame)) = socket.next().await {
                if let Message::Text(text) = frame {
                    streamed.lock().unwrap().push(serde_json::from_str(&text).unwrap());
                }
            }
        });
    }

    async fn start_conversation(&self, node: Uuid, prompt: &str) -> Uuid {
        let response = self
            .api
            .http
            .post(format!("{}/api/v1/conversations", self.api.base))
            .header("cookie", &self.api.cookie)
            .json(&json!({"node_id": node, "agent": "claude", "cwd": self.projects, "prompt": prompt}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
        let conversation: Value = response.json().await.unwrap();

        conversation["id"].as_str().unwrap().parse().unwrap()
    }
}

#[tokio::test]
async fn a_conversation_runs_through_control_node_and_agent_and_survives_a_daemon_restart() {
    let Some(environment) = Environment::start().await else {
        return;
    };
    let api = &environment.api;

    environment.join_node().await;
    let mut daemon = environment.run_daemon();
    let node = environment.wait_for_node_with_claude().await;
    environment.follow_stream().await;

    let conversation = environment.start_conversation(node, "hello").await;
    api.wait_for_status(conversation, "review").await;
    for text in ["one", "two", "three", "late"] {
        api.wait_for_event_text(conversation, text, 1).await;
    }
    assert_eq!(api.get("/api/v1/me").await["email"], "dev@example.com");

    api.post(&format!("/api/v1/conversations/{conversation}/prompt"), json!({"text": "permission please"})).await;
    api.wait_for_status(conversation, "input").await;
    let events = api.events(conversation).await;
    let request = events.iter().find(|event| event["kind"] == "permission_request").unwrap();
    let request_id = request["payload"]["request_id"].as_str().unwrap().to_string();

    api.post(&format!("/api/v1/conversations/{conversation}/prompt"), json!({"text": "queued follow-up"})).await;
    let queue = api.get(&format!("/api/v1/conversations/{conversation}/queue")).await;
    assert_eq!(queue.as_array().unwrap().len(), 1);

    assert_eq!(api.post(&format!("/api/v1/conversations/{conversation}/permission"), json!({"request_id": request_id, "option_id": "allow"})).await, 204);
    api.wait_for_event_text(conversation, "permission allow", 1).await;
    api.wait_for_event_text(conversation, "one", 2).await;
    api.wait_for_status(conversation, "review").await;
    let events = api.events(conversation).await;
    assert!(events.iter().any(|event| event["kind"] == "user_message" && event["payload"].to_string().contains("queued follow-up")));
    assert!(api.get(&format!("/api/v1/conversations/{conversation}/queue")).await.as_array().unwrap().is_empty());

    api.post(&format!("/api/v1/conversations/{conversation}/prompt"), json!({"text": "slow"})).await;
    api.wait_for_event_text(conversation, "working", 1).await;
    assert_eq!(api.post(&format!("/api/v1/conversations/{conversation}/cancel"), json!({})).await, 204);
    api.wait_for_event_text(conversation, "stopped", 1).await;
    api.wait_for_status(conversation, "review").await;

    api.post(&format!("/api/v1/conversations/{conversation}/prompt"), json!({"text": "slow again"})).await;
    api.wait_for_event_text(conversation, "working", 2).await;
    daemon.kill().await.unwrap();
    api.wait_for_node_offline(node).await;

    let mut daemon = environment.run_daemon();
    environment.wait_for_node_with_claude().await;
    assert_eq!(api.status_of(conversation).await, "working");

    assert_eq!(api.post(&format!("/api/v1/conversations/{conversation}/cancel"), json!({})).await, 204);
    api.wait_for_event_text(conversation, "stopped", 2).await;
    api.wait_for_status(conversation, "review").await;

    let events = api.events(conversation).await;
    let seqs: Vec<i64> = events.iter().map(|event| event["seq"].as_i64().unwrap()).collect();
    assert_eq!(seqs.iter().collect::<HashSet<_>>().len(), seqs.len(), "event seqs repeat: {seqs:?}");
    assert!(seqs.windows(2).all(|pair| pair[0] < pair[1]), "events are not in order: {seqs:?}");
    let working = events.iter().filter(|event| event["kind"] == "acp_update" && event["payload"].to_string().contains("\"working\"")).count();
    assert_eq!(working, 2, "the replay duplicated an update");

    eventually("the stream to carry every event once", || async {
        let streamed = environment.streamed.lock().unwrap();
        let streamed_seqs: Vec<i64> = streamed.iter().filter(|message| message["type"] == "event" && message["conversation_id"] == json!(conversation)).map(|message| message["seq"].as_i64().unwrap()).collect();

        (streamed_seqs == seqs).then_some(())
    })
    .await;

    api.post(&format!("/api/v1/conversations/{conversation}/prompt"), json!({"text": "finish"})).await;
    api.wait_for_event_text(conversation, "three", 3).await;
    api.wait_for_status(conversation, "review").await;

    assert_eq!(api.patch(&format!("/api/v1/conversations/{conversation}"), json!({"archived": true})).await, 200);
    eventually("the keeper to exit after the session is closed", || async { (!keeper_is_running(&environment.node_home)).then_some(()) }).await;
    daemon.kill().await.unwrap();
}

#[tokio::test]
async fn a_transcript_under_the_node_home_is_copied_to_the_control_plane() {
    let Some(environment) = Environment::start().await else {
        return;
    };
    environment.join_node().await;
    let transcript = environment.node_home.join(".claude").join("projects").join("-work-app").join("session-1.jsonl");
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::write(&transcript, "{\"line\":1}\n").unwrap();
    let _daemon = environment.run_daemon();
    let node = environment.wait_for_node_with_claude().await;

    let copy = environment.data_dir.join("transcripts").join(node.to_string()).join(".claude/projects/-work-app/session-1.jsonl");
    eventually("the transcript copy", || async { (std::fs::read_to_string(&copy).ok()? == "{\"line\":1}\n").then_some(()) }).await;

    let mut appended = std::fs::OpenOptions::new().append(true).open(&transcript).unwrap();
    std::io::Write::write_all(&mut appended, b"{\"line\":2}\n").unwrap();
    eventually("the appended transcript bytes", || async { (std::fs::read_to_string(&copy).ok()? == "{\"line\":1}\n{\"line\":2}\n").then_some(()) }).await;
}

#[tokio::test]
async fn an_archived_conversation_survives_a_daemon_restart_and_resumes_when_restored_and_prompted() {
    let Some(environment) = Environment::start().await else {
        return;
    };
    let api = &environment.api;
    environment.join_node().await;
    let mut daemon = environment.run_daemon();
    let node = environment.wait_for_node_with_claude().await;
    let conversation = environment.start_conversation(node, "hello").await;
    api.wait_for_status(conversation, "review").await;
    let url = format!("/api/v1/conversations/{conversation}");

    assert_eq!(api.patch(&url, json!({"archived": true})).await, 200);
    eventually("the keeper to exit after the session is closed", || async { (!keeper_is_running(&environment.node_home)).then_some(()) }).await;
    daemon.kill().await.unwrap();
    api.wait_for_node_offline(node).await;
    let mut daemon = environment.run_daemon();
    environment.wait_for_node_with_claude().await;

    assert_eq!(api.patch(&url, json!({"archived": false})).await, 200);
    assert_eq!(api.post(&format!("{url}/prompt"), json!({"text": "again"})).await, 204);
    api.wait_for_event_text(conversation, "one", 2).await;
    api.wait_for_status(conversation, "review").await;

    let events = api.events(conversation).await;
    assert!(events.iter().all(|event| event["kind"] != "error"), "the archived conversation was reported as failed: {events:?}");
    daemon.kill().await.unwrap();
}

#[tokio::test]
async fn a_prompt_lost_with_the_link_arrives_exactly_once_after_reconnect() {
    let Some(environment) = Environment::start().await else {
        return;
    };
    let api = &environment.api;

    environment.join_node().await;
    let control_address: std::net::SocketAddr = api.base.trim_start_matches("http://").parse().unwrap();
    let proxy = LinkProxy::start(control_address).await;
    let config = std::fs::read_to_string(&environment.node_config).unwrap();
    std::fs::write(&environment.node_config, config.replace(&api.base, &format!("http://{}", proxy.address))).unwrap();

    let mut daemon = environment.run_daemon();
    let node = environment.wait_for_node_with_claude().await;
    let conversation = environment.start_conversation(node, "hello").await;
    api.wait_for_status(conversation, "review").await;
    api.wait_for_event_text(conversation, "three", 1).await;

    proxy.swallow_what_the_control_plane_sends();
    api.post(&format!("/api/v1/conversations/{conversation}/prompt"), json!({"text": "second"})).await;
    api.wait_for_status(conversation, "working").await;
    proxy.drop_every_connection();

    api.wait_for_event_text(conversation, "three", 2).await;
    api.wait_for_status(conversation, "review").await;

    api.post(&format!("/api/v1/conversations/{conversation}/prompt"), json!({"text": "third"})).await;
    api.wait_for_event_text(conversation, "three", 3).await;
    api.wait_for_status(conversation, "review").await;

    let events = api.events(conversation).await;
    let turns = events.iter().filter(|event| event["kind"] == "acp_update" && event["payload"].to_string().contains("\"one\"")).count();
    assert_eq!(turns, 3, "the lost prompt ran more or less than once");

    daemon.kill().await.unwrap();
}
