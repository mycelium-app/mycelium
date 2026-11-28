use crate::common::{self, NodeLink, Server};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::{Value, json};
use uuid::Uuid;

const STEERING_NODE: &str = "0.10.1";

struct Rig {
    server: Server,
    link: NodeLink,
    host: Uuid,
    assistant: String,
}

async fn rig(version: &str) -> Rig {
    let server = Server::start().await;
    let mut link = server.online_node_as("laptop", version).await;
    let conversation = server.create_conversation(link.id, Some("Plan the week")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;

    while !matches!(link.next().await, ControlToNode::Prompt { .. }) {}
    let (status, assistant) = server.user.json(server.user.post("/api/v1/assistants").json(&json!({"name": "Release captain", "instructions": "Be brief."}))).await;
    assert_eq!(status, 201, "{assistant}");
    sqlx::query("update assistants set conversation_id = $1 where id = $2::uuid").bind(conversation.id).bind(assistant["id"].as_str().unwrap()).execute(&server.pool).await.unwrap();

    Rig { server, link, host: conversation.id, assistant: assistant["id"].as_str().unwrap().to_string() }
}

impl Rig {
    async fn chat(&self, title: &str) -> Value {
        let (status, chat) = self.server.user.json(self.server.user.post(&format!("/api/v1/assistants/{}/virtual-chats", self.assistant)).json(&json!({"title": title}))).await;
        assert_eq!(status, 201, "{chat}");

        chat
    }

    async fn messages(&self, chat: &Value) -> Vec<Value> {
        let (_, messages) = self.server.user.json(self.server.user.get(&format!("/api/v1/virtual-chats/{}/messages", chat["id"].as_str().unwrap()))).await;

        messages.as_array().unwrap().clone()
    }

    async fn tool(&self, tool: &str, arguments: Value) -> Value {
        common::call_tool(&self.server, &self.link.token, self.host, tool, arguments).await
    }

    async fn say(&self, chat: &Value, text: &str) -> (u16, Value) {
        self.server.user.json(self.server.user.post(&format!("/api/v1/virtual-chats/{}/messages", chat["id"].as_str().unwrap())).json(&json!({"text": text}))).await
    }

    async fn end_turn(&mut self, seq: i64, text: &str) {
        self.link.send(NodeToControl::AcpUpdate { conversation_id: self.host, seq, update: json!({"update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}}) }).await;
        self.link.send(NodeToControl::TurnEnd { conversation_id: self.host, seq: seq + 1, stop_reason: "end_turn".into(), usage: None }).await;
    }
}

fn text_of(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

async fn next_but_git(link: &mut NodeLink) -> ControlToNode {
    loop {
        let message = link.next().await;

        if !matches!(message, ControlToNode::Git { .. }) {
            return message;
        }
    }
}

#[tokio::test]
async fn a_virtual_message_naming_a_chat_lands_in_it_and_one_naming_none_stays_inline() {
    let rig = rig(STEERING_NODE).await;
    let chat = rig.chat("Discord routing rules").await;

    let posted = rig.tool("post_virtual_message", json!({"chat": "discord routing rules", "text": "Which layout?"})).await;
    let inline = rig.tool("post_virtual_message", json!({"text": "Calendar row B is handed over."})).await;

    assert_eq!(text_of(&posted), "Posted to the virtual chat Discord routing rules.");
    assert_eq!(text_of(&inline), "Shown to the person in this conversation.");
    let messages = rig.messages(&chat).await;
    assert_eq!(messages.len(), 1);
    assert_eq!((messages[0]["text"].clone(), messages[0]["from_person"].clone()), (json!("Which layout?"), json!(false)));
}

#[tokio::test]
async fn a_conversation_that_is_not_an_assistants_cannot_post_to_a_named_chat() {
    let rig = rig(STEERING_NODE).await;
    sqlx::query("update assistants set conversation_id = null").execute(&rig.server.pool).await.unwrap();

    let posted = rig.tool("post_virtual_message", json!({"chat": "Anything", "text": "Hello"})).await;

    assert_eq!(posted["isError"], json!(true), "{posted}");
}

#[tokio::test]
async fn the_model_lists_its_chats_and_reads_one_chats_history() {
    let rig = rig(STEERING_NODE).await;
    let chat = rig.chat("Mac mini install").await;
    rig.tool("post_virtual_message", json!({"chat": chat["id"], "text": "Is the disk wiped?"})).await;
    rig.tool("set_todos", json!({"chat": "Mac mini install", "todos": [{"content": "Wipe the disk", "status": "in_progress"}]})).await;

    let listed = rig.tool("list_virtual_chats", json!({})).await;
    let read = rig.tool("read_virtual_chat", json!({"chat": "Mac mini install"})).await;

    assert!(text_of(&listed).contains("Mac mini install"), "{listed}");
    let read = text_of(&read);
    assert!(read.contains("Is the disk wiped?") && read.contains("\"from\": \"assistant\"") && read.contains("Wipe the disk"), "{read}");
    let (_, chats) = rig.server.user.json(rig.server.user.get(&format!("/api/v1/assistants/{}/virtual-chats", rig.assistant))).await;
    assert_eq!(chats[0]["todos"][0]["content"], json!("Wipe the disk"));
}

#[tokio::test]
async fn the_model_opens_a_chat_without_asking_and_can_send_its_first_message_with_it() {
    let rig = rig(STEERING_NODE).await;

    let opened = rig.tool("create_virtual_chat", json!({"title": "Decision needed: SSH cap", "message": "Which cap, **a**, b or c?"})).await;
    let empty = rig.tool("create_virtual_chat", json!({"title": "Calendar pinned row"})).await;

    assert!(text_of(&opened).starts_with("Opened the virtual chat Decision needed: SSH cap"), "{opened}");
    assert!(text_of(&empty).starts_with("Opened the virtual chat Calendar pinned row"), "{empty}");
    let asked = rig.server.events(rig.host).await.iter().filter(|event| event["kind"] == "permission_request").count();
    assert_eq!(asked, 0);
    let (_, chats) = rig.server.user.json(rig.server.user.get(&format!("/api/v1/assistants/{}/virtual-chats", rig.assistant))).await;
    let first = rig.messages(&chats[0]).await;
    assert_eq!((first.len(), first[0]["text"].clone()), (1, json!("Which cap, **a**, b or c?")));
    assert!(rig.messages(&chats[1]).await.is_empty());
}

#[tokio::test]
async fn the_model_closes_a_chat_and_the_person_writing_in_it_opens_it_again() {
    let rig = rig(STEERING_NODE).await;
    let chat = rig.chat("Decision needed: SSH cap").await;

    let closed = rig.tool("close_virtual_chat", json!({"chat": "Decision needed: SSH cap"})).await;
    let (_, after_close) = rig.server.user.json(rig.server.user.get(&format!("/api/v1/assistants/{}/virtual-chats", rig.assistant))).await;
    rig.say(&chat, "one more thing").await;
    let (_, after_writing) = rig.server.user.json(rig.server.user.get(&format!("/api/v1/assistants/{}/virtual-chats", rig.assistant))).await;

    assert_eq!(closed["isError"], json!(false), "{closed}");
    assert_eq!((after_close[0]["closed"].clone(), after_writing[0]["closed"].clone()), (json!(true), json!(false)));
}

#[tokio::test]
async fn a_message_waits_for_a_running_tool_call_to_finish_before_it_is_steered_in() {
    let mut rig = rig(STEERING_NODE).await;
    let chat = rig.chat("Discord routing rules").await;
    rig.link.send(NodeToControl::AcpUpdate { conversation_id: rig.host, seq: 2, update: json!({"update": {"sessionUpdate": "tool_call", "toolCallId": "post-1", "title": "mcp__mycelium__post_virtual_message", "kind": "other", "status": "in_progress"}}) }).await;
    common::eventually("the tool call to be recorded", || async {
        rig.server.events(rig.host).await.iter().any(|event| event["payload"]["update"]["toolCallId"] == "post-1").then_some(())
    })
    .await;

    rig.say(&chat, "can the fallback be any server?").await;
    let early = tokio::time::timeout(std::time::Duration::from_millis(800), next_but_git(&mut rig.link)).await;
    rig.link.send(NodeToControl::AcpUpdate { conversation_id: rig.host, seq: 3, update: json!({"update": {"sessionUpdate": "tool_call_update", "toolCallId": "post-1", "status": "completed"}}) }).await;
    let ControlToNode::Steer { text, .. } = next_but_git(&mut rig.link).await else {
        panic!("expected a steer");
    };

    assert!(early.is_err(), "the steer came while the tool call was running: {early:?}");
    assert!(text.starts_with("can the fallback be any server?"), "{text}");
}

#[tokio::test]
async fn a_message_to_an_idle_session_is_sent_and_the_turns_answer_comes_back_to_the_chat() {
    let mut rig = rig(STEERING_NODE).await;
    rig.end_turn(2, "Planned.").await;
    let chat = rig.chat("Discord routing rules").await;
    common::eventually("the turn to end", || async { (rig.server.conversation(rig.host).await["status"] != "working").then_some(()) }).await;

    let (status, sent) = rig.say(&chat, "C please").await;
    let prompt = loop {
        if let ControlToNode::Prompt { text, .. } = next_but_git(&mut rig.link).await {
            break text;
        }
    };
    rig.end_turn(5, "C is with the owner.").await;

    assert_eq!(status, 201, "{sent}");
    assert!(prompt.starts_with("C please") && prompt.contains("Discord routing rules"), "{prompt}");
    let messages = common::eventually("the answer in the chat", || async {
        let messages = rig.messages(&chat).await;

        (messages.len() == 2).then_some(messages)
    })
    .await;
    assert_eq!((messages[0]["sending"].clone(), messages[1]["text"].clone()), (json!(false), json!("C is with the owner.")));
    let mirrored = rig.server.events(rig.host).await.iter().filter(|event| event["kind"] == "user_message" && event["payload"]["origin"] == "virtual_chat").count();
    assert_eq!(mirrored, 1);
}

#[tokio::test]
async fn a_message_sent_while_the_session_works_is_steered_in_and_stops_showing_sending() {
    let mut rig = rig(STEERING_NODE).await;
    let chat = rig.chat("Discord routing rules").await;

    let (status, sent) = rig.say(&chat, "can the fallback be any server?").await;
    assert_eq!((status, sent["sending"].clone()), (201, json!(true)), "{sent}");
    let ControlToNode::Steer { request_id, text, .. } = next_but_git(&mut rig.link).await else {
        panic!("expected a steer");
    };
    rig.link.send(NodeToControl::Steered { request_id, outcome: "injected".into(), error: None }).await;

    assert!(text.starts_with("can the fallback be any server?"), "{text}");
    common::eventually("the message to be read", || async { (rig.messages(&chat).await[0]["sending"] == json!(false)).then_some(()) }).await;
    let (cancelled, _) = rig.server.user.json(rig.server.user.delete(&format!("/api/v1/virtual-chats/{}/messages/{}", chat["id"].as_str().unwrap(), sent["id"].as_str().unwrap()))).await;
    assert_eq!(cancelled, 409);
}

#[tokio::test]
async fn a_message_not_yet_read_can_be_cancelled_and_never_reaches_the_model() {
    let mut rig = rig("0.10.0").await;
    let chat = rig.chat("Discord routing rules").await;
    let (_, sent) = rig.say(&chat, "never mind").await;

    let (cancelled, _) = rig.server.user.json(rig.server.user.delete(&format!("/api/v1/virtual-chats/{}/messages/{}", chat["id"].as_str().unwrap(), sent["id"].as_str().unwrap()))).await;
    rig.end_turn(2, "Planned.").await;

    assert_eq!(cancelled, 204);
    assert!(rig.messages(&chat).await.is_empty());
    common::eventually("the turn to end", || async { (rig.server.conversation(rig.host).await["status"] != "working").then_some(()) }).await;
    let prompts = rig.server.events(rig.host).await.iter().filter(|event| event["kind"] == "user_message").count();
    assert_eq!(prompts, 1);
}

#[tokio::test]
async fn closing_keeps_the_chat_and_writing_in_it_opens_it_again() {
    let rig = rig(STEERING_NODE).await;
    let chat = rig.chat("Mac mini install").await;
    let id = chat["id"].as_str().unwrap();

    let (_, closed) = rig.server.user.json(rig.server.user.patch(&format!("/api/v1/virtual-chats/{id}")).json(&json!({"closed": true}))).await;
    rig.say(&chat, "one more thing").await;
    let (_, chats) = rig.server.user.json(rig.server.user.get(&format!("/api/v1/assistants/{}/virtual-chats", rig.assistant))).await;

    assert_eq!((closed["closed"].clone(), chats[0]["closed"].clone()), (json!(true), json!(false)));
}

#[tokio::test]
async fn a_new_chat_and_a_message_in_it_are_streamed() {
    let rig = rig(STEERING_NODE).await;
    let mut stream = rig.server.user.stream().await;

    let chat = rig.chat("Mac mini install").await;
    rig.tool("post_virtual_message", json!({"chat": chat["id"], "text": "Is the disk wiped?"})).await;

    let streamed_chat = stream.until(|message| match message {
        mycelium_proto::stream::StreamMessage::VirtualChat(chat) => Some(chat),
        _ => None,
    })
    .await;
    let streamed_message = stream.until(|message| match message {
        mycelium_proto::stream::StreamMessage::VirtualMessage(message) => Some(message),
        _ => None,
    })
    .await;
    assert_eq!((streamed_chat.title.as_str(), streamed_message.text.as_str()), ("Mac mini install", "Is the disk wiped?"));
}
