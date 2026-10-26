mod common;

use common::{NodeLink, Server};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

async fn call(server: &Server, link: &NodeLink, conversation: Uuid, tool: &str, arguments: Value) -> Value {
    let anonymous = server.anonymous();
    let request = anonymous
        .post("/api/v1/mcp")
        .header("authorization", format!("Bearer {}", link.token))
        .header("x-mycelium-conversation", conversation.to_string())
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": tool, "arguments": arguments}}));
    let (status, body) = anonymous.json(request).await;
    assert_eq!(status, 200, "{body}");

    body["result"].clone()
}

async fn working_conversation(server: &Server, link: &mut NodeLink, title: &str, folder: &str) -> Uuid {
    let conversation = server.create_conversation(link.id, Some("Plan the release")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, &format!("acp-{}", conversation.id)).await;
    link.expect_prompt().await;
    sqlx::query("update conversations set title = $2, cwd = $3 where id = $1").bind(conversation.id).bind(title).bind(folder).execute(&server.pool).await.unwrap();

    conversation.id
}

async fn queue(server: &Server, conversation: Uuid) -> Vec<Value> {
    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/queue"))).await;

    queue.as_array().unwrap().clone()
}

async fn proposals(server: &Server) -> Vec<Value> {
    let (_, proposals) = server.user.json(server.user.get("/api/v1/message-proposals")).await;

    proposals.as_array().unwrap().clone()
}

async fn propose(server: &Server, link: &NodeLink, from: Uuid, to: Uuid) -> Value {
    call(server, link, from, "send_to_conversation", json!({"conversation": to.to_string(), "message": "The schema is ready", "reason": "You asked for the schema"})).await
}

async fn turn_on(server: &Server) {
    sqlx::query("insert into user_settings (owner, experiment_chat_across_groups) select id, true from users on conflict (owner) do update set experiment_chat_across_groups = true").execute(&server.pool).await.unwrap();
}

async fn group(server: &Server, name: &str, folder: &str) -> Uuid {
    let (status, created) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": name}))).await;
    assert_eq!(status, 201, "{created}");
    let id: Uuid = created["id"].as_str().unwrap().parse().unwrap();
    let (status, claimed) = server.user.json(server.user.post(&format!("/api/v1/groups/{id}/folders")).json(&json!({"prefix": folder}))).await;
    assert_eq!(status, 201, "{claimed}");

    id
}

async fn connect(server: &Server, first: Uuid, second: Uuid) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/group-connections").json(&json!({"source_group_id": first, "target_group_id": second}))).await
}

struct Fleet {
    server: Server,
    link: NodeLink,
    yucca: Uuid,
    immich: Uuid,
    yucca_chat: Uuid,
    immich_chat: Uuid,
}

async fn fleet() -> Fleet {
    let server = Server::start().await;
    turn_on(&server).await;
    let mut link = server.online_node("laptop").await;
    let yucca = group(&server, "yucca", "/work/yucca").await;
    let immich = group(&server, "Immich", "/work/immich").await;
    let yucca_chat = working_conversation(&server, &mut link, "Plan the release", "/work/yucca/app").await;
    let immich_chat = working_conversation(&server, &mut link, "Fix billing export", "/work/immich/app").await;

    Fleet { server, link, yucca, immich, yucca_chat, immich_chat }
}

#[tokio::test]
async fn group_connections_are_refused_until_the_experiment_is_on() {
    let server = Server::start().await;
    let first = group(&server, "yucca", "/work/yucca").await;
    let second = group(&server, "Immich", "/work/immich").await;

    let (status, refused) = connect(&server, first, second).await;
    assert_eq!((status, refused["error"].clone()), (403, json!("experiment_disabled")));

    turn_on(&server).await;
    let (status, created) = connect(&server, first, second).await;
    assert_eq!(status, 201, "{created}");
}

#[tokio::test]
async fn connecting_two_groups_lists_it_and_revoking_removes_it() {
    let fleet = fleet().await;
    let mut stream = fleet.server.user.stream().await;

    let (status, created) = connect(&fleet.server, fleet.immich, fleet.yucca).await;
    assert_eq!(status, 201, "{created}");
    let names = [created["source_name"].as_str().unwrap(), created["target_name"].as_str().unwrap()];
    assert!(names.contains(&"yucca") && names.contains(&"Immich"), "{created}");

    let streamed = stream.until(|message| match message {
        StreamMessage::GroupConnection(connection) => Some(connection),
        _ => None,
    }).await;
    assert!(streamed.revoked_at.is_none());

    let (_, listed) = fleet.server.user.json(fleet.server.user.get("/api/v1/group-connections")).await;
    assert_eq!(listed.as_array().unwrap().len(), 1);

    let id = created["id"].as_str().unwrap();
    let (status, revoked) = fleet.server.user.json(fleet.server.user.delete(&format!("/api/v1/group-connections/{id}"))).await;
    assert_eq!(status, 200, "{revoked}");
    assert!(revoked["revoked_at"].is_string());

    let (_, listed) = fleet.server.user.json(fleet.server.user.get("/api/v1/group-connections")).await;
    assert_eq!(listed.as_array().unwrap().len(), 0);

    let (again, _) = fleet.server.user.json(fleet.server.user.delete(&format!("/api/v1/group-connections/{id}"))).await;
    assert_eq!(again, 404);
}

