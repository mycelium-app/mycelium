use crate::common::Server;
use serde_json::{Value, json};

fn message(uuid: &str, sender: &str, index: i64, parent: &str, blocks: Vec<Value>) -> Value {
    json!({
        "uuid": uuid, "sender": sender, "index": index, "created_at": format!("2026-01-01T10:0{index}:00Z"),
        "parent_message_uuid": parent, "text": "", "content": blocks, "attachments": [], "files": []
    })
}

fn text(body: &str) -> Value {
    json!({"type": "text", "text": body, "citations": []})
}

fn export(uuid: &str, name: &str, leaf: Option<&str>, messages: Vec<Value>) -> Value {
    json!({
        "uuid": uuid, "name": name, "summary": "", "model": "claude-opus-4-5-20251101",
        "created_at": "2026-01-01T10:00:00Z", "updated_at": "2026-01-02T11:30:00Z",
        "current_leaf_message_uuid": leaf, "chat_messages": messages
    })
}

const ROOT: &str = "00000000-0000-0000-0000-000000000000";

fn simple(uuid: &str, name: &str) -> Value {
    export(uuid, name, Some("a1"), vec![
        message("h1", "human", 0, ROOT, vec![text("How do I stream a response in zebra?")]),
        message("a1", "assistant", 1, "h1", vec![json!({"type": "thinking", "thinking": "Consider pipes."}), text("Use a pipe to the quokka.")]),
    ])
}

async fn import(server: &Server, body: &Value) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chats/import").header("content-type", "application/json").body(body.to_string())).await
}

async fn chats(server: &Server) -> Vec<Value> {
    let (_, all) = server.user.json(server.user.get("/api/v1/conversations")).await;

    all.as_array().unwrap().iter().filter(|conversation| conversation["agent"] == "chat").cloned().collect()
}

fn kinds(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .map(|event| match event["payload"]["update"]["sessionUpdate"].as_str() {
            Some(update) => update.to_string(),
            None => event["kind"].as_str().unwrap().to_string(),
        })
        .collect()
}

fn texts(events: &[Value], kind: &str) -> Vec<String> {
    events
        .iter()
        .filter(|event| event["kind"] == kind)
        .map(|event| event["payload"]["text"].as_str().unwrap().to_string())
        .collect()
}

