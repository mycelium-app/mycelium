mod common;

#[path = "common/fake_zulip.rs"]
mod fake_zulip;

#[path = "common/fake_email.rs"]
mod fake_email;

use axum::Json;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use common::{Server, eventually, serve};
use fake_zulip::{ACCOUNT, API_KEY, fake_zulip};
use futures_util::StreamExt;
use futures_util::stream;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const MCP_TOKEN: &str = "mcp-secret-0123456789abcdef";
const ANTHROPIC_KEY: &str = "sk-ant-0123456789abcdefghij";
const WEATHER: &str = "mcp__lab__get_weather";

#[derive(Clone)]
enum Reply {
    Say(&'static str),
    Call(&'static str, Value),
    Forever(&'static str, Value),
}

#[derive(Default)]
struct Model {
    replies: Mutex<VecDeque<Reply>>,
    requests: Mutex<Vec<Value>>,
    counter: AtomicUsize,
    reject_tools: bool,
}

#[derive(Default)]
struct Remote {
    seen: Mutex<Vec<(String, Value, HeaderMap)>>,
}

fn script(replies: Vec<Reply>, reject_tools: bool) -> Arc<Model> {
    Arc::new(Model { replies: Mutex::new(replies.into()), reject_tools, ..Model::default() })
}

impl Model {
    fn next(&self) -> Reply {
        let mut replies = self.replies.lock().unwrap();

        match replies.front().cloned() {
            Some(Reply::Forever(name, input)) => Reply::Call(name, input),
            Some(_) => replies.pop_front().unwrap(),
            None => Reply::Say("Done."),
        }
    }

    fn request(&self, index: usize) -> Value {
        self.requests.lock().unwrap()[index].clone()
    }

    fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

fn event_stream(lines: Vec<String>) -> Response {
    let sent = stream::iter(lines.into_iter().map(|line| Ok::<Bytes, Infallible>(Bytes::from(line))));

    Response::builder().header("content-type", "text/event-stream").body(Body::from_stream(sent.boxed())).unwrap()
}

fn openai_model(model: Arc<Model>) -> Router {
    openai_model_at(model, "/v1")
}

fn openai_model_at(model: Arc<Model>, prefix: &str) -> Router {
    async fn completions(State(model): State<Arc<Model>>, Json(request): Json<Value>) -> Response {
        model.requests.lock().unwrap().push(request.clone());

        if model.reject_tools && request.get("tools").is_some() {
            return (StatusCode::BAD_REQUEST, Json(json!({"error": "registry.ollama.ai/library/gemma:2b does not support tools"}))).into_response();
        }

        let chunk = |delta: Value| format!("data: {}\n\n", json!({"choices": [{"delta": delta}]}));
        let mut lines = Vec::new();

        match model.next() {
            Reply::Say(text) => lines.push(chunk(json!({"content": text}))),
            Reply::Call(name, input) | Reply::Forever(name, input) => {
                let id = format!("call_{}", model.counter.fetch_add(1, Ordering::SeqCst));
                let arguments = input.to_string();
                let (first, second) = arguments.split_at(arguments.len() / 2);

                lines.push(chunk(json!({"tool_calls": [{"index": 0, "id": id, "type": "function", "function": {"name": name, "arguments": first}}]})));
                lines.push(chunk(json!({"tool_calls": [{"index": 0, "function": {"arguments": second}}]})));
            }
        }

        lines.push("data: [DONE]\n\n".to_string());

        event_stream(lines)
    }

    async fn models() -> Json<Value> {
        Json(json!({"data": [{"id": "qwen3:8b"}]}))
    }

    Router::new().route(&format!("{prefix}/models"), get(models)).route(&format!("{prefix}/chat/completions"), post(completions)).with_state(model)
}

fn anthropic_model(model: Arc<Model>) -> Router {
    async fn messages(State(model): State<Arc<Model>>, Json(request): Json<Value>) -> Response {
        model.requests.lock().unwrap().push(request);

        let event = |name: &str, data: Value| format!("event: {name}\ndata: {data}\n\n");
        let mut lines = vec![event("message_start", json!({"type": "message_start", "message": {"usage": {"input_tokens": 3, "cache_read_input_tokens": 30, "cache_creation_input_tokens": 5}}}))];

        match model.next() {
            Reply::Say(text) => lines.push(event("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}}))),
            Reply::Call(name, input) | Reply::Forever(name, input) => {
                let id = format!("toolu_{}", model.counter.fetch_add(1, Ordering::SeqCst));
                let arguments = input.to_string();
                let (first, second) = arguments.split_at(arguments.len() / 2);

                lines.push(event("content_block_start", json!({"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": id, "name": name, "input": {}}})));
                lines.push(event("content_block_delta", json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": first}})));
                lines.push(event("content_block_delta", json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": second}})));
                lines.push(event("content_block_stop", json!({"type": "content_block_stop", "index": 1})));
            }
        }

        lines.push(event("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 2}})));

        event_stream(lines)
    }

    async fn models() -> Json<Value> {
        Json(json!({"data": [{"id": "claude-sonnet-5-5"}], "has_more": false}))
    }

    Router::new().route("/v1/models", get(models)).route("/v1/messages", post(messages)).with_state(model)
}

fn lab_tools() -> Value {
    json!([
        {"name": "get_weather", "description": "Get the weather forecast for a city", "inputSchema": {"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}},
        {"name": "create_issue", "description": "Create an issue in the tracker", "inputSchema": {"type": "object", "properties": {"title": {"type": "string"}}}},
        {"name": "list_files", "description": "List the entries of a folder", "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}}},
    ])
}

fn remote_server(legacy: bool) -> (Router, Arc<Remote>) {
    keyed_server(legacy, "authorization")
}

fn keyed_server(legacy: bool, key_header: &'static str) -> (Router, Arc<Remote>) {
    async fn mcp(State((remote, legacy, key_header)): State<(Arc<Remote>, bool, &'static str)>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
        let method = body["method"].as_str().unwrap_or_default().to_string();
        remote.seen.lock().unwrap().push((method.clone(), body["params"].clone(), headers.clone()));

        let stray_authorization = key_header != "authorization" && headers.contains_key("authorization");

        if stray_authorization || headers.get(key_header).and_then(|value| value.to_str().ok()) != Some(&format!("Bearer {MCP_TOKEN}")) {
            return StatusCode::UNAUTHORIZED.into_response();
        }

        if body.get("id").is_none() {
            return StatusCode::ACCEPTED.into_response();
        }

        let answer = |result: Value| json!({"jsonrpc": "2.0", "id": body["id"], "result": result});
        let result = match method.as_str() {
            "initialize" => json!({"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}, "serverInfo": {"name": "lab", "version": "1"}}),
            "tools/list" => json!({"tools": lab_tools()}),
            "tools/call" => json!({"content": [{"type": "text", "text": format!("Sunny in {}", body["params"]["arguments"]["city"].as_str().unwrap_or("nowhere"))}]}),
            _ => return StatusCode::NOT_FOUND.into_response(),
        };

        if legacy {
            if method == "initialize" {
                return Response::builder().header("content-type", "application/json").header("mcp-session-id", "sess-1").body(Body::from(answer(result).to_string())).unwrap();
            }

            if headers.get("mcp-session-id").and_then(|value| value.to_str().ok()) != Some("sess-1") {
                return StatusCode::BAD_REQUEST.into_response();
            }

            return Response::builder().header("content-type", "text/event-stream").body(Body::from(format!("event: message\ndata: {}\n\n", answer(result)))).unwrap();
        }

        let metadata = &body["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"];

        if headers.get("mcp-method").and_then(|value| value.to_str().ok()) != Some(method.as_str()) || metadata != "2026-07-28" {
            return (StatusCode::BAD_REQUEST, Json(json!({"jsonrpc": "2.0", "id": body["id"], "error": {"code": -32020, "message": "Header mismatch"}}))).into_response();
        }

        Json(answer(result)).into_response()
    }

    let remote = Arc::new(Remote::default());

    (Router::new().route("/mcp", post(mcp)).route("/mcp/", post(mcp)).route("/mcp/{group}", post(mcp)).with_state((remote.clone(), legacy, key_header)), remote)
}

impl Remote {
    fn calls(&self) -> Vec<Value> {
        self.seen.lock().unwrap().iter().filter(|(method, _, _)| method == "tools/call").map(|(_, params, _)| params.clone()).collect()
    }
}

struct Rig {
    server: Server,
    model: Arc<Model>,
    remote: Arc<Remote>,
    provider: Value,
}

async fn rig(replies: Vec<Reply>) -> Rig {
    rig_with(replies, false, false, false).await
}

async fn rig_with(replies: Vec<Reply>, anthropic: bool, legacy: bool, reject_tools: bool) -> Rig {
    rig_over(replies, anthropic, legacy, reject_tools, false).await
}

async fn rig_over(replies: Vec<Reply>, anthropic: bool, legacy: bool, reject_tools: bool, gateway: bool) -> Rig {
    let server = Server::start().await;
    let model = script(replies, reject_tools);
    let base = serve(if anthropic { anthropic_model(model.clone()) } else { openai_model(model.clone()) }).await;
    let body = if anthropic {
        json!({"kind": "anthropic", "name": "Anthropic", "base_url": base, "api_key": ANTHROPIC_KEY})
    } else {
        json!({"kind": "openai_compatible", "name": "Ollama", "base_url": format!("{base}/v1")})
    };
    let (status, provider) = server.user.json(server.user.post("/api/v1/model-providers").json(&body)).await;
    assert_eq!(status, 201, "{provider}");

    let (router, remote) = keyed_server(legacy, if gateway { "x-litellm-api-key" } else { "authorization" });
    let remote_base = serve(router).await;
    let tool_server = if gateway {
        json!({"name": "Lab", "kind": "litellm", "url": remote_base, "bearer_token": MCP_TOKEN})
    } else {
        json!({"name": "Lab", "url": format!("{remote_base}/mcp"), "bearer_token": MCP_TOKEN})
    };
    let (status, added) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&tool_server)).await;
    assert_eq!(status, 201, "{added}");

    Rig { server, model, remote, provider }
}

impl Rig {
    async fn chat(&self, prompt: &str) -> String {
        let (status, chat) = self.server.user.json(self.server.user.post("/api/v1/chats").json(&json!({"provider_id": self.provider["id"], "prompt": prompt}))).await;
        assert_eq!(status, 201, "{chat}");

        chat["id"].as_str().unwrap().to_string()
    }

    async fn events(&self, id: &str) -> Vec<Value> {
        self.server.events(id.parse().unwrap()).await
    }

    async fn turns_ended(&self, id: &str, count: usize) -> Vec<Value> {
        eventually("the turn to end", || async {
            let events = self.events(id).await;

            let ended = events.iter().filter(|event| event["kind"] == "turn_end").count() == count;
            let settled = self.server.conversation(id.parse().unwrap()).await["status"] == "review";

            (ended && settled).then_some(events)
        })
        .await
    }

    async fn request_waiting(&self, id: &str) -> Value {
        eventually("a permission request", || async {
            let events = self.events(id).await;
            let open = events.iter().find(|event| event["kind"] == "permission_request" && !events.iter().any(|other| other["kind"] == "permission_resolved" && other["payload"]["request_id"] == event["payload"]["request_id"]));

            open.map(|event| event["payload"].clone())
        })
        .await
    }

    async fn decide(&self, id: &str, request: &Value, option: &str) -> (u16, Value) {
        self.server.user.json(self.server.user.post(&format!("/api/v1/conversations/{id}/permission")).json(&json!({"request_id": request["request_id"], "option_id": option}))).await
    }

    async fn say(&self, id: &str, text: &str) {
        let (status, _) = self.server.user.json(self.server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": text}))).await;
        assert_eq!(status, 204);
    }
}

fn tool_names(request: &Value) -> Vec<String> {
    request["tools"].as_array().into_iter().flatten().filter_map(|tool| tool["function"]["name"].as_str().or(tool["name"].as_str())).map(String::from).collect()
}

fn updates<'a>(events: &'a [Value], kind: &str) -> Vec<&'a Value> {
    events.iter().filter(|event| event["kind"] == "acp_update" && event["payload"]["update"]["sessionUpdate"] == kind).map(|event| &event["payload"]["update"]).collect()
}

fn roles(request: &Value) -> Vec<String> {
    request["messages"].as_array().unwrap().iter().map(|message| message["role"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn the_model_is_sent_tool_search_and_mycelium_tools_and_a_tool_it_finds_stays_loaded_for_the_chat() {
    let rig = rig(vec![Reply::Call("tool_search", json!({"query": "weather forecast"})), Reply::Say("Found it.")]).await;
    let id = rig.chat("What is the weather in Paris?").await;
    let events = rig.turns_ended(&id, 1).await;
    rig.say(&id, "And tomorrow?").await;
    rig.turns_ended(&id, 2).await;

    let first = tool_names(&rig.model.request(0));
    let second = tool_names(&rig.model.request(1));
    let later = tool_names(&rig.model.request(2));

    assert_eq!(first[0], "tool_search");
    assert!(first.contains(&"mcp__mycelium__recall".to_string()) && !first.contains(&WEATHER.to_string()));
    assert!(second.contains(&WEATHER.to_string()) && !second.contains(&"mcp__lab__create_issue".to_string()));
    assert!(later.contains(&WEATHER.to_string()));
    assert_eq!(rig.model.request(1)["messages"].as_array().unwrap().last().unwrap()["role"], "tool");
    assert!(rig.model.request(0)["messages"][0]["content"].as_str().unwrap().contains("tool_search"));
    assert_eq!(updates(&events, "tool_call_update")[0]["rawOutput"]["loaded"], json!([WEATHER]));
    assert!(events.iter().all(|event| event["kind"] != "permission_request"));
}

#[tokio::test]
async fn the_system_prompt_lists_each_connected_server_and_its_tools_without_sending_their_definitions() {
    let rig = rig(vec![Reply::Say("Hello.")]).await;
    let id = rig.chat("What can you reach?").await;
    rig.turns_ended(&id, 1).await;

    let request = rig.model.request(0);
    let system = request["messages"][0]["content"].as_str().unwrap();
    let sent = tool_names(&request);

    assert!(system.contains("Connected tool servers, loaded with tool_search when needed:"));
    assert!(system.contains("- Lab: get_weather, create_issue, list_files"));
    assert!(!sent.contains(&WEATHER.to_string()) && !sent.contains(&"mcp__lab__create_issue".to_string()));
}

#[tokio::test]
async fn tool_search_ranks_name_matches_above_description_matches_and_says_when_nothing_matches() {
    let rig = rig(vec![
        Reply::Call("tool_search", json!({"query": "files"})),
        Reply::Call("tool_search", json!({"query": "create issue"})),
        Reply::Call("tool_search", json!({"query": "xylophone"})),
        Reply::Say("Done."),
    ])
    .await;
    let id = rig.chat("Look around").await;
    let events = rig.turns_ended(&id, 1).await;
    let answers = updates(&events, "tool_call_update");
    let text = |index: usize| answers[index]["content"][0]["content"]["text"].as_str().unwrap().to_string();

    assert_eq!(answers[0]["rawOutput"]["loaded"][0], "mcp__lab__list_files");
    assert_eq!(answers[1]["rawOutput"]["loaded"][0], "mcp__lab__create_issue");
    assert!(text(1).contains("Input schema:") && text(1).contains("server: Lab"));
    assert!(text(2).starts_with("No tools match"));
    assert_eq!(answers[2]["rawOutput"]["loaded"], json!([]));
}

#[tokio::test]
async fn a_tool_call_waits_for_approval_and_runs_once_allowed() {
    let rig = rig(vec![Reply::Call(WEATHER, json!({"city": "Paris"})), Reply::Say("It is sunny.")]).await;
    let id = rig.chat("Weather in Paris?").await;

    let request = rig.request_waiting(&id).await;
    let status = rig.server.conversation(id.parse().unwrap()).await["status"].clone();
    let before = rig.remote.calls().len();
    let (allowed, _) = rig.decide(&id, &request, "allow_once").await;
    let (again, error) = rig.decide(&id, &request, "allow_once").await;
    let events = rig.turns_ended(&id, 1).await;

    assert_eq!((request["toolCall"]["title"].clone(), request["toolCall"]["server"].clone(), request["toolCall"]["rawInput"].clone()), (json!(WEATHER), json!("Lab"), json!({"city": "Paris"})));
    assert_eq!(request["options"].as_array().unwrap().iter().map(|option| option["name"].as_str().unwrap()).collect::<Vec<_>>(), ["Allow once", "Always allow this tool", "Deny"]);
    assert_eq!((status, before), (json!("input"), 0));
    assert_eq!((allowed, again), (204, 409));
    assert_eq!(error["error"], "already_resolved");
    assert_eq!(rig.remote.calls(), vec![json!({"name": "get_weather", "arguments": {"city": "Paris"}, "_meta": rig.remote.calls()[0]["_meta"].clone()})]);
    assert_eq!(rig.model.request(1)["messages"].as_array().unwrap().last().unwrap()["content"], "Sunny in Paris");
    assert_eq!(updates(&events, "tool_call_update")[0]["status"], "completed");
    assert_eq!(rig.server.conversation(id.parse().unwrap()).await["status"], "review");
}

#[tokio::test]
async fn a_denied_tool_call_never_runs_and_the_model_is_told_the_person_declined() {
    let rig = rig(vec![Reply::Call(WEATHER, json!({"city": "Paris"})), Reply::Say("Understood.")]).await;
    let id = rig.chat("Weather in Paris?").await;

    let request = rig.request_waiting(&id).await;
    rig.decide(&id, &request, "reject_once").await;
    let events = rig.turns_ended(&id, 1).await;

    assert!(rig.remote.calls().is_empty());
    assert_eq!(rig.model.request(1)["messages"].as_array().unwrap().last().unwrap()["content"], "The person declined this tool call.");
    assert_eq!(updates(&events, "tool_call_update")[0]["status"], "failed");
}

#[tokio::test]
async fn always_allow_is_remembered_for_the_chat_and_a_server_can_be_trusted_in_settings() {
    let rig = rig(vec![
        Reply::Call(WEATHER, json!({"city": "Paris"})),
        Reply::Call(WEATHER, json!({"city": "London"})),
        Reply::Say("Both sunny."),
        Reply::Call(WEATHER, json!({"city": "Rome"})),
        Reply::Say("Rome too."),
        Reply::Call(WEATHER, json!({"city": "Oslo"})),
        Reply::Say("Oslo too."),
    ])
    .await;
    let id = rig.chat("Weather?").await;
    let request = rig.request_waiting(&id).await;
    rig.decide(&id, &request, "allow_always").await;
    let events = rig.turns_ended(&id, 1).await;

    assert_eq!(events.iter().filter(|event| event["kind"] == "permission_request").count(), 1);
    assert_eq!(rig.remote.calls().len(), 2);

    rig.say(&id, "Rome?").await;
    let events = rig.turns_ended(&id, 2).await;
    assert_eq!(events.iter().filter(|event| event["kind"] == "permission_request").count(), 1);
    assert_eq!(rig.remote.calls().len(), 3);

    let other = rig.chat("Weather in Oslo?").await;
    let waiting = rig.request_waiting(&other).await;
    rig.decide(&other, &waiting, "reject_once").await;
    rig.turns_ended(&other, 1).await;
    let (_, servers) = rig.server.user.json(rig.server.user.get("/api/v1/chat-tool-servers")).await;
    let (patched, _) = rig.server.user.json(rig.server.user.patch(&format!("/api/v1/chat-tool-servers/{}", servers[0]["id"].as_str().unwrap())).json(&json!({"always_allow": true}))).await;
    rig.model.replies.lock().unwrap().extend([Reply::Call(WEATHER, json!({"city": "Oslo"})), Reply::Say("Oslo too.")]);
    let trusted = rig.chat("Weather in Oslo, again?").await;
    let events = rig.turns_ended(&trusted, 1).await;

    assert_eq!(patched, 200);
    assert!(events.iter().all(|event| event["kind"] != "permission_request"));
    assert_eq!(rig.remote.calls().len(), 4);
}

#[tokio::test]
async fn a_new_message_cancels_the_waiting_call_with_a_refusal_the_model_sees() {
    let rig = rig(vec![Reply::Call(WEATHER, json!({"city": "Paris"})), Reply::Say("Fine, never mind.")]).await;
    let id = rig.chat("Weather in Paris?").await;
    rig.request_waiting(&id).await;

    rig.say(&id, "Never mind").await;
    let events = rig.turns_ended(&id, 1).await;
    let resolved = events.iter().find(|event| event["kind"] == "permission_resolved").unwrap();
    let last = rig.model.request(1);

    assert_eq!(resolved["payload"]["option_id"], "cancelled");
    assert_eq!(roles(&last), ["system", "user", "assistant", "tool", "user"]);
    assert_eq!(last["messages"][3]["content"], "The person did not approve this call, so it did not run.");
    assert_eq!(last["messages"][4]["content"], "Never mind");
    assert!(rig.remote.calls().is_empty());
    assert_eq!(updates(&events, "tool_call_update")[0]["status"], "failed");
}

#[tokio::test]
async fn stopping_a_chat_that_waits_for_approval_ends_the_turn_and_closes_the_card() {
    let rig = rig(vec![Reply::Call(WEATHER, json!({"city": "Paris"}))]).await;
    let id = rig.chat("Weather in Paris?").await;
    rig.request_waiting(&id).await;

    let (stopped, _) = rig.server.user.json(rig.server.user.post(&format!("/api/v1/conversations/{id}/cancel"))).await;
    let events = rig.turns_ended(&id, 1).await;

    assert_eq!(stopped, 204);
    assert_eq!(events.last().unwrap()["payload"]["stop_reason"], "cancelled");
    assert!(events.iter().any(|event| event["kind"] == "permission_resolved" && event["payload"]["option_id"] == "cancelled"));
    assert_eq!(rig.server.conversation(id.parse().unwrap()).await["status"], "review");
}

#[tokio::test]
async fn mycelium_tools_run_in_the_control_plane_and_say_plainly_when_they_need_a_workspace() {
    let rig = rig(vec![
        Reply::Call("mcp__mycelium__remember", json!({"text": "The lab uses Tailscale."})),
        Reply::Call("mcp__mycelium__walkthrough", json!({"steps": []})),
        Reply::Say("Done."),
    ])
    .await;
    let id = rig.chat("Remember the lab network").await;

    for _ in 0..2 {
        let request = rig.request_waiting(&id).await;
        rig.decide(&id, &request, "allow_once").await;
        eventually("the call to finish", || async { rig.events(&id).await.iter().any(|event| event["kind"] == "permission_resolved" && event["payload"]["request_id"] == request["request_id"]).then_some(()) }).await;
    }

    let events = rig.turns_ended(&id, 1).await;
    let answers = updates(&events, "tool_call_update");
    let text = |index: usize| answers[index]["content"][0]["content"]["text"].as_str().unwrap().to_string();

    assert!(text(0).starts_with("Remembered as"), "{}", text(0));
    assert_eq!(answers[0]["status"], "completed");
    assert!(text(1).contains("no workspace"));
    assert_eq!(answers[1]["status"], "failed");
}

#[tokio::test]
async fn a_chat_proposes_a_task_for_a_named_project_and_asks_a_question() {
    let body = "Compare the routers.\n\n".repeat(500);
    let rig = rig(vec![
        Reply::Call("mcp__mycelium__propose_task", json!({"title": "Compare AI routers", "body": body, "project": "Homelab"})),
        Reply::Call("mcp__mycelium__ask_question", json!({"question": "Which router should we try?", "options": ["First", "Second"]})),
        Reply::Say("Recorded."),
    ])
    .await;
    let node = rig.server.online_node("lab").await;
    let owner = sqlx::query_scalar::<_, uuid::Uuid>("select id from users limit 1").fetch_one(&rig.server.pool).await.unwrap();
    let project = sqlx::query_scalar::<_, uuid::Uuid>("insert into projects (owner, node_id, name, path) values ($1, $2, 'homelab', '/work/homelab') returning id").bind(owner).bind(node.id).fetch_one(&rig.server.pool).await.unwrap();
    let id = rig.chat("Plan the router comparison").await;

    for count in 1..=2 {
        let request = rig.request_waiting(&id).await;
        rig.decide(&id, &request, "allow_once").await;
        let answer = eventually("the tool result", || async {
            let events = rig.events(&id).await;
            let answers = updates(&events, "tool_call_update");

            answers.get(count - 1).map(|answer| (*answer).clone())
        })
        .await;
        assert_eq!(answer["status"], "completed", "{answer}");
    }

    let events = rig.turns_ended(&id, 1).await;
    let answers = updates(&events, "tool_call_update");
    assert_eq!(answers.len(), 2, "{events:?}");
    assert!(answers.iter().all(|answer| answer["status"] == "completed"), "{answers:?}");

    let (status, tasks) = rig.server.user.json(rig.server.user.get("/api/v1/tasks")).await;
    assert_eq!(status, 200, "{tasks}");
    let proposed = tasks.as_array().unwrap().iter().find(|task| task["status"] == "proposed").unwrap();
    assert_eq!(proposed["project_id"], json!(project));
    assert_eq!(proposed["node_id"], json!(node.id));
    assert_eq!(proposed["cwd"], "/work/homelab");
    assert_eq!(proposed["agent"], "claude");
    assert_eq!(proposed["source_id"], id);
    assert_eq!(proposed["prompt"], body.trim());
    let question = tasks.as_array().unwrap().iter().find(|task| task["status"] == "question").unwrap();
    assert_eq!(question["options"], json!(["First", "Second"]));
    assert_eq!(question["source_id"], id);
    assert_eq!(question["agent"], "claude");

    sqlx::query("update tasks set agent = 'chat' where source_id = $1").bind(id.parse::<uuid::Uuid>().unwrap()).execute(&rig.server.pool).await.unwrap();
    sqlx::raw_sql(include_str!("../migrations/0063_chat_task_agents.sql")).execute(&rig.server.pool).await.unwrap();
    let (status, repaired) = rig.server.user.json(rig.server.user.get("/api/v1/tasks")).await;
    assert_eq!(status, 200, "{repaired}");
    assert_eq!(repaired, tasks);
}

#[tokio::test]
async fn a_chat_task_uses_the_named_projects_existing_workspace_harness() {
    let rig = rig(vec![Reply::Call("mcp__mycelium__propose_task", json!({"title": "Compare routers", "project": "homelab"})), Reply::Say("Recorded.")]).await;
    let node = rig.server.online_node("lab").await;
    let owner = sqlx::query_scalar::<_, uuid::Uuid>("select id from users limit 1").fetch_one(&rig.server.pool).await.unwrap();
    let project = sqlx::query_scalar::<_, uuid::Uuid>("insert into projects (owner, node_id, name, path) values ($1, $2, 'homelab', '/work/homelab') returning id").bind(owner).bind(node.id).fetch_one(&rig.server.pool).await.unwrap();
    sqlx::query("insert into conversations (owner, node_id, agent, cwd, project_id, title, status) values ($1, $2, 'codex', '/work/homelab', $3, 'Workspace', 'done')").bind(owner).bind(node.id).bind(project).execute(&rig.server.pool).await.unwrap();
    let id = rig.chat("Plan a comparison").await;
    let request = rig.request_waiting(&id).await;
    rig.decide(&id, &request, "allow_once").await;
    rig.turns_ended(&id, 1).await;

    let (status, tasks) = rig.server.user.json(rig.server.user.get("/api/v1/tasks")).await;
    assert_eq!(status, 200, "{tasks}");
    assert_eq!(tasks[0]["agent"], "codex");
}

#[tokio::test]
async fn a_failed_mycelium_tool_returns_the_error_to_the_model_and_keeps_it_in_the_chat_history() {
    let rig = rig(vec![
        Reply::Call("mcp__mycelium__propose_task", json!({"title": "Compare routers"})),
        Reply::Say("The server could not record the proposal."),
        Reply::Say("The tool failed earlier."),
    ])
    .await;
    let id = rig.chat("Plan a comparison").await;
    let request = rig.request_waiting(&id).await;
    sqlx::query("alter table tasks rename to unavailable_tasks").execute(&rig.server.pool).await.unwrap();
    rig.decide(&id, &request, "allow_once").await;
    rig.turns_ended(&id, 1).await;

    let returned = rig.model.request(1);
    let result = returned["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap();
    assert!(result.contains("failed"), "{result}");
    assert!(result.contains("did not decline"), "{result}");
    assert!(result.contains("Something went wrong on the server"), "{result}");

    rig.say(&id, "What happened?").await;
    rig.turns_ended(&id, 2).await;
    let replayed = rig.model.request(2);
    assert!(replayed["messages"].as_array().unwrap().iter().any(|message| message["role"] == "tool" && message["content"] == result), "{replayed}");
}

#[tokio::test]
async fn an_anthropic_chat_sends_tool_definitions_and_returns_results_as_tool_result_blocks() {
    let rig = rig_with(vec![Reply::Call(WEATHER, json!({"city": "Paris"})), Reply::Say("Sunny.")], true, false, false).await;
    let id = rig.chat("Weather in Paris?").await;
    let request = rig.request_waiting(&id).await;
    rig.decide(&id, &request, "allow_once").await;
    rig.turns_ended(&id, 1).await;

    let first = rig.model.request(0);
    let second = rig.model.request(1);
    let schema = first["tools"].as_array().unwrap().iter().find(|tool| tool["name"] == "tool_search").unwrap()["input_schema"].clone();
    let used = &second["messages"][1]["content"][0];
    let returned = &second["messages"][2]["content"][0];

    assert_eq!(schema["required"], json!(["query"]));
    assert_eq!((used["type"].clone(), used["name"].clone(), used["input"].clone()), (json!("tool_use"), json!(WEATHER), json!({"city": "Paris"})));
    assert_eq!((returned["type"].clone(), returned["tool_use_id"].clone(), returned["content"].clone()), (json!("tool_result"), used["id"].clone(), json!("Sunny in Paris")));
    assert_eq!(second["messages"][2]["role"], "user");

    let (_, activity) = rig.server.user.json(rig.server.user.get("/api/v1/activity").query(&[("range", "today")])).await;

    assert_eq!(activity["totals"]["input_tokens"], 6);
    assert_eq!(activity["totals"]["output_tokens"], 4);
    assert_eq!(activity["totals"]["cache_read_tokens"], 60);
    assert_eq!(activity["totals"]["cache_write_tokens"], 10);

    let (_, usage) = rig.server.user.json(rig.server.user.get(&format!("/api/v1/conversations/{id}/usage"))).await;

    assert_eq!(usage["context_used"], 38);
    assert_eq!(usage["context_size"], Value::Null);
    assert_eq!(usage["input_tokens"], 6);
    assert_eq!(usage["cache_read_tokens"], 60);
    assert_eq!(usage["cache_write_tokens"], 10);
}

#[tokio::test]
async fn the_loop_stops_after_twenty_rounds_of_tool_calls_and_says_so() {
    let rig = rig(vec![Reply::Forever("tool_search", json!({"query": "weather"}))]).await;
    let id = rig.chat("Keep searching").await;
    let events = rig.turns_ended(&id, 1).await;
    let notes = updates(&events, "note");

    assert_eq!(rig.model.request_count(), 20);
    assert_eq!(notes.len(), 1);
    assert!(notes[0]["content"]["text"].as_str().unwrap().contains("20 rounds"));
}

#[tokio::test]
async fn a_model_that_rejects_tools_answers_without_them_and_the_thread_says_so_quietly() {
    let rig = rig_with(vec![Reply::Say("Hello without tools.")], false, false, true).await;
    let id = rig.chat("Hello").await;
    let events = rig.turns_ended(&id, 1).await;
    let notes = updates(&events, "note");

    assert_eq!(rig.model.request_count(), 2);
    assert!(rig.model.request(0).get("tools").is_some() && rig.model.request(1).get("tools").is_none());
    assert!(!rig.model.request(1)["messages"][0]["content"].as_str().unwrap().contains("tool_search"));
    assert_eq!(notes[0]["content"]["text"], "This model does not take tools, so it answered without them.");
    assert!(updates(&events, "agent_message_chunk")[0]["content"]["text"].as_str().unwrap().contains("Hello without tools."));
    assert!(events.iter().all(|event| event["kind"] != "error"));
}

#[tokio::test]
async fn history_replays_tool_calls_and_results_to_the_model_in_order() {
    let rig = rig(vec![Reply::Call("tool_search", json!({"query": "weather"})), Reply::Call(WEATHER, json!({"city": "Paris"})), Reply::Say("Sunny in Paris."), Reply::Say("You are welcome.")]).await;
    let id = rig.chat("Weather in Paris?").await;
    let request = rig.request_waiting(&id).await;
    rig.decide(&id, &request, "allow_once").await;
    rig.turns_ended(&id, 1).await;

    rig.say(&id, "Thanks").await;
    rig.turns_ended(&id, 2).await;
    let last = rig.model.request(rig.model.request_count() - 1);
    let messages = last["messages"].as_array().unwrap();

    assert_eq!(roles(&last), ["system", "user", "assistant", "tool", "assistant", "tool", "assistant", "user"]);
    assert_eq!(messages[2]["tool_calls"][0]["function"]["name"], "tool_search");
    assert_eq!(messages[2]["tool_calls"][0]["id"], messages[3]["tool_call_id"]);
    assert_eq!(messages[4]["tool_calls"][0]["function"]["arguments"], json!({"city": "Paris"}).to_string());
    assert_eq!(messages[5]["content"], "Sunny in Paris");
    assert_eq!((messages[6]["content"].clone(), messages[7]["content"].clone()), (json!("Sunny in Paris."), json!("Thanks")));
}

#[tokio::test]
async fn a_legacy_mcp_server_with_sessions_and_event_streams_is_listed_and_called() {
    let rig = rig_with(vec![Reply::Call(WEATHER, json!({"city": "Rome"})), Reply::Say("Sunny.")], false, true, false).await;
    let (_, servers) = rig.server.user.json(rig.server.user.get("/api/v1/chat-tool-servers")).await;
    let id = rig.chat("Weather in Rome?").await;
    let request = rig.request_waiting(&id).await;
    rig.decide(&id, &request, "allow_once").await;
    rig.turns_ended(&id, 1).await;

    assert_eq!(servers[0]["tools"].as_array().unwrap().len(), 3);
    assert_eq!(rig.model.request(1)["messages"].as_array().unwrap().last().unwrap()["content"], "Sunny in Rome");
    assert!(rig.remote.seen.lock().unwrap().iter().any(|(method, _, headers)| method == "tools/call" && headers.get("mcp-session-id").is_some()));
}

#[tokio::test]
async fn tool_servers_are_added_checked_switched_and_removed_without_returning_the_token() {
    let server = Server::start().await;
    let (router, remote) = remote_server(false);
    let base = serve(router).await;
    let url = format!("{base}/mcp");
    let post = |body: Value| server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&body));

    let (status, added) = post(json!({"name": "Lab", "url": url, "bearer_token": MCP_TOKEN})).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;
    let stored = server.user.get("/api/v1/chat-tool-servers").send().await.unwrap().text().await.unwrap();
    let (_, wrong) = post(json!({"name": "Other", "url": url, "bearer_token": "wrong"})).await;
    let (_, unreachable) = post(json!({"name": "Gone", "url": "http://127.0.0.1:9/mcp"})).await;
    let (_, malformed) = post(json!({"name": "Bad", "url": "example.com"})).await;
    let (_, nameless) = post(json!({"name": " ", "url": url})).await;
    let (_, reserved) = post(json!({"name": "Mycelium", "url": url, "bearer_token": MCP_TOKEN})).await;
    let (duplicate, _) = post(json!({"name": "lab", "url": url, "bearer_token": MCP_TOKEN})).await;
    let id = added["id"].as_str().unwrap();
    let (_, switched) = server.user.json(server.user.patch(&format!("/api/v1/chat-tool-servers/{id}")).json(&json!({"enabled": false}))).await;
    let (refreshed, _) = server.user.json(server.user.post(&format!("/api/v1/chat-tool-servers/{id}/refresh"))).await;
    let other = server.second_user("someone@example.com").await;
    let (hidden, _) = other.json(other.delete(&format!("/api/v1/chat-tool-servers/{id}"))).await;
    let (removed, _) = server.user.json(server.user.delete(&format!("/api/v1/chat-tool-servers/{id}"))).await;

    assert_eq!(status, 201);
    assert_eq!(added["token_hint"], "••••cdef");
    assert_eq!(added["tools"][0], json!({"name": "get_weather", "description": "Get the weather forecast for a city"}));
    assert_eq!((added["enabled"].clone(), added["always_allow"].clone()), (json!(true), json!(false)));
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert!(!stored.contains(MCP_TOKEN));
    assert_eq!((wrong["error"].clone(), unreachable["error"].clone(), malformed["error"].clone()), (json!("tool_server_failed"), json!("tool_server_failed"), json!("url_invalid")));
    assert_eq!((nameless["error"].clone(), reserved["error"].clone(), duplicate), (json!("name_required"), json!("name_taken"), 409));
    assert_eq!(switched["enabled"], false);
    assert_eq!((refreshed, hidden, removed), (200, 404, 204));
    assert!(remote.seen.lock().unwrap().iter().all(|(_, _, headers)| headers.get("mcp-protocol-version").is_some()));
}

async fn add_outline(server: &Server, url: &str, key: Option<&str>) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Docs", "kind": "outline", "url": url, "bearer_token": key}))).await
}

#[tokio::test]
async fn an_outline_address_resolves_to_its_mcp_endpoint_and_connects_with_an_api_key() {
    let server = Server::start().await;
    let (router, _) = remote_server(false);
    let base = serve(router).await;

    let (status, added) = add_outline(&server, &base, Some(MCP_TOKEN)).await;

    assert_eq!(status, 201, "{added}");
    assert_eq!((added["kind"].as_str(), added["url"].as_str().map(str::to_string)), (Some("mcp"), Some(format!("{base}/mcp"))));
    assert_eq!(added["settings"], json!({"app": "outline"}));
    assert_eq!(added["tools"][0]["name"], "get_weather");
}

#[tokio::test]
async fn an_outline_address_that_already_ends_in_mcp_is_used_as_given() {
    let server = Server::start().await;
    let (router, _) = remote_server(false);
    let base = serve(router).await;

    let (_, added) = add_outline(&server, &format!("{base}/mcp"), Some(MCP_TOKEN)).await;

    assert_eq!(added["url"], json!(format!("{base}/mcp")));
}

#[tokio::test]
async fn outline_refusing_the_api_key_says_to_check_the_key_and_stores_nothing() {
    let server = Server::start().await;
    let (router, _) = remote_server(false);
    let base = serve(router).await;

    let (status, refused) = add_outline(&server, &base, Some("wrong")).await;
    let (_, missing) = add_outline(&server, &base, None).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;

    assert_eq!((status, refused["message"].as_str()), (400, Some("Outline refused that API key. Check it was copied whole and has not been revoked.")));
    assert_eq!((missing["error"].as_str(), missing["message"].as_str()), (Some("api_key_required"), Some("Enter your Outline API key. Create one in Outline under Settings, API & Apps.")));
    assert_eq!(listed, json!([]));
}

async fn add_gateway(server: &Server, url: &str, key: Option<&str>) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Gateway", "kind": "litellm", "url": url, "bearer_token": key}))).await
}

#[tokio::test]
async fn a_litellm_gateway_is_reached_at_its_mcp_address_with_the_key_in_its_own_header() {
    let server = Server::start().await;
    let (router, remote) = keyed_server(false, "x-litellm-api-key");
    let base = serve(router).await;

    let (status, added) = add_gateway(&server, &base, Some(MCP_TOKEN)).await;

    assert_eq!(status, 201, "{added}");
    assert_eq!((added["kind"].as_str(), added["url"].as_str().map(str::to_string)), (Some("litellm"), Some(format!("{base}/mcp/"))));
    assert_eq!(added["tools"][0]["name"], "get_weather");
    assert!(remote.seen.lock().unwrap().iter().all(|(_, _, headers)| headers.contains_key("x-litellm-api-key") && !headers.contains_key("authorization")));
}

#[tokio::test]
async fn a_litellm_address_with_a_path_is_kept_as_typed() {
    let server = Server::start().await;
    let (router, _) = keyed_server(false, "x-litellm-api-key");
    let base = serve(router).await;

    let (_, added) = add_gateway(&server, &format!("{base}/mcp/dev_group"), Some(MCP_TOKEN)).await;

    assert_eq!(added["url"], json!(format!("{base}/mcp/dev_group")));
}

#[tokio::test]
async fn a_litellm_gateway_without_a_key_asks_for_one() {
    let server = Server::start().await;
    let (router, _) = keyed_server(false, "x-litellm-api-key");
    let base = serve(router).await;

    let (status, missing) = add_gateway(&server, &base, None).await;

    assert_eq!((status, missing["error"].as_str(), missing["message"].as_str()), (400, Some("api_key_required"), Some("Enter your LiteLLM key.")));
}

#[tokio::test]
async fn refreshing_a_litellm_gateway_lists_its_tools_again() {
    let server = Server::start().await;
    let (router, remote) = keyed_server(false, "x-litellm-api-key");
    let base = serve(router).await;
    let (_, added) = add_gateway(&server, &base, Some(MCP_TOKEN)).await;
    let id = added["id"].as_str().unwrap();
    let listings = || remote.seen.lock().unwrap().iter().filter(|(method, _, _)| method == "tools/list").count();
    let before = listings();

    let (status, refreshed) = server.user.json(server.user.post(&format!("/api/v1/chat-tool-servers/{id}/refresh"))).await;

    assert_eq!((status, refreshed["tools"].as_array().map(Vec::len)), (200, Some(3)));
    assert!(listings() > before);
}

#[tokio::test]
async fn a_chat_calls_a_litellm_tool_with_the_key_in_its_own_header() {
    let rig = rig_over(vec![Reply::Call(WEATHER, json!({"city": "Paris"})), Reply::Say("It is sunny.")], false, false, false, true).await;
    let id = rig.chat("Weather in Paris?").await;

    let request = rig.request_waiting(&id).await;
    let (allowed, _) = rig.decide(&id, &request, "allow_once").await;
    rig.turns_ended(&id, 1).await;

    assert_eq!(allowed, 204);
    assert_eq!(rig.remote.calls().len(), 1);
    assert_eq!(rig.model.request(1)["messages"].as_array().unwrap().last().unwrap()["content"], "Sunny in Paris");
}

#[tokio::test]
async fn a_litellm_proxy_lists_its_models_and_streams_a_reply_from_its_root_address() {
    let server = Server::start().await;
    let model = script(vec![Reply::Say("Hello from the proxy.")], false);
    let base = serve(openai_model_at(model.clone(), "")).await;
    let (status, provider) = server.user.json(server.user.post("/api/v1/model-providers").json(&json!({"kind": "openai_compatible", "name": "LiteLLM", "base_url": base, "api_key": "sk-lite"}))).await;
    assert_eq!(status, 201, "{provider}");
    let (router, remote) = remote_server(false);
    drop(router);
    let rig = Rig { server, model, remote, provider };

    let id = rig.chat("Hello?").await;
    let events = rig.turns_ended(&id, 1).await;

    assert_eq!(rig.provider["models"], json!(["qwen3:8b"]));
    assert!(events.iter().any(|event| event["kind"] == "agent_message_chunk" || event.to_string().contains("Hello from the proxy.")));
}

#[tokio::test]
async fn a_tool_server_is_renamed_with_the_same_rules_as_adding_one() {
    let server = Server::start().await;
    let (router, _) = remote_server(false);
    let base = serve(router).await;
    let url = format!("{base}/mcp");
    let post = |name: &str| server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": name, "url": url, "bearer_token": MCP_TOKEN})));
    let (_, lab) = post("Lab").await;
    let (_, other) = post("Other").await;
    let id = lab["id"].as_str().unwrap();
    let rename = |id: &str, name: &str| server.user.json(server.user.patch(&format!("/api/v1/chat-tool-servers/{id}")).json(&json!({"name": name})));

    let (blank, blank_body) = rename(id, "  ").await;
    let (reserved, reserved_body) = rename(id, "Mycelium").await;
    let (taken, taken_body) = rename(id, " other ").await;
    let (same, _) = rename(id, "LAB").await;
    let (renamed, body) = rename(id, "  Weather  ").await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;
    let someone = server.second_user("someone@example.com").await;
    let (hidden, _) = someone.json(someone.patch(&format!("/api/v1/chat-tool-servers/{id}")).json(&json!({"name": "Mine"}))).await;

    assert_eq!((blank, blank_body["error"].clone()), (400, json!("name_required")));
    assert_eq!((reserved, reserved_body["error"].clone()), (400, json!("name_taken")));
    assert_eq!((taken, taken_body["error"].clone()), (409, json!("name_taken")));
    assert_eq!(same, 200);
    assert_eq!((renamed, body["name"].clone()), (200, json!("Weather")));
    assert_eq!(listed[0]["name"], "Weather");
    assert_eq!(listed[1]["name"], other["name"]);
    assert_eq!(hidden, 404);
}

#[tokio::test]
async fn a_switched_off_server_is_not_searched_and_the_token_never_reaches_the_thread() {
    let rig = rig(vec![Reply::Call("tool_search", json!({"query": "weather"})), Reply::Say("Nothing.")]).await;
    let (_, servers) = rig.server.user.json(rig.server.user.get("/api/v1/chat-tool-servers")).await;
    rig.server.user.json(rig.server.user.patch(&format!("/api/v1/chat-tool-servers/{}", servers[0]["id"].as_str().unwrap())).json(&json!({"enabled": false}))).await;
    let id = rig.chat("Weather?").await;
    let events = rig.turns_ended(&id, 1).await;

    assert_eq!(updates(&events, "tool_call_update")[0]["rawOutput"]["loaded"], json!([]));
    assert!(!serde_json::to_string(&events).unwrap().contains(MCP_TOKEN));
}

#[tokio::test]
async fn a_very_long_tool_result_is_cut_before_it_is_stored_or_sent_to_the_model() {
    let rig = rig(vec![Reply::Call(WEATHER, json!({"city": "x".repeat(60_000)})), Reply::Say("Long.")]).await;
    let id = rig.chat("Weather?").await;
    let request = rig.request_waiting(&id).await;
    rig.decide(&id, &request, "allow_once").await;
    rig.turns_ended(&id, 1).await;
    let sent = rig.model.request(1)["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().to_string();

    assert!(sent.chars().count() < 51_000);
    assert!(sent.ends_with("[Cut: the result was longer than 50000 characters.]"));
}

#[tokio::test]
async fn sending_a_zulip_message_asks_every_time_even_when_the_integration_is_always_allowed() {
    let rig = rig(vec![
        Reply::Call("mcp__zulip__list_channels", json!({})),
        Reply::Call("mcp__zulip__send_message", json!({"channel": "general", "topic": "lunch", "content": "Noon works."})),
        Reply::Say("Sent."),
        Reply::Call("mcp__zulip__send_message", json!({"channel": "general", "topic": "lunch", "content": "Again."})),
        Reply::Say("Sent again."),
    ])
    .await;
    let (router, zulip) = fake_zulip();
    let base = serve(router).await;
    let (status, added) = rig.server.user.json(rig.server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Zulip", "kind": "zulip", "url": base, "account": ACCOUNT, "bearer_token": API_KEY}))).await;
    let (patched, _) = rig.server.user.json(rig.server.user.patch(&format!("/api/v1/chat-tool-servers/{}", added["id"].as_str().unwrap())).json(&json!({"always_allow": true}))).await;
    let id = rig.chat("Tell the team noon works.").await;

    let request = rig.request_waiting(&id).await;
    assert_eq!(request["toolCall"]["title"], "mcp__zulip__send_message");
    assert!(zulip.sent().is_empty());
    rig.decide(&id, &request, "allow_always").await;
    let events = rig.turns_ended(&id, 1).await;

    assert_eq!((status, patched), (201, 200));
    assert_eq!(events.iter().filter(|event| event["kind"] == "permission_request").count(), 1);
    assert_eq!(zulip.sent().len(), 1);
    assert!(zulip.paths().contains(&"/api/v1/users/me/subscriptions".to_string()));

    rig.say(&id, "Once more.").await;
    let request = rig.request_waiting(&id).await;
    rig.decide(&id, &request, "reject_once").await;
    rig.turns_ended(&id, 2).await;

    assert_eq!(zulip.sent().len(), 1);
}

#[tokio::test]
async fn email_send_delete_and_move_always_ask_even_for_a_trusted_integration_and_a_remembered_approval() {
    for (name, arguments) in [
        ("mcp__mail__send_message", json!({"to": ["grace@example.com"], "subject": "Lunch", "body": "Noon works."})),
        ("mcp__mail__delete_message", json!({"id": "5"})),
        ("mcp__mail__move_message", json!({"id": "5", "destination_folder": "Sent"})),
    ] {
        let replies = vec![Reply::Call(name, arguments.clone()), Reply::Say("Done."), Reply::Call(name, arguments), Reply::Say("Stopped.")];
        let rig = rig(replies).await;
        let (imap, smtp, mail) = fake_email::fake_mail().await;
        let (_, account) = rig.server.user.json(rig.server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Mail", "kind": "email", "url": "", "account": fake_email::USERNAME, "bearer_token": fake_email::PASSWORD, "settings": {"imap_host": "127.0.0.1", "imap_port": imap, "smtp_host": "127.0.0.1", "smtp_port": smtp}}))).await;
        rig.server.user.json(rig.server.user.patch(&format!("/api/v1/chat-tool-servers/{}", account["id"].as_str().unwrap())).json(&json!({"always_allow": true}))).await;
        let id = rig.chat("Do the email action.").await;
        let request = rig.request_waiting(&id).await;

        assert_eq!(request["toolCall"]["title"], name);
        assert!(mail.sent.lock().unwrap().is_empty());
        assert!(mail.removed.lock().unwrap().is_empty());
        rig.decide(&id, &request, "allow_always").await;
        rig.turns_ended(&id, 1).await;
        let sent = mail.sent.lock().unwrap().len();
        let removed = mail.removed.lock().unwrap().len();
        assert_eq!(sent + removed, 1);

        rig.say(&id, "Do it again.").await;
        let request = rig.request_waiting(&id).await;
        rig.decide(&id, &request, "reject_once").await;
        rig.turns_ended(&id, 2).await;

        assert_eq!(mail.sent.lock().unwrap().len(), sent);
        assert_eq!(mail.removed.lock().unwrap().len(), removed);
    }
}