#[tokio::test]
async fn a_group_cannot_be_connected_to_itself_or_to_someone_elses_group() {
    let fleet = fleet().await;
    let other = fleet.server.second_user("other@example.com").await;
    let (_, foreign) = other.json(other.post("/api/v1/groups").json(&json!({"name": "theirs"}))).await;
    let foreign: Uuid = foreign["id"].as_str().unwrap().parse().unwrap();

    let (status, same) = connect(&fleet.server, fleet.yucca, fleet.yucca).await;
    assert_eq!((status, same["error"].clone(), same["message"].clone()), (400, json!("connection_self"), json!("Choose two different groups.")));

    let (status, refused) = connect(&fleet.server, fleet.yucca, foreign).await;
    assert_eq!(status, 404, "{refused}");
}

#[tokio::test]
async fn a_message_between_connected_groups_is_sent_without_approval_in_both_directions() {
    let fleet = fleet().await;
    connect(&fleet.server, fleet.yucca, fleet.immich).await;

    let sent = propose(&fleet.server, &fleet.link, fleet.yucca_chat, fleet.immich_chat).await;
    assert_eq!(sent["isError"], false, "{sent}");
    let queued = queue(&fleet.server, fleet.immich_chat).await;
    assert_eq!(queued[0]["text"], json!("From Plan the release:\n\nThe schema is ready"));

    propose(&fleet.server, &fleet.link, fleet.immich_chat, fleet.yucca_chat).await;
    assert_eq!(queue(&fleet.server, fleet.yucca_chat).await.len(), 1);
    assert!(proposals(&fleet.server).await.iter().all(|proposal| proposal["state"] == "sent"));
}

#[tokio::test]
async fn a_message_between_unconnected_or_revoked_groups_waits_for_approval() {
    let fleet = fleet().await;

    propose(&fleet.server, &fleet.link, fleet.yucca_chat, fleet.immich_chat).await;
    assert_eq!(queue(&fleet.server, fleet.immich_chat).await.len(), 0);

    let (_, created) = connect(&fleet.server, fleet.yucca, fleet.immich).await;
    let id = created["id"].as_str().unwrap();
    fleet.server.user.json(fleet.server.user.delete(&format!("/api/v1/group-connections/{id}"))).await;

    propose(&fleet.server, &fleet.link, fleet.yucca_chat, fleet.immich_chat).await;
    assert_eq!(queue(&fleet.server, fleet.immich_chat).await.len(), 0);
    assert!(proposals(&fleet.server).await.iter().all(|proposal| proposal["state"] == "proposed"));
}

#[tokio::test]
async fn a_message_from_a_conversation_with_no_group_waits_for_approval() {
    let mut fleet = fleet().await;
    connect(&fleet.server, fleet.yucca, fleet.immich).await;
    let loose = working_conversation(&fleet.server, &mut fleet.link, "Loose chat", "/elsewhere").await;

    propose(&fleet.server, &fleet.link, loose, fleet.immich_chat).await;

    assert_eq!(queue(&fleet.server, fleet.immich_chat).await.len(), 0);
    let waiting = proposals(&fleet.server).await;
    assert_eq!((waiting[0]["state"].clone(), waiting[0]["source_group"].clone(), waiting[0]["target_group"].clone()), (json!("proposed"), Value::Null, json!("Immich")));

    let id = waiting[0]["id"].as_str().unwrap();
    let (status, refused) = fleet.server.user.json(fleet.server.user.post(&format!("/api/v1/message-proposals/{id}/connect-groups"))).await;
    assert_eq!((status, refused["error"].clone()), (409, json!("connection_no_group")));
}

#[tokio::test]
async fn sending_and_connecting_groups_sends_once_and_connects_the_groups() {
    let fleet = fleet().await;
    propose(&fleet.server, &fleet.link, fleet.yucca_chat, fleet.immich_chat).await;
    let waiting = proposals(&fleet.server).await;
    assert_eq!((waiting[0]["source_group"].clone(), waiting[0]["target_group"].clone()), (json!("yucca"), json!("Immich")));
    let id = waiting[0]["id"].as_str().unwrap();

    let (status, sent) = fleet.server.user.json(fleet.server.user.post(&format!("/api/v1/message-proposals/{id}/connect-groups"))).await;

    assert_eq!((status, sent["state"].clone()), (200, json!("sent")), "{sent}");
    assert_eq!(queue(&fleet.server, fleet.immich_chat).await.len(), 1);
    let (_, listed) = fleet.server.user.json(fleet.server.user.get("/api/v1/group-connections")).await;
    assert_eq!(listed.as_array().unwrap().len(), 1);

    propose(&fleet.server, &fleet.link, fleet.immich_chat, fleet.yucca_chat).await;
    assert_eq!(queue(&fleet.server, fleet.yucca_chat).await.len(), 1);
}
