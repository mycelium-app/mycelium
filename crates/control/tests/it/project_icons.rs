use crate::common;
use crate::common::{Client, Server};
use mycelium_proto::stream::StreamMessage;
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
    upload_logo(&server.user, id, "image/png", b"\x89PNG fake".to_vec()).await;

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
    assert!(body["message"].as_str().unwrap().contains("PNG, JPEG or WebP"));
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

#[tokio::test]
async fn an_agent_sets_its_projects_logo_from_a_file_in_its_conversation_folder() {
    use base64::Engine;
    use mycelium_proto::link::{ControlToNode, GitCommand, NodeToControl};
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    let project = sqlx::query_scalar::<_, Uuid>("insert into projects (owner, node_id, name, path) values ($1, $2, 'app', '/work/app') returning id").bind(conversation.owner).bind(link.id).fetch_one(&server.pool).await.unwrap();
    sqlx::query("update conversations set project_id = $2 where id = $1").bind(conversation.id).bind(project).execute(&server.pool).await.unwrap();
    let anonymous = server.anonymous();
    let request = anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {}", link.token)).header("x-mycelium-conversation", conversation.id.to_string()).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"set_project_icon","arguments":{"path":"logo.png"}}}));
    let pending = tokio::spawn(async move { anonymous.json(request).await });
    let (request_id, cwd, command) = loop {
        if let ControlToNode::Git { request_id, cwd, command } = link.next().await {
            break (request_id, cwd, command);
        }
    };
    assert_eq!(cwd, conversation.cwd);
    assert_eq!(command, GitCommand::ReadFile { path: "logo.png".to_string() });
    link.send(NodeToControl::GitResult { request_id, ok: true, output: json!({"data":base64::engine::general_purpose::STANDARD.encode(PNG)}), error: None }).await;
    let (status, response) = pending.await.unwrap();
    assert_eq!(status, 200);
    assert_eq!(response["result"]["isError"], false, "{response}");
    let (_, projects) = server.user.json(server.user.get("/api/v1/projects")).await;
    assert!(projects[0]["avatar_sha256"].is_string());
}

#[tokio::test]
async fn an_agent_without_a_project_cannot_set_a_project_icon() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    let anonymous = server.anonymous();
    let (_, response) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {}", link.token)).header("x-mycelium-conversation", conversation.id.to_string()).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"set_project_icon","arguments":{"path":"logo.png"}}}))).await;
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert!(response["result"]["content"][0]["text"].as_str().unwrap().contains("project"));
}

async fn find_icon_with_node_answer(server: &Server, link: &mut common::NodeLink, project: Uuid, answer: Value) -> Value {
    use mycelium_proto::link::{ControlToNode, GitCommand, NodeToControl};
    sqlx::query("update nodes set version = '0.9.0' where id = $1").bind(link.id).execute(&server.pool).await.unwrap();
    let client = server.user.clone();
    let pending = tokio::spawn(async move { client.json(client.post(&format!("/api/v1/projects/{project}/find-icon"))).await });
    let request_id = loop {
        if let ControlToNode::Git { request_id, command, .. } = link.raw().await {
            assert_eq!(command, GitCommand::FindLogo);
            break request_id;
        }
    };
    link.send(NodeToControl::GitResult { request_id, ok: true, output: answer, error: None }).await;
    let (status, body) = pending.await.unwrap();
    assert_eq!(status, 200, "{body}");

    body
}

fn found(bytes: &[u8]) -> Value {
    use base64::Engine;

    json!({"path": "logo.png", "media_type": "image/png", "data": base64::engine::general_purpose::STANDARD.encode(bytes)})
}

#[tokio::test]
async fn a_logo_found_on_the_node_is_stored_for_a_project_with_no_icon() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    let id = sqlx::query_scalar::<_, Uuid>("insert into projects (owner, node_id, name, path) values ($1, $2, 'app', '/work/app') returning id").bind(owner).bind(link.id).fetch_one(&server.pool).await.unwrap();

    let project = find_icon_with_node_answer(&server, &mut link, id, found(&PNG)).await;
    let sha256 = project["avatar_sha256"].as_str().unwrap();
    let served = server.user.get(&format!("/api/v1/attachments/{sha256}")).send().await.unwrap();

    assert_eq!(served.bytes().await.unwrap().as_ref(), PNG);
}

#[tokio::test]
async fn a_logo_found_on_the_node_does_not_replace_a_persons_own_choice() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    let id = sqlx::query_scalar::<_, Uuid>("insert into projects (owner, node_id, name, path, icon) values ($1, $2, 'app', '/work/app', 'rocket') returning id").bind(owner).bind(link.id).fetch_one(&server.pool).await.unwrap();

    let (status, project) = server.user.json(server.user.post(&format!("/api/v1/projects/{id}/find-icon"))).await;

    assert_eq!((status, project["icon"].clone(), project["avatar_sha256"].clone()), (200, json!("rocket"), Value::Null));
}

