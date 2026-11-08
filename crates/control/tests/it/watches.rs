use crate::common::{NodeLink, Server};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

pub async fn fast() -> Server {
    Server::start_with(|config| config.scheduler_tick = Duration::from_millis(50)).await
}

pub async fn begun(server: &Server, link: &mut NodeLink, prompt: &str) -> Uuid {
    let conversation = server.create_conversation(link.id, Some(prompt)).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, &format!("acp-{}", conversation.id)).await;
    link.expect_prompt().await;

    conversation.id
}

pub async fn end_turn(link: &mut NodeLink, conversation_id: Uuid, seq: i64) {
    link.send(NodeToControl::TurnEnd { conversation_id, seq, stop_reason: "end_turn".into(), usage: None }).await;
    link.barrier(conversation_id, seq + 1).await;
}

pub async fn prompt_for(link: &mut NodeLink, conversation: Uuid) -> String {
    loop {
        if let ControlToNode::Prompt { conversation_id, text, .. } = link.next().await
            && conversation_id == conversation
        {
            return text;
        }
    }
}

pub async fn quiet_for(link: &mut NodeLink, conversation: Uuid) -> bool {
    let waited = tokio::time::timeout(Duration::from_millis(700), prompt_for(link, conversation)).await;

    waited.is_err()
}

async fn connect(server: &Server, a: Uuid, b: Uuid) {
    let owner = sqlx::query_scalar::<_, Uuid>("select owner from conversations where id = $1").bind(a).fetch_one(&server.pool).await.unwrap();
    sqlx::query("insert into conversation_connections (owner, source_id, target_id) values ($1, least($2::uuid, $3::uuid), greatest($2::uuid, $3::uuid))").bind(owner).bind(a).bind(b).execute(&server.pool).await.unwrap();
}

pub async fn tool(server: &Server, link: &NodeLink, conversation: Uuid, name: &str, arguments: Value) -> Value {
    let reply = server.agent_mcp(&link.token, conversation, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}})).await;

    reply["result"].clone()
}

pub fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

#[tokio::test]
async fn a_watched_conversation_tells_its_watcher_once_when_its_turn_ends() {
    let server = fast().await;
    let mut link = server.online_node("laptop").await;
    let watcher = begun(&server, &mut link, "Plan the release").await;
    end_turn(&mut link, watcher, 1).await;
    let target = begun(&server, &mut link, "Build the release").await;
    connect(&server, watcher, target).await;

    let watched = tool(&server, &link, watcher, "watch_conversation", json!({"conversation": target.to_string()})).await;
    end_turn(&mut link, target, 1).await;
    let told = prompt_for(&mut link, watcher).await;
    link.send(NodeToControl::TurnEnd { conversation_id: target, seq: 1, stop_reason: "end_turn".into(), usage: None }).await;

    assert_eq!(watched["isError"], false, "{watched}");
    assert!(told.contains("finished its turn"), "{told}");
    assert!(quiet_for(&mut link, watcher).await, "the same turn end must not notify twice");
}

#[tokio::test]
async fn a_turn_that_ends_with_an_open_question_is_waiting_on_the_person() {
    let server = fast().await;
    let mut link = server.online_node("laptop").await;
    let watcher = begun(&server, &mut link, "Plan the release").await;
    end_turn(&mut link, watcher, 1).await;
    let target = begun(&server, &mut link, "Build the release").await;
    connect(&server, watcher, target).await;
    tool(&server, &link, watcher, "watch_conversation", json!({"conversation": target.to_string()})).await;

    let asked = tool(&server, &link, target, "ask_question", json!({"question": "Which version number?"})).await;
    end_turn(&mut link, target, 1).await;
    let told = prompt_for(&mut link, watcher).await;

    assert_eq!(asked["isError"], false, "{asked}");
    assert!(told.contains("is waiting on the person"), "{told}");
    assert_eq!(server.conversation(target).await["status"], "input");
}

#[tokio::test]
async fn a_failed_session_tells_its_watcher_it_failed() {
    let server = fast().await;
    let mut link = server.online_node("laptop").await;
    let watcher = begun(&server, &mut link, "Plan the release").await;
    end_turn(&mut link, watcher, 1).await;
    let target = begun(&server, &mut link, "Build the release").await;
    connect(&server, watcher, target).await;
    tool(&server, &link, watcher, "watch_conversation", json!({"conversation": target.to_string()})).await;

    link.send(NodeToControl::SessionFailed { conversation_id: target, seq: 1, message: "The agent stopped".into() }).await;
    let told = prompt_for(&mut link, watcher).await;

    assert!(told.contains("failed"), "{told}");
}

