use axum::Json;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use crate::common::{Server, eventually, serve};
use futures_util::StreamExt;
use futures_util::stream;
use mycelium_proto::conversation::{EventKind, ConversationStatus};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};

const OPENAI_KEY: &str = "sk-local-0123456789abcdef";
const ANTHROPIC_KEY: &str = "sk-ant-0123456789abcdefghij";

#[derive(Default)]
struct Fake {
    requests: Mutex<Vec<Value>>,
    headers: Mutex<Vec<HeaderMap>>,
}

fn last_text(request: &Value) -> String {
    request["messages"].as_array().and_then(|messages| messages.last()).and_then(|message| message["content"].as_str()).unwrap_or_default().to_string()
}

fn event_stream(lines: Vec<String>, hang: bool) -> Response {
    let sent = stream::iter(lines.into_iter().map(|line| Ok::<Bytes, Infallible>(Bytes::from(line))));
    let body: stream::BoxStream<'static, Result<Bytes, Infallible>> = if hang { sent.chain(stream::pending()).boxed() } else { sent.boxed() };

    Response::builder().header("content-type", "text/event-stream").body(Body::from_stream(body)).unwrap()
}

fn fake_openai(key: Option<&'static str>) -> (Router, Arc<Fake>) {
    async fn models(State((fake, key)): State<(Arc<Fake>, Option<&'static str>)>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
        fake.headers.lock().unwrap().push(headers.clone());
        let allowed = key.is_none_or(|key| headers.get("authorization").and_then(|value| value.to_str().ok()) == Some(&format!("Bearer {key}")));

        allowed.then(|| Json(json!({"object": "list", "data": [{"id": "qwen3:8b"}, {"id": "llama3.2"}]}))).ok_or(StatusCode::UNAUTHORIZED)
    }

    async fn completions(State((fake, _)): State<(Arc<Fake>, Option<&'static str>)>, Json(request): Json<Value>) -> Response {
        let text = last_text(&request);
        fake.requests.lock().unwrap().push(request);

        if text.contains("break") {
            return Response::builder().status(500).body(Body::empty()).unwrap();
        }

        let chunk = |content: &str| format!("data: {}\n\n", json!({"choices": [{"delta": {"content": content}}]}));
        let mut lines = vec![chunk("Hello"), chunk(" there")];

        if text.contains("wait") {
            return event_stream(lines.drain(..1).collect(), true);
        }

        lines.push(format!("data: {}\n\n", json!({"choices": [], "usage": {"prompt_tokens": 21, "completion_tokens": 4, "prompt_tokens_details": {"cached_tokens": 12, "cache_write_tokens": 4}}})));
        lines.push("data: [DONE]\n\n".to_string());

        event_stream(lines, false)
    }

    let fake = Arc::new(Fake::default());
    let router = Router::new().route("/v1/models", get(models)).route("/v1/chat/completions", post(completions)).with_state((fake.clone(), key));

    (router, fake)
}

fn fake_anthropic() -> (Router, Arc<Fake>) {
    async fn models(State(fake): State<Arc<Fake>>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
        fake.headers.lock().unwrap().push(headers.clone());

        (headers.get("x-api-key").and_then(|value| value.to_str().ok()) == Some(ANTHROPIC_KEY))
            .then(|| Json(json!({"data": [{"id": "claude-haiku-5-5"}, {"id": "claude-opus-5-5"}, {"id": "claude-sonnet-5-5"}], "has_more": false})))
            .ok_or(StatusCode::UNAUTHORIZED)
    }

    async fn messages(State(fake): State<Arc<Fake>>, Json(request): Json<Value>) -> Response {
        fake.requests.lock().unwrap().push(request.clone());

        let event = |name: &str, data: Value| format!("event: {name}\ndata: {data}\n\n");
        let mut lines = vec![
            event("message_start", json!({"type": "message_start", "message": {"usage": {"input_tokens": 12, "output_tokens": 1, "cache_read_input_tokens": 300, "cache_creation_input_tokens": 40}}})),
            event("content_block_start", json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})),
            event("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Bonjour"}})),
            event("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": " tout le monde"}})),
        ];

        if request["messages"].to_string().contains("overloaded") {
            lines.push(event("error", json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}})));

            return event_stream(lines, false);
        }

        lines.push(event("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 5}})));
        lines.push(event("message_stop", json!({"type": "message_stop"})));

        event_stream(lines, false)
    }

    let fake = Arc::new(Fake::default());
    let router = Router::new().route("/v1/models", get(models)).route("/v1/messages", post(messages)).with_state(fake.clone());

    (router, fake)
}

async fn add_openai(server: &Server, base: &str, key: Option<&str>) -> Value {
    let (status, provider) = server.user.json(server.user.post("/api/v1/model-providers").json(&json!({"kind": "openai_compatible", "name": "Ollama", "base_url": format!("{base}/v1"), "api_key": key}))).await;
    assert_eq!(status, 201, "{provider}");

    provider
}

