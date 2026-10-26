mod common;

use axum::Json;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::response::Response;
use axum::routing::{get, post};
use base64::Engine;
use common::{Client, NodeLink, Server, serve};
use mycelium_proto::link::{ControlToNode, FolderFile, NodeToControl};
use futures_util::stream;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Default)]
struct Fake {
    requests: Mutex<Vec<Value>>,
    calls: Mutex<VecDeque<(&'static str, Value)>>,
    issued: AtomicUsize,
}

type Requests = Arc<Fake>;

fn fake_openai(calls: Vec<(&'static str, Value)>) -> (Router, Requests) {
    async fn models() -> Json<Value> {
        Json(json!({"object": "list", "data": [{"id": "llama3.2"}]}))
    }

    async fn completions(State(fake): State<Requests>, Json(request): Json<Value>) -> Response {
        fake.requests.lock().unwrap().push(request);

        let chunk = |delta: Value| format!("data: {}\n\n", json!({"choices": [{"delta": delta}]}));
        let mut lines = Vec::new();

        match fake.calls.lock().unwrap().pop_front() {
            Some((name, input)) => {
                let arguments = input.to_string();
                let (first, second) = arguments.split_at(arguments.len() / 2);

                lines.push(chunk(json!({"tool_calls": [{"index": 0, "id": format!("call_{}", fake.issued.fetch_add(1, Ordering::SeqCst)), "type": "function", "function": {"name": name, "arguments": first}}]})));
                lines.push(chunk(json!({"tool_calls": [{"index": 0, "function": {"arguments": second}}]})));
            }
            None => {
                lines.push(chunk(json!({"content": "Hello"})));
                lines.push(chunk(json!({"content": " there"})));
            }
        }

        lines.push("data: [DONE]\n\n".to_string());

        let body = stream::iter(lines.into_iter().map(|line| Ok::<Bytes, Infallible>(Bytes::from(line))));

        Response::builder().header("content-type", "text/event-stream").body(Body::from_stream(body)).unwrap()
    }

    let fake = Requests::new(Fake { calls: Mutex::new(calls.into()), ..Fake::default() });
    let router = Router::new().route("/v1/models", get(models)).route("/v1/chat/completions", post(completions)).with_state(fake.clone());

    (router, fake)
}

async fn scripted_provider(server: &Server, calls: Vec<(&'static str, Value)>) -> (Value, Requests) {
    let (router, fake) = fake_openai(calls);
    let base = serve(router).await;
    let (status, provider) = server.user.json(server.user.post("/api/v1/model-providers").json(&json!({"kind": "openai_compatible", "name": "Ollama", "base_url": format!("{base}/v1")}))).await;
    assert_eq!(status, 201, "{provider}");

    (provider, fake)
}

async fn chat_provider(server: &Server) -> (Value, Requests) {
    scripted_provider(server, Vec::new()).await
}

async fn allow_calls(server: &Server, id: &str, count: usize) {
    for _ in 0..count {
        let request = common::eventually("a permission request", || async {
            let events = server.events(id.parse().unwrap()).await;
            let open = events.iter().find(|event| event["kind"] == "permission_request" && !events.iter().any(|other| other["kind"] == "permission_resolved" && other["payload"]["request_id"] == event["payload"]["request_id"]));

            open.map(|event| event["payload"].clone())
        })
        .await;
        let (status, body) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/permission")).json(&json!({"request_id": request["request_id"], "option_id": "allow_once"}))).await;
        assert_eq!(status, 204, "{body}");

        common::eventually("the call to be allowed", || async { server.events(id.parse().unwrap()).await.iter().any(|event| event["kind"] == "permission_resolved" && event["payload"]["request_id"] == request["request_id"]).then_some(()) }).await;
    }
}

async fn finished_turns(server: &Server, id: &str, turns: usize) {
    let id: Uuid = id.parse().unwrap();

    common::eventually("the reply to end", || async { (server.events(id).await.iter().filter(|event| event["kind"] == "turn_end").count() == turns).then_some(()) }).await;
}

async fn prompt(client: &Client, id: &str, text: &str) -> Value {
    let (status, conversation) = client.json(client.post(&format!("/api/v1/assistants/{id}/prompt")).json(&json!({"text": text}))).await;
    assert_eq!(status, 200, "{conversation}");

    conversation
}

async fn enable(client: &Client) {
    let flags = json!({"tasks": false, "workflows": false, "instructions": true, "details": false, "subagents": false, "walkthrough": false, "move_chats": false, "groups": true, "continue_elsewhere": false, "assistants": true});
    let (status, _) = client.json(client.put("/api/v1/experiments").json(&flags)).await;
    assert_eq!(status, 200);
}

async fn create(client: &Client, body: Value) -> Value {
    let (status, assistant) = client.json(client.post("/api/v1/assistants").json(&body)).await;
    assert_eq!(status, 201, "{assistant}");

    assistant
}

#[tokio::test]
async fn assistants_are_refused_until_the_experiment_is_on() {
    let server = Server::start().await;
    let (status, refused) = server.user.json(server.user.get("/api/v1/assistants")).await;
    assert_eq!((status, refused["error"].clone()), (403, json!("experiment_disabled")));

    enable(&server.user).await;
    let (status, listed) = server.user.json(server.user.get("/api/v1/assistants")).await;
    assert_eq!((status, listed), (200, json!([])));
}

#[tokio::test]
async fn an_assistant_is_created_listed_pinned_and_archived_per_person() {
    let server = Server::start().await;
    enable(&server.user).await;
    let (_, group) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": "acme"}))).await;
    let created = create(&server.user, json!({"name": "Stand-up", "group_id": group["id"], "instructions": "# Stand-up\nCollect what everyone did."})).await;
    assert_eq!((created["name"].clone(), created["group_id"].clone(), created["state"].clone()), (json!("Stand-up"), group["id"].clone(), json!("idle")));
    assert_eq!(created["description"], "Stand-up");

    let id = created["id"].as_str().unwrap();
    let (_, fetched) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}"))).await;
    assert_eq!(fetched["instructions"], "# Stand-up\nCollect what everyone did.");

    let (status, pinned) = server.user.json(server.user.patch(&format!("/api/v1/assistants/{id}")).json(&json!({"pinned": true, "description": "Daily summary"}))).await;
    assert_eq!(status, 200, "{pinned}");
    assert!(pinned["pinned_at"].is_string());
    assert_eq!(pinned["description"], "Daily summary");

    let other = server.second_user("other@example.com").await;
    enable(&other).await;
    let (_, theirs) = other.json(other.get("/api/v1/assistants")).await;
    assert_eq!(theirs, json!([]));
    let (status, _) = other.json(other.patch(&format!("/api/v1/assistants/{id}")).json(&json!({"archived": true}))).await;
    assert_eq!(status, 404);

    server.user.json(server.user.patch(&format!("/api/v1/assistants/{id}")).json(&json!({"archived": true, "clear_group": true}))).await;
    let (_, active) = server.user.json(server.user.get("/api/v1/assistants")).await;
    let (_, archived) = server.user.json(server.user.get("/api/v1/assistants?archived=true")).await;
    assert_eq!(active, json!([]));
    assert_eq!(archived[0]["id"], id);
    assert!(archived[0].get("group_id").is_none());
}

#[tokio::test]
async fn a_duplicate_name_is_refused_with_a_message_naming_it() {
    let server = Server::start().await;
    enable(&server.user).await;
    create(&server.user, json!({"name": "War room"})).await;

    let (status, refused) = server.user.json(server.user.post("/api/v1/assistants").json(&json!({"name": "War room"}))).await;
    assert_eq!((status, refused["error"].clone()), (409, json!("name_taken")));
    assert!(refused["message"].as_str().unwrap().contains("War room"));

    let (status, blank) = server.user.json(server.user.post("/api/v1/assistants").json(&json!({"name": "  "}))).await;
    assert_eq!((status, blank["message"].clone()), (400, json!("Enter a name for the assistant.")));
}

#[tokio::test]
async fn preferences_naming_an_unknown_node_or_a_model_less_choice_are_refused() {
    let server = Server::start().await;
    enable(&server.user).await;

    let (status, refused) = server.user.json(server.user.post("/api/v1/assistants").json(&json!({"name": "A", "preferences": {"nodes": [uuid::Uuid::new_v4()]}}))).await;
    assert_eq!((status, refused["message"].clone()), (400, json!("That node is not one of yours")));

    let (status, refused) = server.user.json(server.user.post("/api/v1/assistants").json(&json!({"name": "A", "preferences": {"models": [{}]}}))).await;
    assert_eq!((status, refused["message"].clone()), (400, json!("Choose a model for this assistant")));
}

#[tokio::test]
async fn the_first_prompt_starts_a_chat_session_and_later_prompts_reuse_it() {
    let server = Server::start().await;
    enable(&server.user).await;
    let (provider, fake) = chat_provider(&server).await;
    let assistant = create(&server.user, json!({"name": "Stand-up", "preferences": {"models": [{"provider_id": provider["id"], "model": "llama3.2"}]}})).await;
    let id = assistant["id"].as_str().unwrap();

    let first = prompt(&server.user, id, "What did we ship?").await;
    finished_turns(&server, first["id"].as_str().unwrap(), 1).await;
    let second = prompt(&server.user, id, "And yesterday?").await;
    finished_turns(&server, second["id"].as_str().unwrap(), 2).await;

    assert_eq!(first["id"], second["id"]);
    assert_eq!((first["agent"].clone(), first["assistant_id"].clone()), (json!("chat"), json!(id)));
    let (_, fetched) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}"))).await;
    assert_eq!((fetched["conversation_id"].clone(), fetched["session_count"].clone()), (first["id"].clone(), json!(1)));
    assert_eq!(fake.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn a_prompt_with_no_chat_model_chosen_says_how_to_fix_it() {
    let server = Server::start().await;
    enable(&server.user).await;
    let assistant = create(&server.user, json!({"name": "Stand-up"})).await;

    let (status, refused) = server.user.json(server.user.post(&format!("/api/v1/assistants/{}/prompt", assistant["id"].as_str().unwrap())).json(&json!({"text": "Hello"}))).await;

    assert_eq!((status, refused["error"].clone()), (400, json!("model_required")));
    assert!(refused["message"].as_str().unwrap().contains("settings"));
}

#[tokio::test]
async fn clear_starts_a_new_session_and_keeps_instructions_and_group() {
    let server = Server::start().await;
    enable(&server.user).await;
    let (provider, _) = chat_provider(&server).await;
    let (_, group) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": "acme"}))).await;
    let assistant = create(&server.user, json!({"name": "Stand-up", "group_id": group["id"], "instructions": "Be brief.", "preferences": {"models": [{"provider_id": provider["id"], "model": "llama3.2"}]}})).await;
    let id = assistant["id"].as_str().unwrap();
    let first = prompt(&server.user, id, "Hello").await;
    finished_turns(&server, first["id"].as_str().unwrap(), 1).await;

    let (status, cleared) = server.user.json(server.user.post(&format!("/api/v1/assistants/{id}/clear"))).await;
    assert_eq!(status, 200, "{cleared}");
    assert!(cleared.get("conversation_id").is_none());

    let second = prompt(&server.user, id, "Hello again").await;
    let (_, fetched) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}"))).await;
    assert_ne!(first["id"], second["id"]);
    assert_eq!((fetched["group_id"].clone(), fetched["instructions"].clone(), fetched["session_count"].clone()), (group["id"].clone(), json!("Be brief."), json!(2)));
}

#[tokio::test]
async fn assistant_sessions_take_the_assistants_group_and_its_agents_md_as_instructions() {
    let server = Server::start().await;
    enable(&server.user).await;
    let (provider, fake) = chat_provider(&server).await;
    let (_, group) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": "acme"}))).await;
    server.user.json(server.user.put("/api/v1/instructions/global").json(&json!({"text": "Global rule."}))).await;
    let (status, _) = server.user.json(server.user.put(&format!("/api/v1/groups/{}/instructions", group["id"].as_str().unwrap())).json(&json!({"text": "Group rule."}))).await;
    assert_eq!(status, 200);
    let assistant = create(&server.user, json!({"name": "Stand-up", "group_id": group["id"], "instructions": "Assistant rule.", "preferences": {"models": [{"provider_id": provider["id"], "model": "llama3.2"}]}})).await;

    let conversation = prompt(&server.user, assistant["id"].as_str().unwrap(), "Hello").await;
    finished_turns(&server, conversation["id"].as_str().unwrap(), 1).await;

    let system = fake.requests.lock().unwrap()[0]["messages"][0]["content"].as_str().unwrap().to_string();
    let (global, group_rule, assistant_rule) = (system.find("Global rule.").unwrap(), system.find("Group rule.").unwrap(), system.find("Assistant rule.").unwrap());
    assert!(global < group_rule && group_rule < assistant_rule, "{system}");
}

#[tokio::test]
async fn assistant_sessions_carry_their_assistant_id_in_the_conversation_api() {
    let server = Server::start().await;
    enable(&server.user).await;
    let (provider, _) = chat_provider(&server).await;
    let (_, group) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": "acme"}))).await;
    let assistant = create(&server.user, json!({"name": "Stand-up", "group_id": group["id"], "preferences": {"models": [{"provider_id": provider["id"], "model": "llama3.2"}]}})).await;
    let conversation = prompt(&server.user, assistant["id"].as_str().unwrap(), "Hello").await;
    let id = conversation["id"].as_str().unwrap();

    let (_, listed) = server.user.json(server.user.get("/api/v1/conversations")).await;
    let (_, fetched) = server.user.json(server.user.get(&format!("/api/v1/conversations/{id}"))).await;

    let listed = listed.as_array().unwrap().iter().find(|row| row["id"] == json!(id)).unwrap().clone();
    assert_eq!((listed["assistant_id"].clone(), listed["group_id"].clone()), (assistant["id"].clone(), group["id"].clone()));
    assert_eq!(fetched["assistant_id"], assistant["id"]);
}

fn tool_results(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .filter(|event| event["kind"] == "acp_update" && event["payload"]["update"]["sessionUpdate"] == "tool_call_update")
        .map(|event| event["payload"]["update"]["content"][0]["content"]["text"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[tokio::test]
async fn file_tools_read_and_write_storage_from_a_chat_and_refuse_outside_an_assistant() {
    let server = Server::start().await;
    enable(&server.user).await;
    let calls = vec![
        ("mcp__mycelium__assistant_write_file", json!({"path": "notes/today.md", "content": "Shipped v1"})),
        ("mcp__mycelium__assistant_read_file", json!({"path": "notes/today.md"})),
        ("mcp__mycelium__assistant_list_files", json!({})),
        ("mcp__mycelium__assistant_delete_file", json!({"path": "notes/today.md"})),
        ("mcp__mycelium__assistant_list_files", json!({})),
    ];
    let (provider, _) = scripted_provider(&server, calls).await;
    let assistant = create(&server.user, json!({"name": "Stand-up", "instructions": "Be brief.", "preferences": {"models": [{"provider_id": provider["id"], "model": "llama3.2"}]}})).await;
    let id = assistant["id"].as_str().unwrap();
    let conversation = prompt(&server.user, id, "Keep a note").await;
    let chat = conversation["id"].as_str().unwrap();

    allow_calls(&server, chat, 5).await;
    finished_turns(&server, chat, 1).await;

    let results = tool_results(&server.events(chat.parse().unwrap()).await);
    assert_eq!(results[0], "Wrote notes/today.md.");
    assert_eq!(results[1], "Shipped v1");
    assert!(results[2].contains("AGENTS.md") && results[2].contains("notes/today.md (10 bytes)"), "{}", results[2]);
    assert_eq!(results[3], "Deleted notes/today.md.");
    assert!(!results[4].contains("notes/today.md"));
}

#[tokio::test]
async fn the_file_tools_refuse_a_chat_that_does_not_belong_to_an_assistant() {
    let server = Server::start().await;
    enable(&server.user).await;
    let (provider, _) = scripted_provider(&server, vec![("mcp__mycelium__assistant_list_files", json!({}))]).await;
    let (status, chat) = server.user.json(server.user.post("/api/v1/chats").json(&json!({"provider_id": provider["id"], "prompt": "List my files"}))).await;
    assert_eq!(status, 201, "{chat}");
    let id = chat["id"].as_str().unwrap();

    allow_calls(&server, id, 1).await;
    finished_turns(&server, id, 1).await;

    assert_eq!(tool_results(&server.events(id.parse().unwrap()).await), ["This conversation does not belong to an assistant."]);
}

#[tokio::test]
async fn files_are_stored_listed_read_and_deleted_over_http_per_assistant() {
    let server = Server::start().await;
    enable(&server.user).await;
    let assistant = create(&server.user, json!({"name": "Stand-up", "instructions": "# Stand-up"})).await;
    let id = assistant["id"].as_str().unwrap();

    let (status, written) = server.user.json(server.user.put(&format!("/api/v1/assistants/{id}/files/notes/today.md")).body("Shipped v1")).await;
    assert_eq!((status, written["path"].clone(), written["size"].clone()), (200, json!("notes/today.md"), json!(10)));

    let read = server.user.get(&format!("/api/v1/assistants/{id}/files/notes/today.md")).send().await.unwrap();
    assert_eq!((read.status().as_u16(), read.text().await.unwrap()), (200, "Shipped v1".to_string()));

    let (_, listed) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}/files"))).await;
    let paths: Vec<&str> = listed.as_array().unwrap().iter().map(|file| file["path"].as_str().unwrap()).collect();
    assert_eq!(paths, ["AGENTS.md", "notes/today.md"]);

    let other = server.second_user("other@example.com").await;
    enable(&other).await;
    let (status, _) = other.json(other.get(&format!("/api/v1/assistants/{id}/files"))).await;
    assert_eq!(status, 404);

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/assistants/{id}/files/notes/today.md"))).await;
    let (missing, _) = server.user.json(server.user.delete(&format!("/api/v1/assistants/{id}/files/notes/today.md"))).await;
    assert_eq!((status, missing), (204, 404));
}

#[tokio::test]
async fn writing_agents_md_changes_the_instructions_and_the_description() {
    let server = Server::start().await;
    enable(&server.user).await;
    let assistant = create(&server.user, json!({"name": "Stand-up", "instructions": "# Old"})).await;
    let id = assistant["id"].as_str().unwrap();

    server.user.json(server.user.put(&format!("/api/v1/assistants/{id}/files/AGENTS.md")).body("# New focus\nBe brief.")).await;

    let (_, fetched) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}"))).await;
    assert_eq!((fetched["instructions"].clone(), fetched["description"].clone()), (json!("# New focus\nBe brief."), json!("New focus")));
}

#[tokio::test]
async fn paths_outside_the_storage_and_files_over_a_mebibyte_are_refused_with_a_way_forward() {
    let server = Server::start().await;
    enable(&server.user).await;
    let assistant = create(&server.user, json!({"name": "Stand-up"})).await;
    let id = assistant["id"].as_str().unwrap();

    let (status, escaped) = server.user.json(server.user.put(&format!("/api/v1/assistants/{id}/files/notes/..%2Fsecret")).body("x")).await;
    assert_eq!((status, escaped["error"].clone()), (400, json!("path_invalid")));

    let (status, large) = server.user.json(server.user.put(&format!("/api/v1/assistants/{id}/files/big.bin")).body(vec![b'a'; 1024 * 1024 + 1])).await;
    assert_eq!((status, large["error"].clone()), (413, json!("file_too_large")));
    assert!(large["message"].as_str().unwrap().contains("split"));
}

fn encoded(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

async fn next_matching<T>(link: &mut NodeLink, pick: impl Fn(&ControlToNode) -> Option<T>) -> T {
    loop {
        let message = link.next().await;

        if let Some(found) = pick(&message) {
            return found;
        }
    }
}

async fn harness_assistant(server: &Server, nodes: Value) -> Value {
    create(&server.user, json!({"name": "War room", "instructions": "Run the war room.", "preferences": {"models": [{"agent": "claude"}], "nodes": nodes}})).await
}

#[tokio::test]
async fn a_workspace_turn_copies_storage_down_and_changes_back() {
    let server = Server::start().await;
    enable(&server.user).await;
    let mut link = server.online_node("laptop").await;
    let assistant = harness_assistant(&server, json!([link.id])).await;
    let id = assistant["id"].as_str().unwrap();
    server.user.json(server.user.put(&format!("/api/v1/assistants/{id}/files/notes/old.md")).body("old")).await;

    let started = prompt(&server.user, id, "Open the war room").await;
    let conversation: Uuid = started["id"].as_str().unwrap().parse().unwrap();

    let (root, files, replace) = next_matching(&mut link, |message| match message {
        ControlToNode::WriteFolder { root, files, replace } => Some((root.clone(), files.clone(), *replace)),
        _ => None,
    })
    .await;
    let (cwd, instructions) = next_matching(&mut link, |message| match message {
        ControlToNode::StartSession { cwd, instructions, .. } => Some((cwd.clone(), instructions.clone())),
        _ => None,
    })
    .await;

    assert_eq!((root.clone(), cwd), (format!(".mycelium-assistants/{id}"), root));
    assert!(replace);
    assert_eq!(files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), vec!["AGENTS.md", "notes/old.md"]);
    assert!(instructions.unwrap().contains("Run the war room."));

    link.start_session(conversation, "acp-1").await;
    let (_, text) = link.expect_prompt().await;
    assert_eq!(text, "Open the war room");

    link.send(NodeToControl::TurnEnd { conversation_id: conversation, seq: 3, stop_reason: "end_turn".into(), usage: None }).await;
    let request_id = next_matching(&mut link, |message| match message {
        ControlToNode::ReadFolder { request_id, .. } => Some(*request_id),
        _ => None,
    })
    .await;
    let changed = vec![FolderFile { path: "AGENTS.md".into(), data_base64: encoded("Run the war room.") }, FolderFile { path: "notes/today.md".into(), data_base64: encoded("Shipped v1") }];
    link.send(NodeToControl::FolderFiles { request_id, files: changed, truncated: false, error: None }).await;

    common::eventually("storage to follow the folder", || async {
        let (_, listed) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}/files"))).await;
        let paths: Vec<&str> = listed.as_array().unwrap().iter().map(|file| file["path"].as_str().unwrap()).collect();

        (paths == ["AGENTS.md", "notes/today.md"]).then_some(())
    })
    .await;
}

#[tokio::test]
async fn use_workspace_moves_the_thread_to_the_first_online_node() {
    let server = Server::start().await;
    enable(&server.user).await;
    let mut link = server.online_node("laptop").await;
    let (provider, _) = scripted_provider(&server, vec![("mcp__mycelium__assistant_use_workspace", json!({"reason": "Needs a repository"}))]).await;
    let assistant = create(&server.user, json!({"name": "Stand-up", "instructions": "Be brief.", "preferences": {"models": [{"provider_id": provider["id"], "model": "llama3.2"}, {"agent": "claude"}], "nodes": [link.id]}})).await;
    let id = assistant["id"].as_str().unwrap();
    let started = prompt(&server.user, id, "Clone the repository").await;
    let chat = started["id"].as_str().unwrap();

    allow_calls(&server, chat, 1).await;

    let folder = next_matching(&mut link, |message| match message {
        ControlToNode::WriteFolder { root, .. } => Some(root.clone()),
        _ => None,
    })
    .await;
    let cwd = next_matching(&mut link, |message| match message {
        ControlToNode::StartSession { conversation_id, cwd, .. } if conversation_id.to_string() == chat => Some(cwd.clone()),
        _ => None,
    })
    .await;
    let (_, moved) = server.user.json(server.user.get(&format!("/api/v1/conversations/{chat}"))).await;

    assert_eq!((folder.clone(), cwd), (format!(".mycelium-assistants/{id}"), folder));
    assert_eq!((moved["agent"].clone(), moved["assistant_id"].clone()), (json!("claude"), json!(id)));
    let events = server.events(chat.parse().unwrap()).await;
    let continued = events.iter().find(|event| event["kind"] == "continued").unwrap();
    assert_eq!(continued["payload"]["node_id"], json!(link.id));
}

#[tokio::test]
async fn an_offline_first_node_falls_back_to_the_next_node_before_starting() {
    let server = Server::start().await;
    enable(&server.user).await;
    let (offline, _) = server.join("node", "willow").await;
    let mut link = server.online_node("orion").await;
    let assistant = harness_assistant(&server, json!([offline, link.id])).await;

    prompt(&server.user, assistant["id"].as_str().unwrap(), "Start").await;

    let started = next_matching(&mut link, |message| matches!(message, ControlToNode::StartSession { .. }).then_some(())).await;
    assert_eq!(started, ());
}

#[tokio::test]
async fn a_prompt_with_no_node_online_says_what_to_bring_online() {
    let server = Server::start().await;
    enable(&server.user).await;
    let (offline, _) = server.join("node", "willow").await;
    let assistant = harness_assistant(&server, json!([offline])).await;

    let (status, refused) = server.user.json(server.user.post(&format!("/api/v1/assistants/{}/prompt", assistant["id"].as_str().unwrap())).json(&json!({"text": "Start"}))).await;

    assert_eq!((status, refused["error"].clone()), (409, json!("no_choice_available")));
}

fn limit_message() -> String {
    let resets = chrono::Utc::now() + chrono::Duration::hours(2);

    format!("Internal error: You've hit your session limit · resets {} (UTC)", resets.format("%-I:%M%P"))
}

async fn named_provider(server: &Server, name: &str, router: Router) -> Value {
    let base = serve(router).await;
    let (status, provider) = server.user.json(server.user.post("/api/v1/model-providers").json(&json!({"kind": "openai_compatible", "name": name, "base_url": format!("{base}/v1")}))).await;
    assert_eq!(status, 201, "{provider}");

    provider
}

fn broken_openai() -> Router {
    async fn models() -> Json<Value> {
        Json(json!({"object": "list", "data": [{"id": "llama3.2"}]}))
    }

    async fn completions() -> (axum::http::StatusCode, &'static str) {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "model is down")
    }

    Router::new().route("/v1/models", get(models)).route("/v1/chat/completions", post(completions))
}

#[tokio::test]
async fn a_usage_limit_moves_the_thread_to_the_next_model_and_records_why() {
    let server = Server::start().await;
    enable(&server.user).await;
    let mut link = server.online_node("laptop").await;
    let (provider, fake) = chat_provider(&server).await;
    let assistant = create(&server.user, json!({"name": "War room", "preferences": {"chat_first": false, "nodes": [link.id], "models": [{"agent": "claude"}, {"provider_id": provider["id"], "model": "llama3.2"}]}})).await;
    let id = assistant["id"].as_str().unwrap();
    let started = prompt(&server.user, id, "Open the war room").await;
    let conversation: Uuid = started["id"].as_str().unwrap().parse().unwrap();

    next_matching(&mut link, |message| matches!(message, ControlToNode::StartSession { .. }).then_some(())).await;
    link.start_session(conversation, "acp-1").await;
    link.expect_prompt().await;
    link.send(NodeToControl::SessionFailed { conversation_id: conversation, seq: 3, message: limit_message() }).await;

    finished_turns(&server, &conversation.to_string(), 1).await;
    let moved = server.conversation(conversation).await;
    let events = server.events(conversation).await;
    let continued = events.iter().find(|event| event["kind"] == "continued").unwrap();
    let (_, fetched) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}"))).await;

    assert_eq!((moved["agent"].clone(), continued["payload"]["reason"].clone()), (json!("chat"), json!("usage_limit")));
    assert_eq!(fake.requests.lock().unwrap().len(), 1);
    assert_eq!(fetched["next_choice"], json!("Ollama · llama3.2"));
}

#[tokio::test]
async fn a_failing_chat_model_moves_the_turn_to_the_next_model() {
    let server = Server::start().await;
    enable(&server.user).await;
    let broken = named_provider(&server, "Broken", broken_openai()).await;
    let (router, fake) = fake_openai(Vec::new());
    let working = named_provider(&server, "Working", router).await;
    let assistant = create(&server.user, json!({"name": "Stand-up", "preferences": {"fall_back_on": ["model_error"], "models": [{"provider_id": broken["id"], "model": "llama3.2"}, {"provider_id": working["id"], "model": "llama3.2"}]}})).await;

    let started = prompt(&server.user, assistant["id"].as_str().unwrap(), "What did we ship?").await;
    let chat = started["id"].as_str().unwrap();

    common::eventually("the reply from the next model", || async { (!fake.requests.lock().unwrap().is_empty()).then_some(()) }).await;
    let events = server.events(chat.parse().unwrap()).await;
    let continued = events.iter().find(|event| event["kind"] == "continued").unwrap();
    let conversation = server.conversation(chat.parse().unwrap()).await;

    assert_eq!((continued["payload"]["reason"].clone(), continued["payload"]["kind"].clone()), (json!("model_error"), json!("model")));
    assert_eq!(conversation["provider_id"], working["id"]);
}

#[tokio::test]
async fn a_session_whose_node_went_offline_moves_on_before_the_next_prompt() {
    let server = Server::start().await;
    enable(&server.user).await;
    let mut link = server.online_node("laptop").await;
    let node_id = link.id;
    let (provider, _) = chat_provider(&server).await;
    let assistant = create(&server.user, json!({"name": "War room", "preferences": {"chat_first": false, "nodes": [node_id], "models": [{"agent": "claude"}, {"provider_id": provider["id"], "model": "llama3.2"}]}})).await;
    let id = assistant["id"].as_str().unwrap();
    let started = prompt(&server.user, id, "Open the war room").await;
    let conversation: Uuid = started["id"].as_str().unwrap().parse().unwrap();

    next_matching(&mut link, |message| matches!(message, ControlToNode::StartSession { .. }).then_some(())).await;
    link.start_session(conversation, "acp-1").await;
    link.expect_prompt().await;
    link.send(NodeToControl::TurnEnd { conversation_id: conversation, seq: 3, stop_reason: "end_turn".into(), usage: None }).await;
    common::eventually("the turn to end", || async { (server.conversation(conversation).await["status"] == "review").then_some(()) }).await;
    drop(link);
    common::eventually("the node to go offline", || async {
        let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;

        nodes.as_array().unwrap().iter().any(|node| node["id"] == json!(node_id) && node["online"] == false).then_some(())
    })
    .await;

    prompt(&server.user, id, "Are you there?").await;

    let moved = server.conversation(conversation).await;
    let events = server.events(conversation).await;
    let continued = events.iter().find(|event| event["kind"] == "continued").unwrap();
    assert_eq!((moved["agent"].clone(), continued["payload"]["reason"].clone()), (json!("chat"), json!("offline")));
}

fn events_text(events: &[Value]) -> String {
    serde_json::to_string(events).unwrap()
}

#[tokio::test]
async fn secrets_are_never_returned_reach_the_node_as_environment_and_are_redacted_from_events() {
    let server = Server::start().await;
    enable(&server.user).await;
    let mut link = server.online_node("laptop").await;
    let assistant = harness_assistant(&server, json!([link.id])).await;
    let id = assistant["id"].as_str().unwrap();

    let (status, saved) = server.user.json(server.user.put(&format!("/api/v1/assistants/{id}/secrets/GITHUB_TOKEN")).json(&json!({"value": "ghp_supersecret", "description": "Pushes"}))).await;
    assert_eq!((status, saved["state"].clone(), saved["set_by"].clone()), (200, json!("set"), json!("person")));
    let (_, listed) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}/secrets"))).await;
    let (_, fetched) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}"))).await;
    assert!(!listed.to_string().contains("supersecret") && !fetched.to_string().contains("supersecret"));
    assert_eq!(fetched["secrets"][0]["name"], json!("GITHUB_TOKEN"));

    let started = prompt(&server.user, id, "Push the branch").await;
    let conversation: Uuid = started["id"].as_str().unwrap().parse().unwrap();
    let env = next_matching(&mut link, |message| match message {
        ControlToNode::StartSession { env, .. } => Some(env.clone()),
        _ => None,
    })
    .await;
    assert_eq!(env, vec![("GITHUB_TOKEN".to_string(), "ghp_supersecret".to_string())]);

    link.start_session(conversation, "acp-1").await;
    link.send(NodeToControl::AcpUpdate { conversation_id: conversation, seq: 2, update: json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Using ghp_supersecret now"}}) }).await;
    link.barrier(conversation, 3).await;

    let text = events_text(&server.events(conversation).await);
    assert!(!text.contains("supersecret") && text.contains("Using [redacted] now"), "{text}");
}

#[tokio::test]
async fn request_secret_creates_a_waiting_secret_the_person_can_fill() {
    let server = Server::start().await;
    enable(&server.user).await;
    let calls = vec![("mcp__mycelium__assistant_request_secret", json!({"name": "JELLYFIN_KEY", "description": "From Jellyfin, Dashboard, API keys"}))];
    let (provider, _) = scripted_provider(&server, calls).await;
    let assistant = create(&server.user, json!({"name": "Jellyfin", "preferences": {"models": [{"provider_id": provider["id"], "model": "llama3.2"}]}})).await;
    let id = assistant["id"].as_str().unwrap();
    let chat = prompt(&server.user, id, "Check the library").await["id"].as_str().unwrap().to_string();

    allow_calls(&server, &chat, 1).await;
    finished_turns(&server, &chat, 1).await;

    let (_, waiting) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}/secrets"))).await;
    assert_eq!((waiting[0]["name"].clone(), waiting[0]["state"].clone(), waiting[0]["description"].clone()), (json!("JELLYFIN_KEY"), json!("waiting"), json!("From Jellyfin, Dashboard, API keys")));

    let (status, filled) = server.user.json(server.user.put(&format!("/api/v1/assistants/{id}/secrets/JELLYFIN_KEY")).json(&json!({"value": "abc123def"}))).await;
    assert_eq!((status, filled["state"].clone(), filled["description"].clone()), (200, json!("set"), json!("From Jellyfin, Dashboard, API keys")));
}

