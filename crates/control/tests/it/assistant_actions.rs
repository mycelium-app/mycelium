use crate::common;
use crate::common::{Client, NodeLink, Server};
use mycelium_proto::link::ControlToNode;
use serde_json::{Value, json};
use uuid::Uuid;

async fn next_matching<T>(link: &mut NodeLink, pick: impl Fn(&ControlToNode) -> Option<T>) -> T {
    loop {
        let message = link.next().await;

        if let Some(found) = pick(&message) {
            return found;
        }
    }
}

async fn assistant_on(server: &Server, link: &NodeLink) -> String {
    let (status, assistant) = server.user.json(server.user.post("/api/v1/assistants").json(&json!({"name": "Stand-up", "instructions": "Be brief.", "preferences": {"models": [{"agent": "claude"}], "nodes": [link.id]}}))).await;
    assert_eq!(status, 201, "{assistant}");

    assistant["id"].as_str().unwrap().to_string()
}

async fn save_action(client: &Client, assistant: &str, body: Value) -> (u16, Value) {
    client.json(client.post(&format!("/api/v1/assistants/{assistant}/actions")).json(&body)).await
}

fn summary_action() -> Value {
    json!({
        "name": "Summarise a meeting",
        "prompt": "Summarise {meeting} on {date} as {style}.",
        "inputs": [
            {"name": "meeting", "kind": "text", "required": true},
            {"name": "date", "kind": "date", "required": false},
            {"name": "style", "kind": "choice", "options": ["bullets", "prose"], "required": false, "default": "bullets"},
        ],
        "pinned": true,
    })
}

async fn first_prompt(link: &mut NodeLink) -> (Uuid, String) {
    let conversation = next_matching(link, |message| match message {
        ControlToNode::StartSession { conversation_id, .. } => Some(*conversation_id),
        _ => None,
    })
    .await;

    link.start_session(conversation, "acp-1").await;

    let (_, text) = link.expect_prompt().await;

    (conversation, text)
}

#[tokio::test]
async fn an_action_is_created_listed_edited_and_deleted_per_assistant() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let assistant = assistant_on(&server, &link).await;

    let (status, created) = save_action(&server.user, &assistant, summary_action()).await;
    assert_eq!((status, created["pinned"].clone(), created["inputs"][2]["default"].clone()), (201, json!(true), json!("bullets")), "{created}");
    let id = created["id"].as_str().unwrap();
    let (duplicate, body) = save_action(&server.user, &assistant, summary_action()).await;
    assert_eq!((duplicate, body["error"].clone()), (409, json!("name_taken")));

    let mut edited = summary_action();
    edited["name"] = json!("Summarise");
    edited["pinned"] = json!(false);
    let (status, updated) = server.user.json(server.user.put(&format!("/api/v1/assistants/{assistant}/actions/{id}")).json(&edited)).await;
    assert_eq!((status, updated["name"].clone(), updated["pinned"].clone()), (200, json!("Summarise"), json!(false)), "{updated}");

    let (_, listed) = server.user.json(server.user.get(&format!("/api/v1/assistants/{assistant}/actions"))).await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    let (deleted, _) = server.user.json(server.user.delete(&format!("/api/v1/assistants/{assistant}/actions/{id}"))).await;
    let (_, after) = server.user.json(server.user.get(&format!("/api/v1/assistants/{assistant}/actions"))).await;
    assert_eq!((deleted, after.as_array().unwrap().len()), (204, 0));
}

