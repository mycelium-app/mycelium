use crate::common::Server;
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

async fn create(server: &Server, name: &str) -> Value {
    let (status, body) = server.user.json(server.user.post("/api/v1/nodes/join-tokens").json(&json!({"name": name}))).await;
    assert_eq!(status, 200, "{body}");

    body
}

async fn redeem(server: &Server, token: &str) -> (u16, Value) {
    let anonymous = server.anonymous();

    anonymous.json(anonymous.post("/api/v1/nodes/join").json(&json!({"token": token, "hostname": "lab-node-5d9", "os": "linux", "arch": "x86_64"}))).await
}

#[tokio::test]
async fn a_join_token_makes_a_node_with_the_name_it_was_created_for() {
    let server = Server::start().await;
    let created = create(&server, "lab").await;
    let (status, joined) = redeem(&server, created["token"].as_str().unwrap()).await;

    assert_eq!(status, 200, "{joined}");
    assert_eq!(joined["kind"], "node");
    assert!(joined["token"].as_str().unwrap().starts_with("myc_node_"));

    let id: Uuid = joined["id"].as_str().unwrap().parse().unwrap();
    let (name, owner) = sqlx::query_as::<_, (String, Uuid)>("select name, created_by from nodes where id = $1").bind(id).fetch_one(&server.pool).await.unwrap();
    let me = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();

    assert_eq!((name.as_str(), owner), ("lab", me));
}

#[tokio::test]
async fn the_node_credential_from_a_join_token_connects_a_node() {
    let server = Server::start().await;
    let created = create(&server, "lab").await;
    let (_, joined) = redeem(&server, created["token"].as_str().unwrap()).await;

    let connected = server.try_connect_node(joined["token"].as_str().unwrap()).await;

    assert!(connected.is_ok());
}

#[tokio::test]
async fn joining_with_a_token_is_streamed_as_a_new_node() {
    let server = Server::start().await;
    let mut stream = server.user.stream().await;
    let created = create(&server, "lab").await;
    redeem(&server, created["token"].as_str().unwrap()).await;

    let name = stream
        .until(|message| match message {
            StreamMessage::Node(summary) => Some(summary.name),
            _ => None,
        })
        .await;

    assert_eq!(name, "lab");
}

#[tokio::test]
async fn only_a_hash_of_the_join_token_is_stored() {
    let server = Server::start().await;
    let created = create(&server, "lab").await;
    let token = created["token"].as_str().unwrap();
    let stored = sqlx::query_scalar::<_, Vec<u8>>("select token_hash from node_join_tokens").fetch_one(&server.pool).await.unwrap();

    assert_ne!(stored, token.as_bytes());
    assert_eq!(stored.len(), 32);
}

#[tokio::test]
async fn a_join_token_cannot_be_used_twice() {
    let server = Server::start().await;
    let created = create(&server, "lab").await;
    let token = created["token"].as_str().unwrap();
    redeem(&server, token).await;

    let (status, refused) = redeem(&server, token).await;

    assert_eq!((status, refused["error"].as_str()), (400, Some("join_token_used")));
    assert_eq!(refused["message"], "This join token has already been used. Create a new one in Add an outpost.");
}

#[tokio::test]
async fn an_expired_join_token_is_refused_and_makes_no_node() {
    let server = Server::start().await;
    let created = create(&server, "lab").await;
    sqlx::query("update node_join_tokens set expires_at = now() - interval '1 second'").execute(&server.pool).await.unwrap();

    let (status, refused) = redeem(&server, created["token"].as_str().unwrap()).await;
    let nodes = sqlx::query_scalar::<_, i64>("select count(*) from nodes").fetch_one(&server.pool).await.unwrap();

    assert_eq!((status, refused["error"].as_str(), nodes), (400, Some("join_token_expired"), 0));
}

#[tokio::test]
async fn a_join_token_lasts_an_hour() {
    let server = Server::start().await;
    let created = create(&server, "lab").await;
    let expires = chrono::DateTime::parse_from_rfc3339(created["expires_at"].as_str().unwrap()).unwrap();
    let remaining = expires.signed_duration_since(chrono::Utc::now());

    assert!(remaining > chrono::Duration::minutes(59) && remaining <= chrono::Duration::minutes(60), "{remaining}");
}

#[tokio::test]
async fn an_unknown_join_token_is_refused() {
    let server = Server::start().await;

    let (status, refused) = redeem(&server, "myc_join_nope").await;

    assert_eq!((status, refused["error"].as_str()), (400, Some("join_token_unknown")));
}

#[tokio::test]
async fn a_join_token_for_a_name_already_in_use_is_refused_and_stays_unused() {
    let server = Server::start().await;
    server.join("node", "lab").await;
    let created = create(&server, "lab").await;

    let (status, refused) = redeem(&server, created["token"].as_str().unwrap()).await;
    let consumed = sqlx::query_scalar::<_, bool>("select consumed_at is not null from node_join_tokens").fetch_one(&server.pool).await.unwrap();

    assert_eq!((status, refused["error"].as_str(), consumed), (409, Some("node_name_taken"), false));
}

#[tokio::test]
async fn creating_a_join_token_needs_a_name_and_a_signed_in_person() {
    let server = Server::start().await;
    let (status, body) = server.user.json(server.user.post("/api/v1/nodes/join-tokens").json(&json!({"name": "  "}))).await;
    assert_eq!((status, body["error"].as_str()), (400, Some("name_required")));

    let anonymous = server.anonymous();
    let (status, _) = anonymous.json(anonymous.post("/api/v1/nodes/join-tokens").json(&json!({"name": "lab"}))).await;
    assert_eq!(status, 401);
}
