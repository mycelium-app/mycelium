use crate::common::Server;
use crate::watches::begun;
use serde_json::{Value, json};

#[tokio::test]
async fn every_event_says_when_the_control_plane_received_it() {
    let server = Server::start().await;
    let mut link = server.online_node("atlas").await;
    let conversation = begun(&server, &mut link, "Time the tool calls").await;
    let user = &server.user;

    let (_, events) = user.json(user.get(&format!("/api/v1/conversations/{conversation}/events?after=0&limit=50"))).await;
    let events = events.as_array().cloned().unwrap_or_default();

    assert!(!events.is_empty());
    assert!(events.iter().all(|event| event["created_at"].as_str().is_some_and(|at| chrono::DateTime::parse_from_rfc3339(at).is_ok())), "{events:?}");
}

#[tokio::test]
async fn tools_that_only_read_say_so_and_tools_that_act_do_not() {
    let server = Server::start().await;
    let mut link = server.online_node("atlas").await;
    let conversation = begun(&server, &mut link, "List the tools").await;
    let tools = server.agent_mcp(&link.token, conversation, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
    let annotation = |name: &str| -> Value {
        tools["result"]["tools"].as_array().unwrap().iter().find(|tool| tool["name"] == name).map(|tool| tool["annotations"]["readOnlyHint"].clone()).unwrap_or(Value::Null)
    };

    for reading in ["read_conversation", "find_conversations", "read_traces", "read_client_events", "recall", "list_watches", "get_instructions"] {
        assert_eq!(annotation(reading), json!(true), "{reading}");
    }

    for acting in ["send_to_conversation", "propose_instructions_edit", "remember", "start_conversation"] {
        assert_eq!(annotation(acting), Value::Null, "{acting}");
    }
}
