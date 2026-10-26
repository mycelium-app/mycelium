mod common;

use axum::Json;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::response::Response;
use axum::routing::{get, post};
use common::{Client, Server, serve};
use futures_util::stream;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

type Requests = Arc<Mutex<Vec<Value>>>;

fn fake_openai() -> (Router, Requests) {
    async fn models() -> Json<Value> {
        Json(json!({"object": "list", "data": [{"id": "llama3.2"}]}))
    }

    async fn completions(State(requests): State<Requests>, Json(request): Json<Value>) -> Response {
        requests.lock().unwrap().push(request);

        let chunk = |content: &str| format!("data: {}\n\n", json!({"choices": [{"delta": {"content": content}}]}));
        let lines = vec![chunk("Hello"), chunk(" there"), "data: [DONE]\n\n".to_string()];
        let body = stream::iter(lines.into_iter().map(|line| Ok::<Bytes, Infallible>(Bytes::from(line))));

        Response::builder().header("content-type", "text/event-stream").body(Body::from_stream(body)).unwrap()
    }

    let requests = Requests::default();
    let router = Router::new().route("/v1/models", get(models)).route("/v1/chat/completions", post(completions)).with_state(requests.clone());

    (router, requests)
}

async fn chat_provider(server: &Server) -> (Value, Requests) {
    let (router, requests) = fake_openai();
    let base = serve(router).await;
    let (status, provider) = server.user.json(server.user.post("/api/v1/model-providers").json(&json!({"kind": "openai_compatible", "name": "Ollama", "base_url": format!("{base}/v1")}))).await;
    assert_eq!(status, 201, "{provider}");

    (provider, requests)
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
    let (provider, requests) = chat_provider(&server).await;
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
    assert_eq!(requests.lock().unwrap().len(), 2);
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
    let (provider, requests) = chat_provider(&server).await;
    let (_, group) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": "acme"}))).await;
    server.user.json(server.user.put("/api/v1/instructions/global").json(&json!({"text": "Global rule."}))).await;
    let (status, _) = server.user.json(server.user.put(&format!("/api/v1/groups/{}/instructions", group["id"].as_str().unwrap())).json(&json!({"text": "Group rule."}))).await;
    assert_eq!(status, 200);
    let assistant = create(&server.user, json!({"name": "Stand-up", "group_id": group["id"], "instructions": "Assistant rule.", "preferences": {"models": [{"provider_id": provider["id"], "model": "llama3.2"}]}})).await;

    let conversation = prompt(&server.user, assistant["id"].as_str().unwrap(), "Hello").await;
    finished_turns(&server, conversation["id"].as_str().unwrap(), 1).await;

    let system = requests.lock().unwrap()[0]["messages"][0]["content"].as_str().unwrap().to_string();
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