#[tokio::test]
async fn an_action_with_a_bad_input_is_refused_and_stores_nothing() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let assistant = assistant_on(&server, &link).await;

    for (inputs, error) in [
        (json!([{"name": "has space", "kind": "text"}]), "input_name_invalid"),
        (json!([{"name": "a", "kind": "text"}, {"name": "a", "kind": "date"}]), "input_name_taken"),
        (json!([{"name": "pick", "kind": "choice"}]), "input_options_required"),
    ] {
        let (status, body) = save_action(&server.user, &assistant, json!({"name": "Bad", "prompt": "Do {a}", "inputs": inputs})).await;

        assert_eq!((status, body["error"].clone()), (400, json!(error)));
    }

    let (status, body) = save_action(&server.user, &assistant, json!({"name": " ", "prompt": "Do it"})).await;
    assert_eq!((status, body["error"].clone()), (400, json!("name_required")));
    let (_, listed) = server.user.json(server.user.get(&format!("/api/v1/assistants/{assistant}/actions"))).await;
    assert!(listed.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn running_an_action_sends_the_expanded_prompt_to_the_assistant() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let assistant = assistant_on(&server, &link).await;
    let (_, created) = save_action(&server.user, &assistant, summary_action()).await;
    let id = created["id"].as_str().unwrap();

    let (status, started) = server.user.json(server.user.post(&format!("/api/v1/assistants/{assistant}/actions/{id}/run")).json(&json!({"inputs": {"meeting": "the design review", "date": "2026-10-26"}}))).await;
    assert_eq!(status, 200, "{started}");
    let (conversation, text) = first_prompt(&mut link).await;

    assert_eq!(text, "Summarise the design review on 2026-10-26 as bullets.");
    assert_eq!(started["id"], json!(conversation));
}

#[tokio::test]
async fn an_action_saved_with_a_model_still_runs_on_the_assistants_own_settings() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let assistant = assistant_on(&server, &link).await;
    let mut body = summary_action();
    body["model"] = json!({"agent": "codex", "model": "gpt-5.5"});
    let (status, created) = save_action(&server.user, &assistant, body).await;
    assert_eq!((status, created["model"].clone()), (201, Value::Null), "{created}");
    let id = created["id"].as_str().unwrap();

    server.user.json(server.user.post(&format!("/api/v1/assistants/{assistant}/actions/{id}/run")).json(&json!({"inputs": {"meeting": "the design review"}}))).await;
    let agent = next_matching(&mut link, |message| match message {
        ControlToNode::StartSession { agent, .. } => Some(serde_json::to_value(agent).unwrap()),
        _ => None,
    })
    .await;

    assert_eq!(agent, json!("claude"));
}

#[tokio::test]
async fn running_an_action_without_a_required_input_or_with_a_bad_choice_says_what_to_fix() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let assistant = assistant_on(&server, &link).await;
    let (_, created) = save_action(&server.user, &assistant, summary_action()).await;
    let path = format!("/api/v1/assistants/{assistant}/actions/{}/run", created["id"].as_str().unwrap());

    let (status, missing) = server.user.json(server.user.post(&path).json(&json!({"inputs": {}}))).await;
    assert_eq!((status, missing["error"].clone(), missing["message"].clone()), (400, json!("input_required"), json!("Enter a value for meeting.")));
    let (status, bad_date) = server.user.json(server.user.post(&path).json(&json!({"inputs": {"meeting": "x", "date": "tomorrow"}}))).await;
    assert_eq!((status, bad_date["error"].clone()), (400, json!("input_date_invalid")));
    let (status, bad_choice) = server.user.json(server.user.post(&path).json(&json!({"inputs": {"meeting": "x", "style": "poem"}}))).await;
    assert_eq!((status, bad_choice["error"].clone()), (400, json!("input_choice_invalid")));
}

#[tokio::test]
async fn a_schedule_can_run_an_action_with_fixed_inputs() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let assistant = assistant_on(&server, &link).await;
    let (_, created) = save_action(&server.user, &assistant, summary_action()).await;

    let (status, schedule) = server.user.json(server.user.post("/api/v1/schedules").json(&json!({"assistant_id": assistant, "action_id": created["id"], "action_inputs": {"meeting": "stand-up", "date": "2026-10-27"}, "prompt": "Summarise a meeting", "cron": "0 9 * * 1-5", "each_run": "same"}))).await;
    assert_eq!((status, schedule["node_id"].clone(), schedule["assistant_id"].clone()), (201, Value::Null, json!(assistant)), "{schedule}");
    let schedule_id = schedule["id"].as_str().unwrap();
    let (status, run) = server.user.json(server.user.post(&format!("/api/v1/schedules/{schedule_id}/run"))).await;
    assert!(status < 300, "{run}");
    let (_, text) = first_prompt(&mut link).await;

    assert_eq!(text, "Summarise stand-up on 2026-10-27 as bullets.");
}