async fn add_anthropic(server: &Server, base: &str) -> Value {
    let (status, provider) = server.user.json(server.user.post("/api/v1/model-providers").json(&json!({"kind": "anthropic", "name": "Anthropic", "base_url": base, "api_key": ANTHROPIC_KEY}))).await;
    assert_eq!(status, 201, "{provider}");

    provider
}

async fn new_chat(server: &Server, provider: &Value, prompt: &str) -> Value {
    let (status, chat) = server.user.json(server.user.post("/api/v1/chats").json(&json!({"provider_id": provider["id"], "prompt": prompt}))).await;
    assert_eq!(status, 201, "{chat}");

    chat
}

async fn reply_finished(server: &Server, chat: &Value) -> Vec<Value> {
    let id = chat["id"].as_str().unwrap().to_string();

    eventually("the reply to end", || async {
        let events = server.events(id.parse().unwrap()).await;

        events.iter().any(|event| event["kind"] == "turn_end").then_some(events)
    })
    .await
}

fn reply_text(events: &[Value]) -> String {
    events.iter().filter_map(|event| event["payload"]["update"]["content"]["text"].as_str()).collect()
}

async fn recorded_activity(server: &Server) -> Value {
    eventually("the turn's usage", || async {
        let (_, activity) = server.user.json(server.user.get("/api/v1/activity").query(&[("range", "today")])).await;

        (activity["totals"]["turns"] == 1).then_some(activity)
    })
    .await
}

async fn search(server: &Server, query: &str) -> Vec<Value> {
    let (status, body) = server.user.json(server.user.get("/api/v1/search").query(&[("q", query)])).await;
    assert_eq!(status, 200, "{body}");

    body.as_array().unwrap().clone()
}

#[tokio::test]
async fn an_openai_compatible_provider_lists_its_models_and_never_returns_its_key() {
    let server = Server::start().await;
    let (router, fake) = fake_openai(Some(OPENAI_KEY));
    let base = serve(router).await;

    let provider = add_openai(&server, &base, Some(OPENAI_KEY)).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/model-providers")).await;
    let stored = server.user.get("/api/v1/model-providers").send().await.unwrap().text().await.unwrap();

    assert_eq!(provider["kind"], "openai_compatible");
    assert_eq!(provider["models"], json!(["llama3.2", "qwen3:8b"]));
    assert_eq!(provider["default_model"], "llama3.2");
    assert_eq!(provider["key_hint"], "••••cdef");
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert!(!provider.to_string().contains(OPENAI_KEY) && !stored.contains(OPENAI_KEY));
    assert!(fake.headers.lock().unwrap().iter().all(|headers| headers.get("authorization").is_some()));
}

#[tokio::test]
async fn an_openai_compatible_provider_without_a_key_connects_like_ollama() {
    let server = Server::start().await;
    let (router, fake) = fake_openai(None);
    let base = serve(router).await;

    let provider = add_openai(&server, &base, None).await;

    assert_eq!(provider["key_hint"], Value::Null);
    assert!(fake.headers.lock().unwrap().iter().all(|headers| headers.get("authorization").is_none()));
}

#[tokio::test]
async fn a_provider_that_cannot_be_used_is_refused_with_what_to_fix_and_nothing_is_stored() {
    let server = Server::start().await;
    let (router, _) = fake_openai(Some(OPENAI_KEY));
    let base = serve(router).await;
    let post = |body: Value| server.user.json(server.user.post("/api/v1/model-providers").json(&body));

    let (_, wrong_key) = post(json!({"kind": "openai_compatible", "name": "Ollama", "base_url": format!("{base}/v1"), "api_key": "wrong"})).await;
    let (_, unreachable) = post(json!({"kind": "openai_compatible", "name": "Ollama", "base_url": "http://127.0.0.1:9/v1"})).await;
    let (_, malformed) = post(json!({"kind": "openai_compatible", "name": "Ollama", "base_url": "localhost:11434"})).await;
    let (_, no_address) = post(json!({"kind": "openai_compatible", "name": "Ollama"})).await;
    let (_, no_name) = post(json!({"kind": "openai_compatible", "name": " ", "base_url": format!("{base}/v1")})).await;
    let (_, no_key) = post(json!({"kind": "anthropic", "name": "Anthropic"})).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/model-providers")).await;

    assert_eq!(wrong_key["error"], "provider_key_rejected");
    assert_eq!(unreachable["error"], "provider_unreachable");
    assert_eq!(malformed["error"], "base_url_invalid");
    assert_eq!(no_address["error"], "base_url_required");
    assert_eq!(no_name["error"], "name_required");
    assert_eq!(no_key["error"], "api_key_required");
    assert_eq!(listed, json!([]));
}

#[tokio::test]
async fn an_anthropic_provider_lists_models_from_the_api_and_defaults_to_sonnet() {
    let server = Server::start().await;
    let (router, fake) = fake_anthropic();
    let base = serve(router).await;

    let provider = add_anthropic(&server, &base).await;

    assert_eq!(provider["models"], json!(["claude-haiku-5-5", "claude-opus-5-5", "claude-sonnet-5-5"]));
    assert_eq!(provider["default_model"], "claude-sonnet-5-5");
    assert_eq!(provider["key_hint"], "••••ghij");
    assert!(!provider.to_string().contains(ANTHROPIC_KEY));
    assert_eq!(fake.headers.lock().unwrap()[0].get("anthropic-version").unwrap(), "2023-06-01");
}