#[tokio::test]
async fn notifications_that_arrive_while_the_watcher_works_come_as_one_turn() {
    let server = fast().await;
    let mut link = server.online_node("laptop").await;
    let watcher = begun(&server, &mut link, "Plan the release").await;
    let first = begun(&server, &mut link, "Build the app").await;
    let second = begun(&server, &mut link, "Build the site").await;
    connect(&server, watcher, first).await;
    connect(&server, watcher, second).await;
    tool(&server, &link, watcher, "watch_conversation", json!({"conversation": first.to_string()})).await;
    tool(&server, &link, watcher, "watch_conversation", json!({"conversation": second.to_string()})).await;

    end_turn(&mut link, first, 1).await;
    end_turn(&mut link, second, 1).await;
    end_turn(&mut link, watcher, 1).await;
    let told = prompt_for(&mut link, watcher).await;

    assert!(told.contains(&first.to_string()) && told.contains(&second.to_string()), "{told}");
}

#[tokio::test]
async fn a_turn_started_by_a_notification_does_not_notify_so_two_watchers_cannot_loop() {
    let server = fast().await;
    let mut link = server.online_node("laptop").await;
    let a = begun(&server, &mut link, "Plan the release").await;
    end_turn(&mut link, a, 1).await;
    let b = begun(&server, &mut link, "Build the release").await;
    connect(&server, a, b).await;
    tool(&server, &link, a, "watch_conversation", json!({"conversation": b.to_string()})).await;
    tool(&server, &link, b, "watch_conversation", json!({"conversation": a.to_string()})).await;

    end_turn(&mut link, b, 1).await;
    prompt_for(&mut link, a).await;
    end_turn(&mut link, a, 2).await;

    assert!(quiet_for(&mut link, b).await, "a's turn came from a notification, so b must not hear of it");
}

#[tokio::test]
async fn a_conversation_without_access_cannot_be_watched() {
    let server = fast().await;
    let mut link = server.online_node("laptop").await;
    let watcher = begun(&server, &mut link, "Plan the release").await;
    let target = begun(&server, &mut link, "Somebody else's work").await;

    let refused = tool(&server, &link, watcher, "watch_conversation", json!({"conversation": target.to_string()})).await;
    let listed = tool(&server, &link, watcher, "list_watches", json!({})).await;

    assert_eq!(refused["isError"], true, "{refused}");
    assert!(text(&refused).starts_with("Nothing is watched."), "{refused}");
    assert_eq!(serde_json::from_str::<Value>(&text(&listed)).unwrap()["watches"], json!([]));
}

#[tokio::test]
async fn unwatch_stops_the_notifications() {
    let server = fast().await;
    let mut link = server.online_node("laptop").await;
    let watcher = begun(&server, &mut link, "Plan the release").await;
    end_turn(&mut link, watcher, 1).await;
    let target = begun(&server, &mut link, "Build the release").await;
    connect(&server, watcher, target).await;
    tool(&server, &link, watcher, "watch_conversation", json!({"conversation": target.to_string()})).await;

    let stopped = tool(&server, &link, watcher, "unwatch", json!({"watch": target.to_string()})).await;
    end_turn(&mut link, target, 1).await;

    assert_eq!(stopped["isError"], false, "{stopped}");
    assert!(quiet_for(&mut link, watcher).await);
}

#[tokio::test]
async fn a_wake_up_set_before_a_restart_is_delivered_by_the_restarted_control_plane() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let watcher = begun(&server, &mut link, "Plan the release").await;
    let at = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();

    let set = tool(&server, &link, watcher, "wake_at", json!({"at": at, "note": "Check the release run finished"})).await;
    sqlx::query("update watches set due_at = now() where watcher_id = $1").bind(watcher).execute(&server.pool).await.unwrap();
    let restarted = Server::start_on(server.database_url.clone(), |config| config.scheduler_tick = Duration::from_millis(50)).await;

    let mut queued = Vec::new();

    for _ in 0..100 {
        queued = sqlx::query_scalar::<_, String>("select text from queued_prompts where conversation_id = $1").bind(watcher).fetch_all(&restarted.pool).await.unwrap();

        if !queued.is_empty() {
            break;
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert_eq!(set["isError"], false, "{set}");
    assert!(queued.iter().any(|text| text.contains("Check the release run finished")), "{queued:?}");
}

#[tokio::test]
async fn the_person_sees_a_conversations_watches_and_can_end_one() {
    let server = fast().await;
    let mut link = server.online_node("laptop").await;
    let watcher = begun(&server, &mut link, "Plan the release").await;
    tool(&server, &link, watcher, "wake_at", json!({"in_minutes": 60, "note": "Post the release notes"})).await;

    let (_, listed) = server.user.json(server.user.get(&format!("/api/v1/conversations/{watcher}/watches"))).await;
    let watch = listed[0]["id"].as_str().unwrap().to_string();
    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/watches/{watch}"))).await;
    let (_, after) = server.user.json(server.user.get(&format!("/api/v1/conversations/{watcher}/watches"))).await;

    assert_eq!((listed[0]["kind"].clone(), listed[0]["note"].clone()), (json!("timer"), json!("Post the release notes")));
    assert_eq!(status, 204);
    assert_eq!(after, json!([]));
}