#[tokio::test]
async fn a_schedule_for_an_action_with_a_missing_input_is_refused() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let assistant = assistant_on(&server, &link).await;
    let (_, created) = save_action(&server.user, &assistant, summary_action()).await;

    let (status, body) = server.user.json(server.user.post("/api/v1/schedules").json(&json!({"assistant_id": assistant, "action_id": created["id"], "prompt": "Summarise", "cron": "0 9 * * 1-5", "each_run": "same"}))).await;

    assert_eq!((status, body["error"].clone()), (400, json!("input_required")));
}

async fn propose(server: &Server, link: &mut NodeLink, conversation: Uuid, arguments: Value) -> Value {
    let body = server.agent_mcp(&link.token, conversation, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "assistant_propose_action", "arguments": arguments}})).await;

    body["result"].clone()
}

#[tokio::test]
async fn a_proposed_action_waits_for_approval_and_can_be_edited_or_declined() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let assistant = assistant_on(&server, &link).await;
    server.user.json(server.user.post(&format!("/api/v1/assistants/{assistant}/prompt")).json(&json!({"text": "Hello"}))).await;
    let (conversation, _) = first_prompt(&mut link).await;

    let proposed = propose(&server, &mut link, conversation, json!({"name": "Weekly digest", "prompt": "Digest {week}", "inputs": [{"name": "week", "kind": "date"}], "reason": "You ask for this every Friday"})).await;
    assert_ne!(proposed["isError"], true, "{proposed}");
    let (_, listed) = server.user.json(server.user.get(&format!("/api/v1/assistants/{assistant}/action-proposals"))).await;
    let proposal = listed[0]["id"].as_str().unwrap().to_string();
    assert_eq!((listed[0]["state"].clone(), listed[0]["reason"].clone()), (json!("proposed"), json!("You ask for this every Friday")));
    let (_, none_yet) = server.user.json(server.user.get(&format!("/api/v1/assistants/{assistant}/actions"))).await;
    assert!(none_yet.as_array().unwrap().is_empty());

    let (status, approved) = server.user.json(server.user.post(&format!("/api/v1/assistants/{assistant}/action-proposals/{proposal}/approve")).json(&json!({"name": "Friday digest", "prompt": "Digest {week}", "inputs": [{"name": "week", "kind": "date"}], "pinned": true}))).await;
    assert_eq!((status, approved["name"].clone(), approved["pinned"].clone()), (200, json!("Friday digest"), json!(true)), "{approved}");
    let (again, _) = server.user.json(server.user.post(&format!("/api/v1/assistants/{assistant}/action-proposals/{proposal}/approve"))).await;
    assert_eq!(again, 409);

    let second = propose(&server, &mut link, conversation, json!({"name": "Daily note", "prompt": "Write a note"})).await;
    assert_ne!(second["isError"], true, "{second}");
    let (_, listed) = server.user.json(server.user.get(&format!("/api/v1/assistants/{assistant}/action-proposals"))).await;
    let waiting = listed.as_array().unwrap().iter().find(|proposal| proposal["state"] == "proposed").unwrap()["id"].as_str().unwrap().to_string();
    let (status, declined) = server.user.json(server.user.post(&format!("/api/v1/assistants/{assistant}/action-proposals/{waiting}/decline"))).await;
    assert_eq!((status, declined["state"].clone()), (200, json!("declined")));
}

#[tokio::test]
async fn an_action_cannot_be_proposed_outside_an_assistant_or_with_a_taken_name() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let plain = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(plain.id, "acp-plain").await;

    let refused = propose(&server, &mut link, plain.id, json!({"name": "Anything", "prompt": "Do it"})).await;
    assert_eq!(refused["isError"], true, "{refused}");

    let assistant = assistant_on(&server, &link).await;
    server.user.json(server.user.post(&format!("/api/v1/assistants/{assistant}/prompt")).json(&json!({"text": "Hello"}))).await;
    let (conversation, _) = first_prompt(&mut link).await;
    save_action(&server.user, &assistant, json!({"name": "Digest", "prompt": "Digest"})).await;

    let taken = propose(&server, &mut link, conversation, json!({"name": "digest", "prompt": "Digest again"})).await;
    assert_eq!(taken["isError"], true, "{taken}");
}