#[tokio::test]
async fn refreshing_a_provider_fetches_its_models_again_and_removing_it_forgets_it() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let id = provider["id"].as_str().unwrap();
    sqlx::query("update model_providers set models = '{}', default_model = null").execute(&server.pool).await.unwrap();

    let (status, refreshed) = server.user.json(server.user.post(&format!("/api/v1/model-providers/{id}/refresh"))).await;
    let other = server.second_user("someone@example.com").await;
    let (hidden, _) = other.json(other.delete(&format!("/api/v1/model-providers/{id}"))).await;
    let (removed, _) = server.user.json(server.user.delete(&format!("/api/v1/model-providers/{id}"))).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/model-providers")).await;

    assert_eq!((status, refreshed["models"].clone(), refreshed["default_model"].clone()), (200, json!(["llama3.2", "qwen3:8b"]), json!("llama3.2")));
    assert_eq!((hidden, removed, listed), (404, 204, json!([])));
}

#[tokio::test]
async fn renaming_a_provider_trims_the_name_and_only_its_owner_can() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let id = provider["id"].as_str().unwrap();
    let path = format!("/api/v1/model-providers/{id}");

    let (status, renamed) = server.user.json(server.user.patch(&path).json(&json!({"name": "  Workstation  "}))).await;
    let (blank, rejected) = server.user.json(server.user.patch(&path).json(&json!({"name": " "}))).await;
    let other = server.second_user("someone@example.com").await;
    let (hidden, _) = other.json(other.patch(&path).json(&json!({"name": "Mine now"}))).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/model-providers")).await;

    assert_eq!((status, renamed["name"].clone(), renamed["models"].clone()), (200, json!("Workstation"), provider["models"].clone()));
    assert_eq!((blank, rejected["error"].clone()), (400, json!("name_required")));
    assert_eq!(hidden, 404);
    assert_eq!(listed[0]["name"], "Workstation");
}

#[tokio::test]
async fn a_favourite_model_is_listed_with_its_provider_and_can_be_removed() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let path = format!("/api/v1/model-providers/{}/favourites", provider["id"].as_str().unwrap());

    let (status, starred) = server.user.json(server.user.put(&path).json(&json!({"model": "qwen3:8b", "favourite": true}))).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/model-providers")).await;
    server.user.json(server.user.put(&path).json(&json!({"model": "qwen3:8b", "favourite": false}))).await;
    let (_, after) = server.user.json(server.user.get("/api/v1/model-providers")).await;

    assert_eq!((status, starred["favourites"].clone(), listed[0]["favourites"].clone()), (200, json!(["qwen3:8b"]), json!(["qwen3:8b"])));
    assert_eq!(after[0]["favourites"], json!([]));
}

#[tokio::test]
async fn favouriting_a_model_twice_stores_it_once() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let path = format!("/api/v1/model-providers/{}/favourites", provider["id"].as_str().unwrap());

    server.user.json(server.user.put(&path).json(&json!({"model": "llama3.2", "favourite": true}))).await;
    let (_, twice) = server.user.json(server.user.put(&path).json(&json!({"model": "llama3.2", "favourite": true}))).await;

    assert_eq!(twice["favourites"], json!(["llama3.2"]));
}

#[tokio::test]
async fn a_model_the_provider_does_not_list_cannot_be_a_favourite() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let path = format!("/api/v1/model-providers/{}/favourites", provider["id"].as_str().unwrap());

    let (status, rejected) = server.user.json(server.user.put(&path).json(&json!({"model": "gpt-9", "favourite": true}))).await;

    assert_eq!((status, rejected["error"].clone()), (400, json!("model_unknown")));
}

#[tokio::test]
async fn only_the_owner_can_favourite_a_providers_model() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let path = format!("/api/v1/model-providers/{}/favourites", provider["id"].as_str().unwrap());
    let other = server.second_user("someone@example.com").await;

    let (status, _) = other.json(other.put(&path).json(&json!({"model": "llama3.2", "favourite": true}))).await;

    assert_eq!(status, 404);
}

#[tokio::test]
async fn refreshing_a_provider_keeps_its_favourites() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let id = provider["id"].as_str().unwrap();
    server.user.json(server.user.put(&format!("/api/v1/model-providers/{id}/favourites")).json(&json!({"model": "llama3.2", "favourite": true}))).await;

    let (_, refreshed) = server.user.json(server.user.post(&format!("/api/v1/model-providers/{id}/refresh"))).await;

    assert_eq!(refreshed["favourites"], json!(["llama3.2"]));
}

