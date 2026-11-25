use crate::common::{Client, Server};
use crate::snapshots::browser;
use base64::Engine;
use serde_json::{Value, json};
use uuid::Uuid;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

fn png() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();

    bytes.extend(4u32.to_be_bytes());
    bytes.extend(3u32.to_be_bytes());
    bytes.extend([8, 6, 0, 0, 0]);

    bytes
}

async fn created(client: &Client, body: Value) -> String {
    let (status, artifact) = client.json(client.post("/api/v1/artifacts").json(&body)).await;
    assert_eq!(status, 201, "{artifact}");

    artifact["id"].as_str().unwrap().to_string()
}

async fn shared_with(client: &Client, artifact: &str, conversation: Uuid) {
    let (status, body) = client.json(client.post(&format!("/api/v1/artifacts/{artifact}/links")).json(&json!({"conversation_id": conversation, "permission": "view"}))).await;
    assert_eq!(status, 200, "{body}");
}

async fn image(client: &Client, conversation: Uuid, reference: &str) -> (u16, String, Vec<u8>) {
    let response = client.get(&format!("/api/v1/conversations/{conversation}/remote-images")).query(&[("url", reference)]).send().await.unwrap();
    let status = response.status().as_u16();
    let media_type = response.headers().get("content-type").map(|value| value.to_str().unwrap().to_string()).unwrap_or_default();

    (status, media_type, response.bytes().await.unwrap().to_vec())
}

async fn conversation(server: &Server) -> Uuid {
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("hello")).await;
    link.expect_start_session().await;

    conversation.id
}

#[tokio::test]
async fn an_image_artefact_the_conversation_can_view_is_served_as_the_image() {
    let server = Server::start().await;
    let conversation = conversation(&server).await;
    let encoded = base64::engine::general_purpose::STANDARD.encode(png());
    let artifact = created(&server.user, json!({"kind": "file", "title": "Variants", "content_base64": encoded, "media_type": "image/png"})).await;
    shared_with(&server.user, &artifact, conversation).await;

    let (status, media_type, bytes) = image(&server.user, conversation, &format!("artifact:{artifact}")).await;

    assert_eq!((status, media_type.as_str(), bytes), (200, "image/png", png()));
}

#[tokio::test]
async fn an_artefact_the_conversation_cannot_view_is_not_shown_and_says_why() {
    let server = Server::start().await;
    let conversation = conversation(&server).await;
    let encoded = base64::engine::general_purpose::STANDARD.encode(png());
    let artifact = created(&server.user, json!({"kind": "file", "title": "Private", "content_base64": encoded, "media_type": "image/png"})).await;

    let (status, _, body) = image(&server.user, conversation, &format!("artifact:{artifact}")).await;
    let refused: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!((status, refused["error"].clone(), refused["message"].clone()), (403, json!("image_artifact_not_viewable"), json!("This conversation cannot view that artefact.")));
}

#[tokio::test]
async fn one_artboard_of_a_design_is_drawn_as_a_png() {
    let Some(browser) = browser().await else {
        return;
    };
    let server = Server::start_with(|config| config.browser_url = Some(browser.url.clone())).await;
    let conversation = conversation(&server).await;
    let design = json!({"pages": [{"id": "p1", "name": "Variants", "items": [
        {"type": "artboard", "id": "a1", "name": "Variant A", "x": 0, "y": 0, "w": 320, "h": 200, "html": "<html><body style=\"background:#123456\">A</body></html>"},
        {"type": "artboard", "id": "a2", "name": "Variant B", "x": 400, "y": 0, "w": 300, "h": 100, "html": "<html><body style=\"background:#654321\">B</body></html>"}
    ]}]});
    let artifact = created(&server.user, json!({"kind": "design", "title": "Variants", "content": design.to_string()})).await;
    shared_with(&server.user, &artifact, conversation).await;

    let (status, media_type, bytes) = image(&server.user, conversation, &format!("artifact:{artifact}#Variant B")).await;

    assert_eq!((status, media_type.as_str()), (200, "image/png"));
    assert_eq!(bytes[..8], PNG_SIGNATURE);
    assert_eq!((u32::from_be_bytes(bytes[16..20].try_into().unwrap()), u32::from_be_bytes(bytes[20..24].try_into().unwrap())), (600, 200));
}