#[tokio::test]
async fn a_secret_the_assistant_stores_is_redacted_from_its_own_tool_call() {
    let server = Server::start().await;
    enable(&server.user).await;
    let calls = vec![("mcp__mycelium__assistant_set_secret", json!({"name": "API_KEY", "value": "sk-live-12345"}))];
    let (provider, _) = scripted_provider(&server, calls).await;
    let assistant = create(&server.user, json!({"name": "Stand-up", "preferences": {"models": [{"provider_id": provider["id"], "model": "llama3.2"}]}})).await;
    let id = assistant["id"].as_str().unwrap();
    let chat = prompt(&server.user, id, "Remember my key sk-live-12345").await["id"].as_str().unwrap().to_string();

    allow_calls(&server, &chat, 1).await;
    finished_turns(&server, &chat, 1).await;

    let text = events_text(&server.events(chat.parse().unwrap()).await);
    let (_, secrets) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}/secrets"))).await;
    assert!(!text.contains("sk-live-12345"), "{text}");
    assert_eq!((secrets[0]["state"].clone(), secrets[0]["set_by"].clone()), (json!("set"), json!("assistant")));
}

#[tokio::test]
async fn a_secret_name_that_is_not_an_environment_variable_is_refused_with_an_example() {
    let server = Server::start().await;
    enable(&server.user).await;
    let assistant = create(&server.user, json!({"name": "Stand-up"})).await;

    let (status, refused) = server.user.json(server.user.put(&format!("/api/v1/assistants/{}/secrets/github-token", assistant["id"].as_str().unwrap())).json(&json!({"value": "abcdefgh"}))).await;

    assert_eq!((status, refused["error"].clone()), (400, json!("secret_name_invalid")));
    assert!(refused["message"].as_str().unwrap().contains("GITHUB_TOKEN"));
}

