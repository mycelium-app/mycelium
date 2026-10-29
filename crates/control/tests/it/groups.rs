use crate::common::Server;
use serde_json::json;

#[tokio::test]
async fn a_group_keeps_the_icon_it_was_given_and_loses_it_when_set_back_to_the_default() {
    let server = Server::start().await;
    let (status, created) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": "work", "icon": "briefcase"}))).await;
    let id = created["id"].as_str().unwrap().to_string();
    let (_, renamed) = server.user.json(server.user.patch(&format!("/api/v1/groups/{id}")).json(&json!({"name": "work", "icon": "group"}))).await;

    assert_eq!((status, created["icon"].clone()), (201, json!("briefcase")));
    assert_eq!(renamed["icon"], json!(null));
}

#[tokio::test]
async fn a_group_icon_outside_the_fixed_set_is_refused() {
    let server = Server::start().await;
    let (status, body) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": "work", "icon": "skull"}))).await;

    assert_eq!((status, body["error"].clone()), (400, json!("icon_invalid")));
}