#[tokio::test]
async fn a_chat_streams_the_reply_token_by_token_as_conversation_events() {
    let server = Server::start().await;
    let (router, fake) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let mut stream = server.user.stream().await;

    let chat = new_chat(&server, &provider, "Say hello to the lab").await;
    let id = chat["id"].as_str().unwrap().to_string();
    let first = stream.until(|message| match message {
        StreamMessage::Event(event) if event.kind == EventKind::AcpUpdate && event.payload["update"]["sessionUpdate"] == "agent_message_chunk" => Some(event),
        _ => None,
    })
    .await;
    let finished = stream.conversation_with_status("review").await;
    let events = server.events(id.parse().unwrap()).await;
    let chunks: Vec<&str> = events.iter().filter(|event| event["payload"]["update"]["sessionUpdate"] == "agent_message_chunk").filter_map(|event| event["payload"]["update"]["content"]["text"].as_str()).collect();
    let context = events.iter().find(|event| event["payload"]["update"]["sessionUpdate"] == "usage_update").unwrap();

    assert_eq!(first.payload["update"]["sessionUpdate"], "agent_message_chunk");
    assert_eq!(first.payload["update"]["content"]["text"], "Hello");
    assert_eq!(finished.status, ConversationStatus::Review);
    assert_eq!(events.first().unwrap()["kind"], "user_message");
    assert_eq!(events.last().unwrap()["kind"], "turn_end");
    assert_eq!(chunks, ["Hello", " there"]);
    assert_eq!(context["payload"]["update"]["used"], 21);
    assert_eq!(reply_text(&events), "Hello there");
    assert_eq!((chat["agent"].clone(), chat["model"].clone(), chat["provider_id"].clone()), (json!("chat"), json!("llama3.2"), provider["id"].clone()));
    let request = fake.requests.lock().unwrap()[0].clone();
    assert_eq!(request["model"], "llama3.2");
    assert_eq!(request["stream"], true);
    assert_eq!(request["messages"][0]["role"], "system");
    assert!(request["messages"][0]["content"].as_str().unwrap().contains("mycelium-ui"));
}

#[tokio::test]
async fn a_follow_up_message_carries_the_conversation_so_far_and_the_title_is_predicted_from_the_first() {
    let server = Server::start().await;
    let (router, fake) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let chat = new_chat(&server, &provider, "Plan the tailscale rollout\nstarting with the lab").await;
    let id = chat["id"].as_str().unwrap();
    reply_finished(&server, &chat).await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "And then the office"}))).await;
    eventually("the second reply", || async { (server.events(id.parse().unwrap()).await.iter().filter(|event| event["kind"] == "turn_end").count() == 2).then_some(()) }).await;
    let sent = fake.requests.lock().unwrap()[1]["messages"].clone();
    let roles: Vec<&str> = sent.as_array().unwrap().iter().map(|message| message["role"].as_str().unwrap()).collect();

    assert_eq!(status, 204);
    assert_eq!(roles, ["system", "user", "assistant", "user"]);
    assert_eq!(sent[2]["content"], "Hello there");
    assert_eq!(sent[3]["content"], "And then the office");
    assert_eq!(server.conversation(id.parse().unwrap()).await["title"], "Plan the tailscale rollout");
}

#[tokio::test]
async fn an_anthropic_chat_streams_through_the_messages_api_and_records_the_tokens_it_reports() {
    let server = Server::start().await;
    let (router, fake) = fake_anthropic();
    let base = serve(router).await;
    let provider = add_anthropic(&server, &base).await;

    let chat = new_chat(&server, &provider, "Dis bonjour").await;
    let events = reply_finished(&server, &chat).await;
    let activity = recorded_activity(&server).await;
    let request = fake.requests.lock().unwrap()[0].clone();

    assert_eq!(reply_text(&events), "Bonjour tout le monde");
    assert_eq!(request["model"], "claude-sonnet-5-5");
    assert!(request["system"].as_str().unwrap().contains("mycelium-ui"));
    assert_eq!(request["messages"], json!([{"role": "user", "content": [{"type": "text", "text": "Dis bonjour", "cache_control": {"type": "ephemeral"}}]}]));
    assert_eq!((activity["totals"]["turns"].clone(), activity["totals"]["input_tokens"].clone(), activity["totals"]["output_tokens"].clone()), (json!(1), json!(12), json!(5)));
    assert_eq!((activity["totals"]["cache_read_tokens"].clone(), activity["totals"]["cache_write_tokens"].clone()), (json!(300), json!(40)));
    assert_eq!(activity["agents"][0]["key"], "chat");
    assert_eq!(activity["models"][0]["key"], "claude-sonnet-5-5");
    assert_eq!(activity["unreported_agents"], json!([]));
}

#[tokio::test]
async fn an_openai_compatible_chat_records_the_tokens_from_the_final_usage_chunk() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;

    let chat = new_chat(&server, &provider, "Hello").await;
    reply_finished(&server, &chat).await;
    let activity = recorded_activity(&server).await;

    assert_eq!((activity["totals"]["input_tokens"].clone(), activity["totals"]["output_tokens"].clone()), (json!(5), json!(4)));
    assert_eq!((activity["totals"]["cache_read_tokens"].clone(), activity["totals"]["cache_write_tokens"].clone()), (json!(12), json!(4)));
}

