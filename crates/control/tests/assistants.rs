mod common;

use common::{Client, Server};
use serde_json::{Value, json};

async fn enable(client: &Client) {
    let flags = json!({"tasks": false, "workflows": false, "instructions": false, "details": false, "subagents": false, "walkthrough": false, "move_chats": false, "groups": true, "continue_elsewhere": false, "assistants": true});
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
