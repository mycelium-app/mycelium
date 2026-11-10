use crate::common::{self, NodeLink, Server};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

const UPDATE: &str = "Update Mycelium on atlas: docker compose pull && docker compose up -d";

struct Setup {
    server: Server,
    link: NodeLink,
    chat: Uuid,
}

async fn setup() -> Setup {
    let server = Server::start_with(|config| config.scheduler_tick = Duration::from_millis(50)).await;
    let link = server.online_node("laptop").await;
    let chat = server.create_conversation(link.id, None).await.id;

    Setup { server, link, chat }
}

async fn tool(setup: &Setup, caller: Uuid, name: &str, arguments: Value) -> Value {
    let reply = setup.server.agent_mcp(&setup.link.token, caller, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}})).await;

    reply["result"].clone()
}

async fn add(setup: &Setup, caller: Uuid, text: &str) -> String {
    let reply = tool(setup, caller, "add_action_item", json!({"text": text})).await;
    assert_eq!(reply["isError"], json!(false), "{reply}");
    let message = reply["content"][0]["text"].as_str().unwrap();

    message.split_whitespace().nth(3).unwrap().to_string()
}

async fn items(setup: &Setup, id: Uuid) -> Vec<Value> {
    let conversation = setup.server.conversation(id).await;

    conversation.get("action_items").map(|items| items.as_array().unwrap().clone()).unwrap_or_default()
}

fn texts(items: &[Value]) -> Vec<&str> {
    items.iter().map(|item| item["text"].as_str().unwrap()).collect()
}

#[tokio::test]
async fn an_added_action_item_shows_on_the_conversation() {
    let setup = setup().await;

    let item = add(&setup, setup.chat, UPDATE).await;

    let shown = items(&setup, setup.chat).await;
    assert_eq!(texts(&shown), [UPDATE]);
    assert_eq!(shown[0]["id"], item);
    assert!(shown[0]["created_at"].is_string());
}

#[tokio::test]
async fn a_conversation_holds_several_action_items_oldest_first() {
    let setup = setup().await;

    add(&setup, setup.chat, UPDATE).await;
    add(&setup, setup.chat, "Send Alex the invoice").await;

    assert_eq!(texts(&items(&setup, setup.chat).await), [UPDATE, "Send Alex the invoice"]);
}

#[tokio::test]
async fn the_person_marking_an_item_done_removes_it_from_the_conversation() {
    let setup = setup().await;
    let item = add(&setup, setup.chat, UPDATE).await;
    add(&setup, setup.chat, "Send Alex the invoice").await;

    let (status, conversation) = setup.server.user.json(setup.server.user.delete(&format!("/api/v1/conversations/{}/action-items/{item}", setup.chat))).await;

    assert_eq!(status, 200, "{conversation}");
    assert_eq!(texts(&items(&setup, setup.chat).await), ["Send Alex the invoice"]);
    assert_eq!(conversation["id"], setup.chat.to_string());
    assert_eq!(conversation["action_items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn marking_an_item_done_twice_is_not_found() {
    let setup = setup().await;
    let item = add(&setup, setup.chat, UPDATE).await;
    let path = format!("/api/v1/conversations/{}/action-items/{item}", setup.chat);
    setup.server.user.json(setup.server.user.delete(&path)).await;

    let (status, _) = setup.server.user.json(setup.server.user.delete(&path)).await;

    assert_eq!(status, 404);
}

#[tokio::test]
async fn writing_in_the_conversation_does_not_clear_an_action_item() {
    let setup = setup().await;
    add(&setup, setup.chat, UPDATE).await;

    let (status, body) = setup.server.user.json(setup.server.user.post(&format!("/api/v1/conversations/{}/prompt", setup.chat)).json(&json!({"text": "I will do it after lunch"}))).await;

    assert!(status < 300, "{body}");
    assert_eq!(texts(&items(&setup, setup.chat).await), [UPDATE]);
}

#[tokio::test]
async fn the_agent_resolving_its_own_item_removes_it() {
    let setup = setup().await;
    let item = add(&setup, setup.chat, UPDATE).await;

    let reply = tool(&setup, setup.chat, "resolve_action_item", json!({"item": item})).await;

    assert_eq!(reply["isError"], json!(false), "{reply}");
    assert!(items(&setup, setup.chat).await.is_empty());
}

#[tokio::test]
async fn resolving_an_unknown_item_is_refused() {
    let setup = setup().await;
    add(&setup, setup.chat, UPDATE).await;

    let unknown = tool(&setup, setup.chat, "resolve_action_item", json!({"item": Uuid::new_v4().to_string()})).await;
    let malformed = tool(&setup, setup.chat, "resolve_action_item", json!({"item": "the update"})).await;

    assert_eq!((unknown["isError"].clone(), malformed["isError"].clone()), (json!(true), json!(true)));
    assert_eq!(texts(&items(&setup, setup.chat).await), [UPDATE]);
}

#[tokio::test]
async fn resolving_another_conversations_item_is_refused() {
    let setup = setup().await;
    let other = setup.server.create_conversation(setup.link.id, None).await.id;
    let item = add(&setup, setup.chat, UPDATE).await;

    let reply = tool(&setup, other, "resolve_action_item", json!({"item": item})).await;

    assert_eq!(reply["isError"], json!(true));
    assert_eq!(texts(&items(&setup, setup.chat).await), [UPDATE]);
}

#[tokio::test]
async fn another_owner_cannot_see_or_finish_the_items() {
    let setup = setup().await;
    let item = add(&setup, setup.chat, UPDATE).await;
    let sam = setup.server.second_user("sam@example.com").await;

    let (shown, _) = sam.json(sam.get(&format!("/api/v1/conversations/{}", setup.chat))).await;
    let (finished, _) = sam.json(sam.delete(&format!("/api/v1/conversations/{}/action-items/{item}", setup.chat))).await;

    assert_eq!((shown, finished), (404, 404));
    assert_eq!(texts(&items(&setup, setup.chat).await), [UPDATE]);
}

#[tokio::test]
async fn a_managed_conversation_with_an_open_action_item_is_not_archived_when_idle() {
    let setup = setup().await;
    let managed = setup.server.create_conversation(setup.link.id, None).await.id;
    let quiet = setup.server.create_conversation(setup.link.id, None).await.id;
    add(&setup, managed, UPDATE).await;
    for id in [managed, quiet] {
        sqlx::query("update conversations set managed_by = $2 where id = $1").bind(id).bind(setup.chat).execute(&setup.server.pool).await.unwrap();
        sqlx::query("update conversations set created_at = now() - interval '4 days', updated_at = now() - interval '4 days', last_activity_at = now() - interval '4 days' where id = $1").bind(id).execute(&setup.server.pool).await.unwrap();
        sqlx::query("update events set created_at = now() - interval '4 days' where conversation_id = $1").bind(id).execute(&setup.server.pool).await.unwrap();
    }

    common::eventually("the quiet conversation to be archived", || async { setup.server.conversation(quiet).await["archived_at"].is_string().then_some(()) }).await;

    assert!(setup.server.conversation(managed).await["archived_at"].is_null());
}