type Repository = Arc<Mutex<std::collections::BTreeMap<String, Vec<u8>>>>;

fn fake_forge(repository: Repository) -> Router {
    use axum::extract::Path as UrlPath;
    use axum::http::StatusCode;

    async fn read(State(repository): State<Repository>, UrlPath((_, _, path)): UrlPath<(String, String, String)>) -> Result<Json<Value>, StatusCode> {
        let files = repository.lock().unwrap();
        let content = files.get(&path).ok_or(StatusCode::NOT_FOUND)?;

        Ok(Json(json!({"content": base64::engine::general_purpose::STANDARD.encode(content)})))
    }

    async fn write(State(repository): State<Repository>, UrlPath((_, _, path)): UrlPath<(String, String, String)>, Json(body): Json<Value>) -> Json<Value> {
        let content = base64::engine::general_purpose::STANDARD.decode(body["content"].as_str().unwrap()).unwrap();
        repository.lock().unwrap().insert(path, content);

        Json(json!({"commit": {"sha": "commit-1"}}))
    }

    async fn remove(State(repository): State<Repository>, UrlPath((_, _, path)): UrlPath<(String, String, String)>) -> Json<Value> {
        repository.lock().unwrap().remove(&path);

        Json(json!({"commit": {"sha": "commit-2"}}))
    }

    async fn tree(State(repository): State<Repository>) -> Json<Value> {
        let files = repository.lock().unwrap();

        Json(json!({"tree": files.keys().map(|path| json!({"path": path, "type": "blob", "sha": format!("blob-{path}")})).collect::<Vec<_>>()}))
    }

    Router::new()
        .route("/api/v3/user", get(|| async { Json(json!({"login": "samexample"})) }))
        .route("/api/v3/user/orgs", get(|| async { Json(json!([])) }))
        .route("/api/v3/repos/{owner}/{name}", get(|| async { Json(json!({"default_branch": "main"})) }))
        .route("/api/v3/repos/{owner}/{name}/git/trees/{branch}", get(tree))
        .route("/api/v3/repos/{owner}/{name}/contents/{*path}", get(read).put(write).delete(remove))
        .with_state(repository)
}