#[tokio::test]
async fn stopping_a_chat_ends_the_turn_keeps_what_was_said_and_frees_the_chat() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let chat = new_chat(&server, &provider, "wait for it").await;
    let id = chat["id"].as_str().unwrap();
    eventually("the first words", || async { server.events(id.parse().unwrap()).await.iter().any(|event| event["kind"] == "acp_update").then_some(()) }).await;

    let (busy, error) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "again"}))).await;
    let (stopped, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/cancel"))).await;
    let events = reply_finished(&server, &chat).await;
    let conversation = server.conversation(id.parse().unwrap()).await;

    assert_eq!((busy, error["error"].clone()), (409, json!("chat_busy")));
    assert_eq!(stopped, 204);
    assert_eq!(events.last().unwrap()["payload"]["stop_reason"], "cancelled");
    assert_eq!(conversation["status"], "review");
    assert_eq!(search(&server, "Hello").await.len(), 1);

    let (again, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "carry on"}))).await;
    assert_eq!(again, 204);
}

async fn open_task(server: &Server, title: &str) -> Value {
    let link = server.online_node("laptop").await;
    let (status, task) = server.user.json(server.user.post("/api/v1/tasks").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "title": title}))).await;
    assert_eq!(status, 201, "{task}");

    task
}

fn user_messages(events: &[Value]) -> Vec<String> {
    events.iter().filter(|event| event["kind"] == "user_message").filter_map(|event| event["payload"]["text"].as_str().map(String::from)).collect()
}

#[tokio::test]
async fn a_task_sent_to_an_idle_chat_is_asked_of_it_and_claimed() {
    let server = Server::start().await;
    let (router, fake) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let chat = new_chat(&server, &provider, "hello").await;
    reply_finished(&server, &chat).await;
    let task = open_task(&server, "Tidy up").await;

    let (status, sent) = server.user.json(server.user.post(&format!("/api/v1/tasks/{}/start", task["id"].as_str().unwrap())).json(&json!({"conversation_id": chat["id"]}))).await;
    assert_eq!((status, sent["id"].clone()), (200, chat["id"].clone()), "{sent}");

    eventually("the task to be asked of the chat", || async { (fake.requests.lock().unwrap().len() == 2).then_some(()) }).await;
    assert_eq!(last_text(&fake.requests.lock().unwrap()[1]), "Tidy up");
    let (_, tasks) = server.user.json(server.user.get("/api/v1/tasks")).await;
    assert_eq!((tasks[0]["status"].clone(), tasks[0]["conversation_id"].clone()), (json!("claimed"), chat["id"].clone()));
}

#[tokio::test]
async fn a_task_sent_to_a_replying_chat_waits_in_its_queue_and_is_asked_when_the_reply_ends() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let chat = new_chat(&server, &provider, "wait for it").await;
    let id = chat["id"].as_str().unwrap();
    eventually("the first words", || async { server.events(id.parse().unwrap()).await.iter().any(|event| event["kind"] == "acp_update").then_some(()) }).await;
    let task = open_task(&server, "Tidy up").await;

    let (status, sent) = server.user.json(server.user.post(&format!("/api/v1/tasks/{}/start", task["id"].as_str().unwrap())).json(&json!({"conversation_id": id}))).await;
    assert_eq!(status, 200, "{sent}");

    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{id}/queue"))).await;
    assert_eq!(queue.as_array().unwrap().iter().map(|item| item["text"].clone()).collect::<Vec<_>>(), vec![json!("Tidy up")]);

    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/cancel"))).await;

    let events = eventually("the queued task to be sent", || async {
        let events = server.events(id.parse().unwrap()).await;

        (user_messages(&events).len() == 2).then_some(events)
    })
    .await;
    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{id}/queue"))).await;
    assert_eq!((user_messages(&events)[1].clone(), queue.as_array().unwrap().len()), ("Tidy up".to_string(), 0));
}

#[tokio::test]
async fn a_provider_that_fails_mid_chat_leaves_an_error_in_the_thread_and_the_chat_usable() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;

    let chat = new_chat(&server, &provider, "please break").await;
    let events = reply_finished(&server, &chat).await;
    let id = chat["id"].as_str().unwrap();
    let kinds: Vec<&str> = events.iter().map(|event| event["kind"].as_str().unwrap()).collect();
    let status = server.conversation(id.parse().unwrap()).await["status"].clone();
    let (retry, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "try again"}))).await;

    assert_eq!(kinds, ["user_message", "error", "turn_end"]);
    assert!(events[1]["payload"]["message"].as_str().unwrap().contains("answered with status 500"));
    assert_eq!((status, retry), (json!("review"), 204));
}

#[tokio::test]
async fn an_error_event_from_the_anthropic_stream_ends_the_turn_with_its_message() {
    let server = Server::start().await;
    let (router, _) = fake_anthropic();
    let base = serve(router).await;
    let provider = add_anthropic(&server, &base).await;

    let chat = new_chat(&server, &provider, "overloaded please").await;
    let events = reply_finished(&server, &chat).await;
    let error = events.iter().find(|event| event["kind"] == "error").unwrap();

    assert_eq!(error["payload"]["message"], "The model stopped with an error: Overloaded");
    assert_eq!(events.last().unwrap()["payload"]["stop_reason"], "error");
}