#[tokio::test]
async fn a_logo_found_on_the_node_does_not_replace_an_uploaded_logo() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    let id = sqlx::query_scalar::<_, Uuid>("insert into projects (owner, node_id, name, path) values ($1, $2, 'app', '/work/app') returning id").bind(owner).bind(link.id).fetch_one(&server.pool).await.unwrap();
    let (_, uploaded) = upload_logo(&server.user, id, "image/png", PNG.to_vec()).await;

    let (_, project) = server.user.json(server.user.post(&format!("/api/v1/projects/{id}/find-icon"))).await;

    assert_eq!(project["avatar_sha256"], uploaded["avatar_sha256"]);
}

#[tokio::test]
async fn an_automatic_logo_is_replaced_by_a_newer_find_but_a_person_can_still_choose_over_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    let id = sqlx::query_scalar::<_, Uuid>("insert into projects (owner, node_id, name, path) values ($1, $2, 'app', '/work/app') returning id").bind(owner).bind(link.id).fetch_one(&server.pool).await.unwrap();
    let first = find_icon_with_node_answer(&server, &mut link, id, found(&PNG)).await;
    let mut newer = PNG.to_vec();
    newer.push(1);

    let second = find_icon_with_node_answer(&server, &mut link, id, found(&newer)).await;
    let (_, chosen) = server.user.json(server.user.patch(&format!("/api/v1/projects/{id}")).json(&json!({"icon": "leaf"}))).await;

    assert_ne!(first["avatar_sha256"], second["avatar_sha256"]);
    assert_eq!((chosen["icon"].clone(), chosen["avatar_sha256"].clone()), (json!("leaf"), Value::Null));
}

#[tokio::test]
async fn connecting_a_supported_node_finds_a_logo_for_an_existing_project_and_preserves_a_chosen_icon() {
    use mycelium_proto::link::{ControlToNode, GitCommand, NodeToControl};
    let server = Server::start().await;
    let (node_id, token) = server.join("node", "laptop").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    let id = sqlx::query_scalar::<_, Uuid>("insert into projects (owner, node_id, name, path) values ($1, $2, 'app', '/work/app') returning id").bind(owner).bind(node_id).fetch_one(&server.pool).await.unwrap();
    let chosen = sqlx::query_scalar::<_, Uuid>("insert into projects (owner, node_id, name, path, icon) values ($1, $2, 'chosen', '/work/chosen', 'rocket') returning id").bind(owner).bind(node_id).fetch_one(&server.pool).await.unwrap();
    let mut link = server.connect_node(node_id, &token).await;

    link.hello_as("v0.9.0", vec![]).await;

    let request_id = match link.raw().await {
        ControlToNode::Git { request_id, cwd, command } => {
            assert_eq!((cwd.as_str(), command), ("/work/app", GitCommand::FindLogo));
            request_id
        }
        other => panic!("expected logo discovery, got {other:?}"),
    };
    link.send(NodeToControl::GitResult { request_id, ok: true, output: found(&PNG), error: None }).await;
    common::eventually("automatic project logo", || async {
        let avatar = sqlx::query_scalar::<_, Option<String>>("select avatar_sha256 from projects where id = $1").bind(id).fetch_one(&server.pool).await.unwrap();

        avatar
    }).await;

    let icon = sqlx::query_scalar::<_, String>("select icon from projects where id = $1").bind(chosen).fetch_one(&server.pool).await.unwrap();
    assert_eq!(icon, "rocket");
}

#[tokio::test]
async fn finding_a_logo_on_an_older_node_returns_unchanged_without_sending_an_unknown_command() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    let id = sqlx::query_scalar::<_, Uuid>("insert into projects (owner, node_id, name, path) values ($1, $2, 'app', '/work/app') returning id").bind(owner).bind(link.id).fetch_one(&server.pool).await.unwrap();
    sqlx::query("update nodes set version = '0.8.1' where id = $1").bind(link.id).execute(&server.pool).await.unwrap();

    let (status, project) = server.user.json(server.user.post(&format!("/api/v1/projects/{id}/find-icon"))).await;

    assert_eq!(status, 200);
    assert_eq!(project["avatar_sha256"], Value::Null);
}

#[tokio::test]
async fn a_new_logo_is_streamed_so_every_list_showing_the_project_updates() {
    let server = Server::start().await;
    let id = project(&server).await;
    let mut stream = server.user.stream().await;

    upload_logo(&server.user, id, "image/png", PNG.to_vec()).await;
    let streamed = stream.until(|message| match message { StreamMessage::Project(project) if project.id == id => Some(project), _ => None }).await;

    assert!(streamed.avatar_sha256.is_some());
}