async fn backed_up_assistant(server: &Server, repository: &Repository) -> (String, Value) {
    let base = serve(fake_forge(repository.clone())).await;
    let (status, forge) = server.user.json(server.user.post("/api/v1/forges").json(&json!({"kind": "github", "base_url": base, "token": "ghp_0123456789abcdefghijklmnopqrstuvwxyz"}))).await;
    assert_eq!(status, 201, "{forge}");
    let assistant = create(&server.user, json!({"name": "Stand-up", "instructions": "Be brief."})).await;
    let id = assistant["id"].as_str().unwrap().to_string();

    (id, forge)
}

fn remote_paths(repository: &Repository) -> Vec<String> {
    repository.lock().unwrap().keys().cloned().collect()
}

#[tokio::test]
async fn backup_commits_changed_files_skips_gitignored_ones_and_follows_deletions() {
    let server = Server::start().await;
    enable(&server.user).await;
    let repository = Repository::default();
    let (id, forge) = backed_up_assistant(&server, &repository).await;
    for (path, text) in [(".gitignore", "secret/\n*.tmp\n"), ("notes/a.md", "Shipped"), ("secret/x.md", "hidden"), ("cache.tmp", "temporary")] {
        server.user.json(server.user.put(&format!("/api/v1/assistants/{id}/files/{path}")).body(text)).await;
    }

    let (status, saved) = server.user.json(server.user.put(&format!("/api/v1/assistants/{id}/backup")).json(&json!({"forge_id": forge["id"], "repository": "samexample/standup"}))).await;
    assert_eq!(status, 200, "{saved}");

    common::eventually("the files to reach the repository", || async { (remote_paths(&repository) == [".gitignore", "AGENTS.md", "notes/a.md"]).then_some(()) }).await;
    let (_, files) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}/files"))).await;
    let flags: Vec<(String, bool)> = files.as_array().unwrap().iter().map(|file| (file["path"].as_str().unwrap().to_string(), file["backed_up"] == json!(true))).collect();
    assert!(flags.contains(&("notes/a.md".to_string(), true)) && flags.contains(&("secret/x.md".to_string(), false)), "{flags:?}");

    server.user.json(server.user.delete(&format!("/api/v1/assistants/{id}/files/notes/a.md"))).await;
    common::eventually("the deletion to reach the repository", || async { (remote_paths(&repository) == [".gitignore", "AGENTS.md"]).then_some(()) }).await;
    let (_, assistant) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}"))).await;
    assert!(assistant["backup"]["last_at"].is_string() && assistant["backup"].get("error").is_none(), "{assistant}");
}