#[tokio::test]
async fn chats_are_found_by_history_search_by_title_and_by_what_was_said() {
    let server = Server::start().await;
    let (router, _) = fake_anthropic();
    let base = serve(router).await;
    let provider = add_anthropic(&server, &base).await;
    let chat = new_chat(&server, &provider, "Compare restic and kopia for offsite backups").await;
    let id = chat["id"].as_str().unwrap();
    reply_finished(&server, &chat).await;

    let by_question = search(&server, "kopia").await;
    let by_answer = search(&server, "Bonjour").await;
    server.user.json(server.user.patch(&format!("/api/v1/conversations/{id}")).json(&json!({"title": "Zebra migration"}))).await;
    let by_new_title = search(&server, "zebra").await;
    let other = server.second_user("someone@example.com").await;
    let (_, theirs) = other.json(other.get("/api/v1/search").query(&[("q", "kopia")])).await;

    assert_eq!(by_question[0]["conversation_id"], chat["id"]);
    assert_eq!(by_answer[0]["conversation_id"], chat["id"]);
    assert_eq!(by_new_title[0]["conversation_id"], chat["id"]);
    assert_eq!(theirs, json!([]));
}

#[tokio::test]
async fn a_chat_can_start_a_linked_side_chat_with_the_same_model() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let chat = new_chat(&server, &provider, "Hello").await;
    let id = chat["id"].as_str().unwrap();
    reply_finished(&server, &chat).await;

    let (status, side) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/side")).json(&json!({"kind": "side", "text": "Compare the two options"}))).await;
    let events = reply_finished(&server, &side).await;

    assert_eq!(status, 201, "{side}");
    assert_eq!((side["agent"].clone(), side["parent_id"].clone(), side["side_kind"].clone(), side["provider_id"].clone()), (json!("chat"), chat["id"].clone(), json!("side"), provider["id"].clone()));
    assert_eq!(reply_text(&events), "Hello there");
}

#[tokio::test]
async fn a_chat_needs_a_provider_you_own_and_a_model() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    sqlx::query("update model_providers set default_model = null").execute(&server.pool).await.unwrap();
    let other = server.second_user("someone@example.com").await;

    let (unknown, _) = server.user.json(server.user.post("/api/v1/chats").json(&json!({"provider_id": uuid::Uuid::new_v4()}))).await;
    let (theirs, _) = other.json(other.post("/api/v1/chats").json(&json!({"provider_id": provider["id"], "model": "llama3.2"}))).await;
    let (_, no_model) = server.user.json(server.user.post("/api/v1/chats").json(&json!({"provider_id": provider["id"]}))).await;
    let (created, empty) = server.user.json(server.user.post("/api/v1/chats").json(&json!({"provider_id": provider["id"], "model": "qwen3:8b"}))).await;

    assert_eq!((unknown, theirs), (404, 404));
    assert_eq!(no_model["error"], "model_required");
    assert_eq!((created, empty["status"].clone(), empty["title"].clone()), (201, json!("done"), json!("New chat")));
}

#[tokio::test]
async fn a_chat_whose_provider_was_removed_says_to_choose_another_model() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let (_, chat) = server.user.json(server.user.post("/api/v1/chats").json(&json!({"provider_id": provider["id"]}))).await;
    let id = chat["id"].as_str().unwrap();
    server.user.json(server.user.delete(&format!("/api/v1/model-providers/{}", provider["id"].as_str().unwrap()))).await;

    let (status, error) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "hello"}))).await;

    assert_eq!((status, error["error"].clone()), (409, json!("provider_removed")));
    assert_eq!(error["message"], "This chat's model provider was removed. Choose another model in the message box to carry on.");
}

fn switch_to(provider: &Value, model: &str) -> Value {
    json!({"provider_id": provider["id"], "model": model})
}

#[tokio::test]
async fn switching_a_chat_to_another_provider_and_model_carries_the_history_and_records_where_it_changed() {
    let server = Server::start().await;
    let (openai, _) = fake_openai(None);
    let (anthropic, claude) = fake_anthropic();
    let openai_base = serve(openai).await;
    let anthropic_base = serve(anthropic).await;
    let ollama = add_openai(&server, &openai_base, None).await;
    let claude_provider = add_anthropic(&server, &anthropic_base).await;
    let chat = new_chat(&server, &ollama, "Plan the rollout").await;
    let id = chat["id"].as_str().unwrap();
    reply_finished(&server, &chat).await;

    let (status, switched) = server.user.json(server.user.patch(&format!("/api/v1/conversations/{id}")).json(&switch_to(&claude_provider, "claude-opus-5-5"))).await;
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "And the office"}))).await;
    eventually("the second reply", || async { (server.events(id.parse().unwrap()).await.iter().filter(|event| event["kind"] == "turn_end").count() == 2).then_some(()) }).await;
    let request = claude.requests.lock().unwrap()[0].clone();
    let events = server.events(id.parse().unwrap()).await;
    let changes: Vec<&Value> = events.iter().filter(|event| event["kind"] == "model_changed").collect();
    let order: Vec<&str> = events.iter().map(|event| event["kind"].as_str().unwrap()).collect();

    assert_eq!(status, 200, "{switched}");
    assert_eq!((switched["provider_id"].clone(), switched["model"].clone()), (claude_provider["id"].clone(), json!("claude-opus-5-5")));
    assert_eq!(request["model"], "claude-opus-5-5");
    assert_eq!(request["messages"], json!([{"role": "user", "content": "Plan the rollout"}, {"role": "assistant", "content": "Hello there"}, {"role": "user", "content": [{"type": "text", "text": "And the office", "cache_control": {"type": "ephemeral"}}]}]));
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["payload"]["provider_name"], "Anthropic");
    assert_eq!(changes[0]["payload"]["model"], "claude-opus-5-5");
    assert!(order.windows(3).any(|events| events == ["turn_end", "model_changed", "user_message"]));
}