async fn tool(server: &Server, link: &NodeLink, conversation: Uuid, name: &str, arguments: Value) -> Value {
    let body = server.agent_mcp(&link.token, conversation, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}})).await;

    body["result"].clone()
}

fn text_of(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

#[tokio::test]
async fn an_agent_lists_assistants_and_a_creation_waits_for_approval() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    let assistant = assistant_on(&server, &link).await;

    let listed = tool(&server, &link, conversation.id, "assistant_list", json!({})).await;
    assert!(text_of(&listed).contains(&assistant) && text_of(&listed).contains("Stand-up"), "{listed}");

    let proposed = tool(&server, &link, conversation.id, "assistant_create", json!({"name": "Jellyfin", "description": "Looks after the media library", "instructions": "Manage the library."})).await;
    assert_ne!(proposed["isError"], true, "{proposed}");
    let (_, assistants) = server.user.json(server.user.get("/api/v1/assistants")).await;
    assert_eq!(assistants.as_array().unwrap().iter().filter(|assistant| assistant["builtin_key"].is_null()).count(), 1);

    let (_, changes) = server.user.json(server.user.get("/api/v1/assistant-changes")).await;
    let change = changes[0]["id"].as_str().unwrap().to_string();
    assert_eq!((changes[0]["kind"].clone(), changes[0]["state"].clone()), (json!("create"), json!("proposed")));
    let (status, approved) = server.user.json(server.user.post(&format!("/api/v1/assistant-changes/{change}/approve"))).await;
    assert_eq!((status, approved["state"].clone()), (200, json!("approved")), "{approved}");
    let (_, assistants) = server.user.json(server.user.get("/api/v1/assistants")).await;
    let names: Vec<&str> = assistants.as_array().unwrap().iter().filter(|assistant| assistant["builtin_key"].is_null()).map(|assistant| assistant["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Jellyfin", "Stand-up"]);
}

#[tokio::test]
async fn a_declined_creation_makes_no_assistant_and_a_taken_name_is_refused_up_front() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    assistant_on(&server, &link).await;

    let taken = tool(&server, &link, conversation.id, "assistant_create", json!({"name": "Stand-up", "instructions": "x"})).await;
    assert_eq!(taken["isError"], true, "{taken}");
    tool(&server, &link, conversation.id, "assistant_create", json!({"name": "Jellyfin", "instructions": "Manage the library."})).await;
    let (_, changes) = server.user.json(server.user.get("/api/v1/assistant-changes")).await;
    let change = changes[0]["id"].as_str().unwrap();

    let (status, declined) = server.user.json(server.user.post(&format!("/api/v1/assistant-changes/{change}/decline"))).await;

    assert_eq!((status, declined["state"].clone()), (200, json!("declined")));
    let (_, assistants) = server.user.json(server.user.get("/api/v1/assistants")).await;
    assert_eq!(assistants.as_array().unwrap().iter().filter(|assistant| assistant["builtin_key"].is_null()).count(), 1);
}

