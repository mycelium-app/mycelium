use crate::common::Server;
use crate::watches::begun;
use mycelium_proto::link::NodeToControl;
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn an_idle_stream_is_pinged_so_the_connection_stays_open() {
    let server = Server::start_with(|config| config.stream_heartbeat_interval = Duration::from_millis(100)).await;
    let mut stream = server.user.stream().await;

    assert!(stream.pinged().await);
}

#[tokio::test]
async fn a_stream_that_falls_behind_is_closed_with_a_code_that_says_so() {
    let server = Server::start_with(|config| config.stream_capacity = 4).await;
    let mut link = server.online_node("atlas").await;
    let conversation = begun(&server, &mut link, "Write a long answer").await;
    let mut stream = server.user.stream().await;
    let long_text = "x".repeat(64 * 1024);

    for seq in 1..=300 {
        link.send(NodeToControl::AcpUpdate { conversation_id: conversation, seq, update: json!({"update": {"sessionUpdate": "tool_call", "toolCallId": format!("t{seq}"), "title": "Read a long file", "rawInput": {"text": long_text}}}) }).await;
    }

    link.barrier(conversation, 301).await;

    assert_eq!(stream.close_code().await, Some(4000));
}