#[tokio::test]
async fn choosing_the_model_a_chat_already_uses_records_nothing() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let (_, chat) = server.user.json(server.user.post("/api/v1/chats").json(&json!({"provider_id": provider["id"], "model": "qwen3:8b"}))).await;
    let id = chat["id"].as_str().unwrap();

    let (status, _) = server.user.json(server.user.patch(&format!("/api/v1/conversations/{id}")).json(&switch_to(&provider, "qwen3:8b"))).await;
    let events = server.events(id.parse().unwrap()).await;

    assert_eq!(status, 200);
    assert!(events.iter().all(|event| event["kind"] != "model_changed"));
}

#[tokio::test]
async fn a_chat_whose_provider_was_removed_carries_on_once_another_model_is_chosen() {
    let server = Server::start().await;
    let (gone, _) = fake_openai(None);
    let (anthropic, claude) = fake_anthropic();
    let gone_base = serve(gone).await;
    let anthropic_base = serve(anthropic).await;
    let removed = add_openai(&server, &gone_base, None).await;
    let claude_provider = add_anthropic(&server, &anthropic_base).await;
    let chat = new_chat(&server, &removed, "Plan the rollout").await;
    let id = chat["id"].as_str().unwrap();
    reply_finished(&server, &chat).await;
    server.user.json(server.user.delete(&format!("/api/v1/model-providers/{}", removed["id"].as_str().unwrap()))).await;

    let (status, _) = server.user.json(server.user.patch(&format!("/api/v1/conversations/{id}")).json(&switch_to(&claude_provider, "claude-sonnet-5-5"))).await;
    server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": "Carry on"}))).await;
    eventually("the second reply", || async { (server.events(id.parse().unwrap()).await.iter().filter(|event| event["kind"] == "turn_end").count() == 2).then_some(()) }).await;
    let request = claude.requests.lock().unwrap()[0].clone();

    assert_eq!(status, 200);
    assert_eq!(request["messages"][0]["content"], "Plan the rollout");
    assert_eq!(request["messages"][2]["content"], json!([{"type": "text", "text": "Carry on", "cache_control": {"type": "ephemeral"}}]));
}

#[tokio::test]
async fn switching_a_chat_needs_a_provider_you_own_a_model_and_a_chat() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let chat = new_chat(&server, &provider, "Hello").await;
    let id = chat["id"].as_str().unwrap();
    let other = server.second_user("someone@example.com").await;
    let theirs = other.json(other.post("/api/v1/model-providers").json(&json!({"kind": "openai_compatible", "name": "Theirs", "base_url": format!("{base}/v1")}))).await.1;
    let patch = |body: Value| server.user.json(server.user.patch(&format!("/api/v1/conversations/{id}")).json(&body));

    let (not_theirs, _) = patch(switch_to(&theirs, "llama3.2")).await;
    let (_, empty) = patch(json!({"provider_id": provider["id"], "model": "  "})).await;
    let (_, no_model) = patch(json!({"provider_id": provider["id"]})).await;
    let (_, no_provider) = patch(json!({"model": "llama3.2"})).await;

    assert_eq!(not_theirs, 404);
    assert_eq!(empty["error"], "model_required");
    assert_eq!(empty["message"], "Choose a model for this chat.");
    assert_eq!(no_model["error"], "model_required");
    assert_eq!(no_provider["error"], "provider_required");
    assert_eq!(no_provider["message"], "Choose a model provider for this chat.");
}

#[tokio::test]
async fn chats_that_were_replying_when_the_control_plane_stopped_are_ready_again_after_a_restart() {
    let server = Server::start().await;
    let (router, _) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let (_, chat) = server.user.json(server.user.post("/api/v1/chats").json(&json!({"provider_id": provider["id"]}))).await;
    let id = chat["id"].as_str().unwrap();
    sqlx::query("update conversations set status = 'working' where id = $1::uuid").bind(id).execute(&server.pool).await.unwrap();

    let restarted = Server::start_on(server.database_url.clone(), |_| {}).await;
    let status = eventually("the chat to be ready", || async {
        let conversation = restarted.conversation(id.parse().unwrap()).await;

        (conversation["status"] != "working").then(|| conversation["status"].clone())
    })
    .await;

    assert_eq!(status, "review");
}