#[tokio::test]
async fn updating_settings_applies_at_once_and_can_be_undone_while_instructions_wait_for_approval() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    let assistant = assistant_on(&server, &link).await;

    let updated = tool(&server, &link, conversation.id, "assistant_update", json!({"id": assistant, "description": "Daily stand-up notes", "instructions": "Be very brief.", "reason": "Shorter answers"})).await;
    assert_ne!(updated["isError"], true, "{updated}");
    let (_, held) = server.user.json(server.user.get(&format!("/api/v1/assistants/{assistant}"))).await;
    assert_eq!((held["description"].clone(), held["instructions"].clone()), (json!("Daily stand-up notes"), json!("Be brief.")));

    let (_, proposals) = server.user.json(server.user.get("/api/v1/instruction-proposals")).await;
    let proposal = proposals.as_array().unwrap().iter().find(|proposal| proposal["layer"] == json!(format!("assistant:{assistant}"))).unwrap();
    assert_eq!((proposal["state"].clone(), proposal["text"].clone(), proposal["base_text"].clone()), (json!("proposed"), json!("Be very brief."), json!("Be brief.")));

    let (_, changes) = server.user.json(server.user.get("/api/v1/assistant-changes")).await;
    let change = changes[0]["id"].as_str().unwrap().to_string();
    assert_eq!((changes[0]["kind"].clone(), changes[0]["state"].clone()), (json!("update"), json!("applied")));
    let (status, undone) = server.user.json(server.user.post(&format!("/api/v1/assistant-changes/{change}/undo"))).await;
    assert_eq!((status, undone["state"].clone()), (200, json!("undone")));
    let (_, restored) = server.user.json(server.user.get(&format!("/api/v1/assistants/{assistant}"))).await;
    assert_ne!(restored["description"], json!("Daily stand-up notes"));

    let id = proposal["id"].as_str().unwrap();
    let (status, approved) = server.user.json(server.user.post(&format!("/api/v1/instruction-proposals/{id}/approve"))).await;
    assert_eq!((status, approved["state"].clone()), (200, json!("approved")), "{approved}");
    let (_, after) = server.user.json(server.user.get(&format!("/api/v1/assistants/{assistant}"))).await;
    assert_eq!(after["instructions"], json!("Be very brief."));
}

#[tokio::test]
async fn updating_with_a_bad_id_or_nothing_to_change_says_so() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    let assistant = assistant_on(&server, &link).await;

    let unknown = tool(&server, &link, conversation.id, "assistant_update", json!({"id": Uuid::new_v4(), "description": "x"})).await;
    let empty = tool(&server, &link, conversation.id, "assistant_update", json!({"id": assistant})).await;

    assert_eq!((unknown["isError"].clone(), empty["isError"].clone()), (json!(true), json!(true)));
}

#[tokio::test]
async fn a_scoped_token_with_assistants_reads_and_creates_but_instruction_changes_become_proposals() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let assistant = assistant_on(&server, &link).await;
    let secret = Uuid::new_v4().to_string();
    sqlx::query("insert into client_tokens (user_id, label, token_hash, scopes) values ((select id from users limit 1), 'Test', $1, $2)").bind(common::hash(&secret)).bind(vec!["assistants".to_string()]).execute(&server.pool).await.unwrap();
    let scoped = server.anonymous();
    let with_token = |request: reqwest::RequestBuilder| request.header("authorization", format!("Bearer myc_client_{secret}"));

    let (status, _) = scoped.json(with_token(scoped.get("/api/v1/assistants"))).await;
    let (created, _) = scoped.json(with_token(scoped.post("/api/v1/assistants")).json(&json!({"name": "Made by API", "instructions": "Hi"}))).await;
    let (patched, held) = scoped.json(with_token(scoped.patch(&format!("/api/v1/assistants/{assistant}"))).json(&json!({"instructions": "Changed by API"}))).await;
    let (secrets, _) = scoped.json(with_token(scoped.get(&format!("/api/v1/assistants/{assistant}/secrets")))).await;

    assert_eq!((status, created, patched, secrets), (200, 201, 200, 403));
    assert_eq!(held["instructions"], json!("Be brief."));
    let (_, proposals) = server.user.json(server.user.get("/api/v1/instruction-proposals")).await;
    assert!(proposals.as_array().unwrap().iter().any(|proposal| proposal["text"] == "Changed by API" && proposal["state"] == "proposed"));
}

#[tokio::test]
async fn an_assistant_schedule_needs_no_node() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let assistant = assistant_on(&server, &link).await;

    let (status, schedule) = server.user.json(server.user.post("/api/v1/schedules").json(&json!({"assistant_id": assistant, "prompt": "Wake up", "cron": "0 9 * * *", "each_run": "same"}))).await;
    let (_, held) = server.user.json(server.user.get(&format!("/api/v1/assistants/{assistant}"))).await;

    assert_eq!((status, schedule["agent"].clone(), held["wakes"].clone()), (201, Value::Null, json!("Daily at 09:00")), "{schedule}");
}
