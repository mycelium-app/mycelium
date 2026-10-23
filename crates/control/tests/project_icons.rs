mod common;

use common::{Client, Server};
use serde_json::{Value, json};
use uuid::Uuid;

const PNG: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

async fn project(server: &Server) -> Uuid {
    let link = server.online_node("laptop").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();

    sqlx::query_scalar::<_, Uuid>("insert into projects (owner, node_id, name, path) values ($1, $2, 'app', '/work/app') returning id").bind(owner).bind(link.id).fetch_one(&server.pool).await.unwrap()
}

async fn upload_logo(client: &Client, project: Uuid, media_type: &str, bytes: Vec<u8>) -> (u16, Value) {
    client.json(client.post(&format!("/api/v1/projects/{project}/avatar")).header("content-type", media_type).body(bytes)).await
}

async fn listed(server: &Server) -> Value {
    let (_, projects) = server.user.json(server.user.get("/api/v1/projects")).await;

    projects[0].clone()
}

#[tokio::test]
async fn a_project_is_given_a_named_icon() {
    let server = Server::start().await;
    let id = project(&server).await;

    let (status, patched) = server.user.json(server.user.patch(&format!("/api/v1/projects/{id}")).json(&json!({"icon": "rocket"}))).await;

    assert_eq!((status, patched["icon"].clone(), patched["avatar_sha256"].clone()), (200, json!("rocket"), Value::Null));
    assert_eq!(listed(&server).await["icon"], "rocket");
}

#[tokio::test]
async fn an_icon_name_outside_the_allowed_characters_is_refused() {
    let server = Server::start().await;
    let id = project(&server).await;

    let (status, body) = server.user.json(server.user.patch(&format!("/api/v1/projects/{id}")).json(&json!({"icon": "../etc"}))).await;

    assert_eq!((status, body["error"].clone()), (400, json!("icon_invalid")), "{body}");
}

#[tokio::test]
async fn an_uploaded_logo_is_read_back_and_replaces_the_named_icon() {
    let server = Server::start().await;
    let id = project(&server).await;
    server.user.json(server.user.patch(&format!("/api/v1/projects/{id}")).json(&json!({"icon": "rocket"}))).await;

    let (status, uploaded) = upload_logo(&server.user, id, "image/png", PNG.to_vec()).await;
    let sha256 = uploaded["avatar_sha256"].as_str().unwrap();
    let served = server.user.get(&format!("/api/v1/attachments/{sha256}")).send().await.unwrap();

    assert_eq!((status, uploaded["icon"].clone()), (200, Value::Null));
    assert_eq!(served.status().as_u16(), 200);
    assert_eq!(served.bytes().await.unwrap().as_ref(), PNG);
}

#[tokio::test]
async fn choosing_a_named_icon_removes_the_uploaded_logo() {
    let server = Server::start().await;
    let id = project(&server).await;
    upload_logo(&server.user, id, "image/svg+xml", b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec()).await;

    let (_, patched) = server.user.json(server.user.patch(&format!("/api/v1/projects/{id}")).json(&json!({"icon": "leaf"}))).await;

    assert_eq!((patched["icon"].clone(), patched["avatar_sha256"].clone()), (json!("leaf"), Value::Null));
}

#[tokio::test]
async fn clearing_removes_both_the_icon_and_the_logo() {
    let server = Server::start().await;
    let id = project(&server).await;
    upload_logo(&server.user, id, "image/png", PNG.to_vec()).await;

    let (status, cleared) = server.user.json(server.user.patch(&format!("/api/v1/projects/{id}")).json(&json!({"clear_icon": true}))).await;

    assert_eq!((status, cleared["icon"].clone(), cleared["avatar_sha256"].clone()), (200, Value::Null, Value::Null));
}

#[tokio::test]
async fn changing_the_icon_leaves_the_group_alone() {
    let server = Server::start().await;
    let id = project(&server).await;
    let (_, group) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": "work"}))).await;
    server.user.json(server.user.patch(&format!("/api/v1/projects/{id}")).json(&json!({"group_id": group["id"]}))).await;

    let (_, patched) = server.user.json(server.user.patch(&format!("/api/v1/projects/{id}")).json(&json!({"icon": "star"}))).await;

    assert_eq!(patched["group_id"], group["id"]);
}

#[tokio::test]
async fn a_logo_that_is_not_an_image_is_refused_with_a_message() {
    let server = Server::start().await;
    let id = project(&server).await;

    let (status, body) = upload_logo(&server.user, id, "text/plain", b"hello".to_vec()).await;

    assert_eq!((status, body["error"].clone()), (400, json!("avatar_not_image")), "{body}");
    assert!(body["message"].as_str().unwrap().contains("PNG, JPEG, WebP or SVG"));
    assert_eq!(listed(&server).await["avatar_sha256"], Value::Null);
}

#[tokio::test]
async fn a_logo_over_256_kib_is_refused() {
    let server = Server::start().await;
    let id = project(&server).await;

    let (status, body) = upload_logo(&server.user, id, "image/png", vec![0; 256 * 1024 + 1]).await;

    assert_eq!((status, body["error"].clone()), (413, json!("avatar_too_large")), "{body}");
}

#[tokio::test]
async fn another_person_cannot_change_the_icon_or_logo_of_someone_elses_project() {
    let server = Server::start().await;
    let id = project(&server).await;
    let other = server.second_user("other@example.com").await;

    let (patched, _) = other.json(other.patch(&format!("/api/v1/projects/{id}")).json(&json!({"icon": "rocket"}))).await;
    let (uploaded, _) = upload_logo(&other, id, "image/png", PNG.to_vec()).await;

    assert_eq!((patched, uploaded), (404, 404));
    assert_eq!(listed(&server).await["icon"], Value::Null);
}
