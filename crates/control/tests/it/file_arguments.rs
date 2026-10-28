use axum::Json;
use axum::Router;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use base64::Engine;
use crate::common::{Server, serve};
use mycelium_proto::link::{ControlToNode, GitCommand, NodeToControl};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Remote {
    calls: Mutex<Vec<(String, Value)>>,
}

async fn mcp(State(remote): State<Arc<Remote>>, Json(body): Json<Value>) -> Response {
    if body.get("id").is_none() {
        return StatusCode::ACCEPTED.into_response();
    }

    let result = match body["method"].as_str().unwrap_or_default() {
        "initialize" => json!({"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}, "serverInfo": {"name": "outline", "version": "1"}}),
        "tools/list" => json!({"tools": [
            {"name": "create_document", "description": "Create", "inputSchema": {"type": "object"}},
            {"name": "create_attachment", "description": "Attach", "inputSchema": {"type": "object"}},
        ]}),
        "tools/call" => {
            let params = &body["params"];
            remote.calls.lock().unwrap().push((params["name"].as_str().unwrap_or_default().to_string(), params["arguments"].clone()));

            json!({"content": [{"type": "text", "text": "Created"}]})
        }
        _ => return StatusCode::NOT_FOUND.into_response(),
    };

    Json(json!({"jsonrpc": "2.0", "id": body["id"], "result": result})).into_response()
}

async fn call_with_file(tool: &str, arguments: Value, file: Vec<u8>, reported_size: usize) -> (Value, Vec<(String, Value)>, Vec<Value>) {
    call_with_file_sent(tool, arguments, file, reported_size, false).await
}

async fn call_with_file_sent(tool: &str, arguments: Value, file: Vec<u8>, reported_size: usize, in_chunks: bool) -> (Value, Vec<(String, Value)>, Vec<Value>) {
    let server = Server::start().await;
    let remote = Arc::new(Remote::default());
    let base = serve(Router::new().route("/mcp", post(mcp)).layer(DefaultBodyLimit::max(64 * 1024 * 1024)).with_state(remote.clone())).await;
    let (status, added) = server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Docs", "kind": "outline", "url": base, "bearer_token": "key"}))).await;
    assert_eq!(status, 201, "{added}");

    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("Upload the file")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, &format!("acp-{}", conversation.id)).await;
    link.expect_prompt().await;

    let token = link.token.clone();
    let request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": format!("docs__{tool}"), "arguments": arguments}});
    let (reply, ()) = tokio::join!(
        server.agent_mcp(&token, conversation.id, request),
        async {
            let ControlToNode::Git { request_id, command, .. } = link.next().await else {
                panic!("expected the node to be asked for the file");
            };

            assert!(matches!(command, GitCommand::ReadFile { .. }));
            let data = base64::engine::general_purpose::STANDARD.encode(&file);

            if in_chunks {
                let pieces: Vec<&[u8]> = data.as_bytes().chunks(1024 * 1024).collect();

                for (seq, piece) in pieces.iter().enumerate() {
                    link.send(NodeToControl::GitChunk { request_id, seq: seq as u32, data: String::from_utf8(piece.to_vec()).unwrap() }).await;
                }

                link.send(NodeToControl::GitResult { request_id, ok: true, output: json!({"size": reported_size, "chunks": pieces.len()}), error: None }).await;
            } else {
                link.send(NodeToControl::GitResult { request_id, ok: true, output: json!({"data": data, "size": reported_size}), error: None }).await;
            }
        }
    );
    let events = server.events(conversation.id).await;
    let calls = remote.calls.lock().unwrap().clone();

    (reply, calls, events)
}

#[tokio::test]
async fn a_large_text_file_reaches_a_create_tool_exactly_and_never_enters_the_transcript() {
    let text = format!("# Transcript\n{}\n", "line of the long transcript, repeated\n".repeat(60_000));
    let (reply, calls, events) = call_with_file("create_document", json!({"title": "Transcript", "text": {"file": "transcripts/long.md"}}), text.clone().into_bytes(), text.len()).await;

    assert_eq!(reply["result"]["isError"], false, "{reply}");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1["title"], "Transcript");
    assert_eq!(calls[0].1["text"].as_str().unwrap(), text);
    assert!(!serde_json::to_string(&events).unwrap().contains("line of the long transcript"));
}

#[tokio::test]
async fn an_attachment_tool_receives_the_files_bytes_as_base64() {
    let bytes: Vec<u8> = (0..=255u8).collect();
    let (reply, calls, _) = call_with_file("create_attachment", json!({"name": "all-bytes.bin", "content": {"file": "out/all-bytes.bin"}}), bytes.clone(), bytes.len()).await;

    assert_eq!(reply["result"]["isError"], false, "{reply}");
    assert_eq!(calls[0].1["content"].as_str().unwrap(), base64::engine::general_purpose::STANDARD.encode(&bytes));
}

#[tokio::test]
async fn a_file_bigger_than_one_websocket_frame_arrives_in_chunks_byte_for_byte_without_dropping_the_link() {
    let bytes: Vec<u8> = (0..13_400_000u32).map(|index| (index.wrapping_mul(2_654_435_761) >> 24) as u8).collect();
    let (reply, calls, _) = call_with_file_sent("create_attachment", json!({"name": "big.bin", "content": {"file": "out/big.bin"}}), bytes.clone(), bytes.len(), true).await;

    assert_eq!(reply["result"]["isError"], false, "{reply}");
    assert_eq!(calls[0].1["content"].as_str().unwrap(), base64::engine::general_purpose::STANDARD.encode(&bytes));
}

#[tokio::test]
async fn a_binary_file_given_to_a_text_tool_is_refused_and_nothing_is_sent() {
    let (reply, calls, _) = call_with_file("create_document", json!({"title": "Binary", "text": {"file": "out/image.png"}}), vec![0xff, 0xfe, 0x00, 0x9f], 4).await;

    assert_eq!(reply["result"]["isError"], true);
    assert!(reply["result"]["content"][0]["text"].as_str().unwrap().contains("is not text"));
    assert!(calls.is_empty());
}

#[tokio::test]
async fn a_file_over_the_size_limit_is_refused_and_nothing_is_sent() {
    let (reply, calls, _) = call_with_file("create_document", json!({"title": "Huge", "text": {"file": "out/huge.txt"}}), b"tiny".to_vec(), 26 * 1024 * 1024).await;

    assert_eq!(reply["result"]["isError"], true);
    assert!(reply["result"]["content"][0]["text"].as_str().unwrap().contains("larger than 25 MB"));
    assert!(calls.is_empty());
}