fn chunks(events: &[Value], update: &str) -> Vec<String> {
    events
        .iter()
        .filter(|event| event["payload"]["update"]["sessionUpdate"] == update)
        .map(|event| event["payload"]["update"]["content"]["text"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn a_single_conversation_becomes_a_chat_with_its_title_dates_model_and_events() {
    let server = Server::start().await;
    let (status, result) = import(&server, &simple("c1", "Streaming zebras")).await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result, json!({"imported": 1, "skipped": 0}));

    let imported = chats(&server).await;
    assert_eq!(imported.len(), 1);
    let chat = &imported[0];
    assert_eq!(chat["title"], "Streaming zebras");
    assert_eq!(chat["model"], "claude-opus-4-5-20251101");
    assert_eq!(chat["provider_id"], Value::Null);
    assert_eq!(chat["created_at"].as_str().unwrap()[..10], *"2026-01-01");
    assert_eq!(chat["updated_at"].as_str().unwrap()[..10], *"2026-01-02");

    let events = server.events(chat["id"].as_str().unwrap().parse().unwrap()).await;
    assert_eq!(kinds(&events), ["user_message", "agent_thought_chunk", "agent_message_chunk", "turn_end"]);
    assert_eq!(texts(&events, "user_message"), ["How do I stream a response in zebra?"]);
    assert_eq!(chunks(&events, "agent_thought_chunk"), ["Consider pipes."]);
    assert_eq!(chunks(&events, "agent_message_chunk"), ["Use a pipe to the quokka."]);
}

#[tokio::test]
async fn imported_messages_are_searchable() {
    let server = Server::start().await;
    import(&server, &simple("c1", "Streaming zebras")).await;

    let (_, hits) = server.user.json(server.user.get("/api/v1/search").query(&[("q", "quokka")])).await;

    assert_eq!(hits.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn an_account_export_array_imports_every_conversation() {
    let server = Server::start().await;
    let (status, result) = import(&server, &json!([simple("c1", "One"), simple("c2", "Two"), simple("c3", "Three")])).await;

    assert_eq!(status, 200, "{result}");
    assert_eq!(result, json!({"imported": 3, "skipped": 0}));
    assert_eq!(chats(&server).await.len(), 3);
}

#[tokio::test]
async fn only_the_active_branch_is_imported() {
    let server = Server::start().await;
    let branched = export("c1", "Branches", Some("a2b"), vec![
        message("h1", "human", 0, ROOT, vec![text("first question")]),
        message("a1", "assistant", 1, "h1", vec![text("first answer")]),
        message("h2a", "human", 2, "a1", vec![text("abandoned follow-up")]),
        message("a2a", "assistant", 3, "h2a", vec![text("abandoned answer")]),
        message("h2b", "human", 4, "a1", vec![text("chosen follow-up")]),
        message("a2b", "assistant", 5, "h2b", vec![text("chosen answer")]),
    ]);
    import(&server, &branched).await;

    let chat = &chats(&server).await[0];
    let events = server.events(chat["id"].as_str().unwrap().parse().unwrap()).await;

    assert_eq!(texts(&events, "user_message"), ["first question", "chosen follow-up"]);
    assert_eq!(chunks(&events, "agent_message_chunk"), ["first answer", "chosen answer"]);
}

#[tokio::test]
async fn without_a_leaf_the_messages_are_imported_in_index_order() {
    let server = Server::start().await;
    let unordered = export("c1", "No leaf", None, vec![
        message("a1", "assistant", 1, "h1", vec![text("answer")]),
        message("h1", "human", 0, ROOT, vec![text("question")]),
    ]);
    import(&server, &unordered).await;

    let chat = &chats(&server).await[0];
    let events = server.events(chat["id"].as_str().unwrap().parse().unwrap()).await;

    assert_eq!(kinds(&events), ["user_message", "agent_message_chunk", "turn_end"]);
}

#[tokio::test]
async fn unknown_blocks_and_attachments_are_kept_as_readable_text_without_failing() {
    let server = Server::start().await;
    let mut question = message("h1", "human", 0, ROOT, vec![text("summarise the notes")]);
    question["attachments"] = json!([{"file_name": "notes.txt", "file_type": "txt", "file_size": 9, "extracted_content": "marmot burrows"}]);
    let odd = export("c1", "Odd blocks", Some("a1"), vec![
        question,
        message("a1", "assistant", 1, "h1", vec![
            json!({"type": "tool_use", "name": "web_search", "input": {"query": "marmots"}}),
            json!({"type": "tool_result", "name": "web_search", "content": [{"type": "text", "text": "rodent facts"}]}),
            json!({"type": "hologram", "payload": 7}),
            text("Done."),
        ]),
    ]);
    let (status, result) = import(&server, &odd).await;
    assert_eq!(status, 200, "{result}");

    let chat = &chats(&server).await[0];
    let events = server.events(chat["id"].as_str().unwrap().parse().unwrap()).await;
    let said = texts(&events, "user_message").join("\n");
    let replied = chunks(&events, "agent_message_chunk").join("\n");

    assert!(said.contains("notes.txt") && said.contains("marmot burrows"), "{said}");
    assert!(replied.contains("web_search") && replied.contains("rodent facts") && replied.contains("Done."), "{replied}");
}

#[tokio::test]
async fn importing_the_same_export_again_skips_what_is_already_there() {
    let server = Server::start().await;
    import(&server, &json!([simple("c1", "One"), simple("c2", "Two")])).await;

    let (status, result) = import(&server, &json!([simple("c1", "One"), simple("c2", "Two"), simple("c3", "Three")])).await;

    assert_eq!(status, 200, "{result}");
    assert_eq!(result, json!({"imported": 1, "skipped": 2}));
    assert_eq!(chats(&server).await.len(), 3);
}

#[tokio::test]
async fn another_user_importing_the_same_export_gets_their_own_chats() {
    let server = Server::start().await;
    let other = server.second_user("other@example.com").await;
    import(&server, &simple("c1", "One")).await;

    let (status, result) = other.json(other.post("/api/v1/chats/import").header("content-type", "application/json").body(simple("c1", "One").to_string())).await;

    assert_eq!(status, 200, "{result}");
    assert_eq!(result["imported"], 1);
}

#[tokio::test]
async fn a_file_that_is_not_a_claude_export_is_refused_with_how_to_fix_it() {
    let server = Server::start().await;

    for body in ["not json at all", "{\"hello\": 1}", "[1, 2]", "[]"] {
        let (status, error) = server.user.json(server.user.post("/api/v1/chats/import").body(body.to_string())).await;

        assert_eq!(status, 400, "{body}: {error}");
        assert_eq!(error["error"], "not_claude_export");
        assert_eq!(error["message"], "The file is not a Claude export. Choose the conversations.json from your Claude data export.");
    }

    assert!(chats(&server).await.is_empty());
}
