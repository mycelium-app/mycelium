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

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_string()
}

async fn project_in(fleet: &Fleet, name: &str, group: Option<Uuid>) -> Uuid {
    sqlx::query_scalar::<_, Uuid>("insert into projects (owner, node_id, name, path, group_id) values ((select owner from conversations where id = $1), $2, $3, $4, $5) returning id")
        .bind(fleet.yucca_chat)
        .bind(fleet.link.id)
        .bind(name)
        .bind(format!("/work/{name}"))
        .bind(group)
        .fetch_one(&fleet.server.pool)
        .await
        .unwrap()
}

fn start_arguments(group: &str, project: &str) -> Value {
    json!({"group": group, "project": project, "title": "Billing follow-up", "prompt": "Please add the export column", "reason": "The release needs it"})
}

#[tokio::test]
async fn start_conversation_is_not_listed_until_the_experiment_is_on() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let listed = |tools: Value| tools["result"]["tools"].as_array().unwrap().iter().any(|tool| tool["name"] == "start_conversation");
    let list = |server: &Server| {
        let anonymous = server.anonymous();
        let request = anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {}", link.token)).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
        async move { anonymous.json(request).await.1 }
    };

    assert!(!listed(list(&server).await));

    turn_on(&server).await;
    assert!(listed(list(&server).await));
}

#[tokio::test]
async fn starting_a_conversation_in_a_connected_groups_project_queues_the_prompt_from_the_caller() {
    let mut fleet = fleet().await;
    connect(&fleet.server, fleet.yucca, fleet.immich).await;
    project_in(&fleet, "billing", Some(fleet.immich)).await;

    let started = call(&fleet.server, &fleet.link, fleet.yucca_chat, "start_conversation", start_arguments("immich", "billing")).await;

    assert_eq!(started["isError"], false, "{started}");
    assert!(text(&started).starts_with("Started Billing follow-up ("), "{started}");
    assert!(text(&started).contains(") in Immich on laptop."), "{started}");
    let new_chat: Uuid = text(&started).split('(').nth(1).unwrap().split(')').next().unwrap().parse().unwrap();
    fleet.link.expect_start_session().await;
    fleet.link.start_session(new_chat, "acp-new").await;
    let (_, prompt) = fleet.link.expect_prompt().await;
    assert_eq!(prompt, "From Plan the release:\n\nPlease add the export column");
}

#[tokio::test]
async fn starting_in_an_unconnected_group_starts_nothing_and_says_how_to_connect() {
    let fleet = fleet().await;
    project_in(&fleet, "billing", Some(fleet.immich)).await;

    let refused = call(&fleet.server, &fleet.link, fleet.yucca_chat, "start_conversation", start_arguments("Immich", "billing")).await;

    assert_eq!(refused["isError"], true);
    assert_eq!(text(&refused), "Nothing was started. The yucca and Immich groups are not connected. Ask the person to connect them in Settings, Agent connections.");
    let (_, conversations) = fleet.server.user.json(fleet.server.user.get("/api/v1/conversations")).await;
    assert_eq!(conversations.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn starting_from_a_conversation_with_no_group_says_to_put_it_in_one() {
    let mut fleet = fleet().await;
    let loose = working_conversation(&fleet.server, &mut fleet.link, "Loose chat", "/elsewhere").await;
    project_in(&fleet, "billing", Some(fleet.immich)).await;

    let refused = call(&fleet.server, &fleet.link, loose, "start_conversation", start_arguments("Immich", "billing")).await;

    assert_eq!(text(&refused), "Nothing was started. This conversation is not in a group, so it cannot start one in Immich. Ask the person to put it in a group and connect that group to Immich in Settings, Agent connections.");
}

#[tokio::test]
async fn connecting_groups_for_two_conversations_in_the_same_group_says_there_is_nothing_to_connect() {
    let mut fleet = fleet().await;
    let loose = working_conversation(&fleet.server, &mut fleet.link, "Loose chat", "/elsewhere").await;
    propose(&fleet.server, &fleet.link, loose, fleet.immich_chat).await;
    sqlx::query("update conversations set cwd = '/work/immich/other' where id = $1").bind(loose).execute(&fleet.server.pool).await.unwrap();
    let waiting = proposals(&fleet.server).await;
    let id = waiting[0]["id"].as_str().unwrap();

    let (status, refused) = fleet.server.user.json(fleet.server.user.post(&format!("/api/v1/message-proposals/{id}/connect-groups"))).await;

    assert_eq!((status, refused["error"].clone()), (409, json!("connection_same_group")));
    assert!(refused["message"].as_str().unwrap().contains("Approve the message without connecting groups"));
}

#[tokio::test]
async fn starting_in_a_project_outside_the_named_group_is_refused() {
    let fleet = fleet().await;
    connect(&fleet.server, fleet.yucca, fleet.immich).await;
    project_in(&fleet, "ungrouped", None).await;

    let refused = call(&fleet.server, &fleet.link, fleet.yucca_chat, "start_conversation", start_arguments("Immich", "ungrouped")).await;

    assert_eq!(refused["isError"], true);
    assert_eq!(text(&refused), "Nothing was started. The project ungrouped is not in the Immich group.");
}

#[tokio::test]
async fn a_started_conversation_can_reply_to_its_starter_without_approval() {
    let mut fleet = fleet().await;
    connect(&fleet.server, fleet.yucca, fleet.immich).await;
    project_in(&fleet, "billing", Some(fleet.immich)).await;
    let started = text(&call(&fleet.server, &fleet.link, fleet.yucca_chat, "start_conversation", start_arguments("Immich", "billing")).await);
    let new_chat: Uuid = started.split('(').nth(1).unwrap().split(')').next().unwrap().parse().unwrap();
    fleet.link.expect_start_session().await;

    let (_, connections) = fleet.server.user.json(fleet.server.user.get("/api/v1/conversation-connections")).await;
    assert_eq!(connections.as_array().unwrap().len(), 1);

    let replied = call(&fleet.server, &fleet.link, new_chat, "send_to_conversation", json!({"conversation": fleet.yucca_chat.to_string(), "message": "Column added", "reason": "You asked for it"})).await;

    assert!(text(&replied).starts_with("Sent to Plan the release"), "{replied}");
    assert_eq!(queue(&fleet.server, fleet.yucca_chat).await.len(), 1);
}