#[tokio::test]
async fn connecting_a_repository_that_already_has_files_never_removes_them() {
    let server = Server::start().await;
    enable(&server.user).await;
    let repository = Repository::default();
    repository.lock().unwrap().insert("README.md".to_string(), b"Hello".to_vec());
    let (id, forge) = backed_up_assistant(&server, &repository).await;
    let (status, saved) = server.user.json(server.user.put(&format!("/api/v1/assistants/{id}/backup")).json(&json!({"forge_id": forge["id"], "repository": "samexample/standup"}))).await;
    assert_eq!(status, 200, "{saved}");
    server.user.json(server.user.put(&format!("/api/v1/assistants/{id}/files/notes/a.md")).body("note")).await;

    common::eventually("the backup to finish", || async {
        let (_, assistant) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}"))).await;
        assistant["backup"]["last_at"].is_string().then_some(())
    })
    .await;

    assert!(remote_paths(&repository).contains(&"README.md".to_string()));
}

#[tokio::test]
async fn pull_replaces_storage_with_the_repository_and_keeps_ignored_files() {
    let server = Server::start().await;
    enable(&server.user).await;
    let repository = Repository::default();
    repository.lock().unwrap().extend([("AGENTS.md".to_string(), b"Remote rules".to_vec()), ("README.md".to_string(), b"Hello".to_vec()), (".gitignore".to_string(), b"secret/\n".to_vec())]);
    let (id, forge) = backed_up_assistant(&server, &repository).await;
    server.user.json(server.user.put(&format!("/api/v1/assistants/{id}/files/.gitignore")).body("secret/\n")).await;
    server.user.json(server.user.put(&format!("/api/v1/assistants/{id}/files/secret/x.md")).body("hidden")).await;
    server.user.json(server.user.put(&format!("/api/v1/assistants/{id}/files/old.md")).body("old")).await;
    let (status, _) = server.user.json(server.user.put(&format!("/api/v1/assistants/{id}/backup")).json(&json!({"forge_id": forge["id"], "repository": "samexample/standup"}))).await;
    assert_eq!(status, 200);

    let (status, pulled) = server.user.json(server.user.post(&format!("/api/v1/assistants/{id}/backup/pull"))).await;
    assert_eq!(status, 200, "{pulled}");

    let (_, files) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}/files"))).await;
    let paths: Vec<&str> = files.as_array().unwrap().iter().map(|file| file["path"].as_str().unwrap()).collect();
    let (_, fetched) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}"))).await;
    assert_eq!(paths, vec![".gitignore", "AGENTS.md", "README.md", "secret/x.md"]);
    assert_eq!(fetched["instructions"], json!("Remote rules"));
}