async fn upload_to(server: &Server, chat: &Value, name: &str, media_type: &str, bytes: &[u8]) -> Value {
    let id = chat["id"].as_str().unwrap();
    let (status, attachment) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/attachments")).query(&[("name", name)]).header("content-type", media_type).body(bytes.to_vec())).await;
    assert_eq!(status, 201, "{attachment}");

    attachment
}

async fn say_with(server: &Server, chat: &Value, text: &str, attachments: Vec<Value>) {
    let id = chat["id"].as_str().unwrap();
    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": text, "attachments": attachments}))).await;
    assert_eq!(status, 204, "{body}");
}

#[tokio::test]
async fn an_image_in_an_openai_compatible_chat_goes_to_the_provider_as_an_image_url_data_url() {
    let server = Server::start().await;
    let (router, fake) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let chat = new_chat(&server, &provider, "hi").await;
    reply_finished(&server, &chat).await;
    let picture = upload_to(&server, &chat, "shot.png", "image/png", &[137, 80, 78, 71]).await;

    say_with(&server, &chat, "what is this", vec![picture]).await;
    eventually("the second request", || async { (fake.requests.lock().unwrap().len() == 2).then_some(()) }).await;

    let requests = fake.requests.lock().unwrap();
    let sent = requests[1]["messages"].as_array().unwrap().last().unwrap().clone();

    assert_eq!(sent["content"][0], json!({"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw=="}}));
    assert_eq!(sent["content"][1], json!({"type": "text", "text": "what is this"}));
}

#[tokio::test]
async fn an_image_in_an_anthropic_chat_goes_to_the_provider_as_an_image_block() {
    let server = Server::start().await;
    let (router, fake) = fake_anthropic();
    let base = serve(router).await;
    let provider = add_anthropic(&server, &base).await;
    let chat = new_chat(&server, &provider, "hi").await;
    reply_finished(&server, &chat).await;
    let picture = upload_to(&server, &chat, "shot.png", "image/png", &[137, 80, 78, 71]).await;

    say_with(&server, &chat, "what is this", vec![picture]).await;
    eventually("the second request", || async { (fake.requests.lock().unwrap().len() == 2).then_some(()) }).await;

    let requests = fake.requests.lock().unwrap();
    let sent = requests[1]["messages"].as_array().unwrap().last().unwrap().clone();

    assert_eq!(sent["content"][0], json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw=="}}));
}

#[tokio::test]
async fn a_text_file_in_a_chat_is_sent_inline_and_an_earlier_image_is_still_sent_on_later_turns() {
    let server = Server::start().await;
    let (router, fake) = fake_openai(None);
    let base = serve(router).await;
    let provider = add_openai(&server, &base, None).await;
    let chat = new_chat(&server, &provider, "hi").await;
    reply_finished(&server, &chat).await;
    let picture = upload_to(&server, &chat, "shot.png", "image/png", &[137, 80, 78, 71]).await;
    let notes = upload_to(&server, &chat, "notes.txt", "text/plain", b"buy milk").await;

    say_with(&server, &chat, "see these", vec![picture, notes]).await;
    eventually("the second request", || async { (fake.requests.lock().unwrap().len() == 2).then_some(()) }).await;
    eventually("the second reply", || async { (server.events(chat["id"].as_str().unwrap().parse().unwrap()).await.iter().filter(|event| event["kind"] == "turn_end").count() == 2).then_some(()) }).await;
    say_with(&server, &chat, "and again", vec![]).await;
    eventually("the third request", || async { (fake.requests.lock().unwrap().len() == 3).then_some(()) }).await;

    let requests = fake.requests.lock().unwrap();
    let second = requests[1]["messages"].as_array().unwrap().last().unwrap().clone();
    let third = requests[2]["messages"].as_array().unwrap().iter().find(|message| message["content"].is_array()).cloned();

    assert_eq!(second["content"][1]["text"], "see these\n\nnotes.txt:\n```\nbuy milk\n```");
    assert!(third.is_some());
}

#[tokio::test]
async fn a_chat_is_told_which_markdown_is_rendered_only_while_rich_markdown_is_on() {
    let server = Server::start().await;
    let (router, fake) = fake_anthropic();
    let base = serve(router).await;
    let provider = add_anthropic(&server, &base).await;

    let on = new_chat(&server, &provider, "One").await;
    reply_finished(&server, &on).await;
    server.user.json(server.user.put("/api/v1/extensions").json(&json!({"generated_ui": true, "rich_markdown": false, "mycelium_tools": true}))).await;
    let off = new_chat(&server, &provider, "Two").await;
    reply_finished(&server, &off).await;
    let requests = fake.requests.lock().unwrap().clone();

    assert!(requests[0]["system"].as_str().unwrap().contains("Raw HTML is not rendered."));
    assert!(!requests[1]["system"].as_str().unwrap().contains("Raw HTML is not rendered."));
}