#[tokio::test]
async fn a_schedule_on_an_assistant_prompts_its_current_session() {
    let server = Server::start().await;
    enable(&server.user).await;
    let calls = vec![("mcp__mycelium__propose_schedule", json!({"prompt": "Post the stand-up", "preset": "weekdays at 08:00"}))];
    let (provider, fake) = scripted_provider(&server, calls).await;
    let assistant = create(&server.user, json!({"name": "Stand-up", "preferences": {"models": [{"provider_id": provider["id"], "model": "llama3.2"}]}})).await;
    let id = assistant["id"].as_str().unwrap();
    let chat = prompt(&server.user, id, "Post the stand-up every weekday").await["id"].as_str().unwrap().to_string();

    allow_calls(&server, &chat, 1).await;
    finished_turns(&server, &chat, 1).await;

    let (_, schedules) = server.user.json(server.user.get("/api/v1/schedules")).await;
    let schedule_id = schedules[0]["id"].as_str().unwrap().to_string();
    assert_eq!(schedules[0]["assistant_id"], json!(id));

    let (_, before) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}"))).await;
    assert!(before.get("wakes").is_none());
    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/schedules/{schedule_id}/confirm")).json(&json!({}))).await;
    assert_eq!(status, 200);
    common::eventually("the assistant to show when it wakes", || async {
        let (_, after) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}"))).await;

        (after["wakes"] == json!("Weekdays at 08:00")).then_some(())
    })
    .await;

    let (status, run) = server.user.json(server.user.post(&format!("/api/v1/schedules/{schedule_id}/run"))).await;
    assert_eq!((status, run["conversation_id"].clone()), (201, json!(chat)));
    finished_turns(&server, &chat, 2).await;

    let events = server.events(chat.parse().unwrap()).await;
    let said: Vec<&str> = events.iter().filter(|event| event["kind"] == "user_message").map(|event| event["payload"]["text"].as_str().unwrap()).collect();
    assert_eq!(said, vec!["Post the stand-up every weekday", "Post the stand-up"]);
    assert_eq!(fake.requests.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn oversized_instructions_are_refused_without_creating_the_assistant() {
    let server = Server::start().await;
    enable(&server.user).await;

    let (status, refused) = server.user.json(server.user.post("/api/v1/assistants").json(&json!({"name": "Stand-up", "instructions": "x".repeat(1024 * 1024 + 1)}))).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/assistants")).await;

    assert_eq!((status, refused["error"].clone(), listed), (413, json!("file_too_large"), json!([])));
}

#[tokio::test]
async fn an_archived_assistant_cannot_be_prompted_until_restored() {
    let server = Server::start().await;
    enable(&server.user).await;
    let assistant = create(&server.user, json!({"name": "Stand-up"})).await;
    let id = assistant["id"].as_str().unwrap();
    server.user.json(server.user.patch(&format!("/api/v1/assistants/{id}")).json(&json!({"archived": true}))).await;

    let (status, refused) = server.user.json(server.user.post(&format!("/api/v1/assistants/{id}/prompt")).json(&json!({"text": "Hello"}))).await;

    assert_eq!((status, refused["error"].clone()), (409, json!("assistant_archived")));
    assert!(refused["message"].as_str().unwrap().contains("Restore"));
}
