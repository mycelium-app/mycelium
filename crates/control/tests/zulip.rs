mod common;

#[path = "common/fake_zulip.rs"]
mod fake_zulip;

use common::{Server, serve};
use fake_zulip::{ACCOUNT, API_KEY, fake_zulip};
use serde_json::{Value, json};
use uuid::Uuid;

async fn mcp(server: &Server, token: &str, body: Value) -> Value {
    let anonymous = server.anonymous();
    let (status, body) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&body)).await;
    assert_eq!(status, 200, "{body}");

    body
}

fn call(id: u64, tool: &str, arguments: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": tool, "arguments": arguments}})
}

async fn open_review(server: &Server, conversation: Uuid) -> Value {
    common::eventually("a review request", || async {
        let events = server.events(conversation).await;
        let open = events.iter().find(|event| event["kind"] == "review_request" && !events.iter().any(|other| other["kind"] == "review_resolved" && other["payload"]["request_id"] == event["payload"]["request_id"]));

        open.map(|event| event["payload"].clone())
    })
    .await
}

async fn answer_review(server: &Server, conversation: Uuid, request: &Value, decision: &str, removed: Vec<usize>) -> u16 {
    let body = json!({"request_id": request["request_id"], "decision": decision, "removed": removed});
    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{conversation}/review")).json(&body)).await;

    status
}

async fn reviewed_call(server: &Server, token: &str, body: Value, decision: &str, removed: Vec<usize>) -> (Value, Uuid, Value) {
    let conversation = server.node_conversation(token).await;
    let (reply, shown) = tokio::join!(server.agent_mcp(token, conversation, body), async {
        let permission = server.open_request(conversation).await;
        server.answer(conversation, &permission, "allow_once").await;
        let review = open_review(server, conversation).await;
        let (_, shown) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/reviews/{}", review["request_id"].as_str().unwrap()))).await;

        assert_eq!(answer_review(server, conversation, &review, decision, removed).await, 204);

        shown
    });

    (reply, conversation, shown)
}

async fn add_zulip(server: &Server, base: &str, name: &str) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": name, "kind": "zulip", "url": base, "account": ACCOUNT, "bearer_token": API_KEY}))).await
}

#[tokio::test]
async fn a_zulip_integration_is_added_listed_renamed_and_removed_without_returning_the_api_key() {
    let server = Server::start().await;
    let (router, zulip) = fake_zulip();
    let base = serve(router).await;
    let post = |body: Value| server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&body));

    let (status, added) = add_zulip(&server, &base, "Zulip").await;
    let stored = server.user.get("/api/v1/chat-tool-servers").send().await.unwrap().text().await.unwrap();
    let (_, wrong) = post(json!({"name": "Other", "kind": "zulip", "url": base, "account": ACCOUNT, "bearer_token": "wrong"})).await;
    let (_, no_account) = post(json!({"name": "Other", "kind": "zulip", "url": base, "bearer_token": API_KEY})).await;
    let (_, no_key) = post(json!({"name": "Other", "kind": "zulip", "url": base, "account": ACCOUNT})).await;
    let (_, unknown) = post(json!({"name": "Other", "kind": "carrier-pigeon", "url": base})).await;
    let id = added["id"].as_str().unwrap();
    let (_, renamed) = server.user.json(server.user.patch(&format!("/api/v1/chat-tool-servers/{id}")).json(&json!({"name": "Work chat", "always_allow": true}))).await;
    let (removed, _) = server.user.json(server.user.delete(&format!("/api/v1/chat-tool-servers/{id}"))).await;
    let (_, after) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;

    assert_eq!(status, 201);
    assert_eq!((added["kind"].clone(), added["account"].clone(), added["token_hint"].clone()), (json!("zulip"), json!(ACCOUNT), json!("••••cdef")));
    let names: Vec<&str> = added["tools"].as_array().unwrap().iter().map(|tool| tool["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["list_channels", "get_messages", "search_messages", "send_message"]);
    assert!(!stored.contains(API_KEY));
    assert_eq!((wrong["error"].clone(), no_account["error"].clone(), no_key["error"].clone(), unknown["error"].clone()), (json!("tool_server_failed"), json!("account_required"), json!("api_key_required"), json!("kind_invalid")));
    assert!(wrong["message"].as_str().unwrap().contains("API key"));
    assert_eq!((renamed["name"].clone(), renamed["always_allow"].clone()), (json!("Work chat"), json!(true)));
    assert_eq!((removed, after), (204, json!([])));
    assert!(zulip.paths().iter().all(|path| path == "/api/v1/users/me"));
}

#[tokio::test]
async fn zulip_tools_are_listed_under_the_integrations_name_and_calls_reach_zulip_with_the_account_credentials() {
    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (router, zulip) = fake_zulip();
    let base = serve(router).await;
    add_zulip(&server, &base, "Zulip").await;

    let listed = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
    let names: Vec<&str> = listed["result"]["tools"].as_array().unwrap().iter().map(|tool| tool["name"].as_str().unwrap()).collect();
    let send = listed["result"]["tools"].as_array().unwrap().iter().find(|tool| tool["name"] == "zulip__send_message").unwrap();

    assert!(["zulip__list_channels", "zulip__get_messages", "zulip__search_messages", "zulip__send_message"].iter().all(|name| names.contains(name)));
    assert!(send["description"].as_str().unwrap().contains("needs the person's approval"));

    let (channels, _, _) = reviewed_call(&server, &token, call(2, "zulip__list_channels", json!({})), "share", vec![]).await;
    let channels: Value = serde_json::from_str(channels["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(channels, json!([{"id": 7, "name": "general", "description": "Everything"}]));

    let (messages, _, _) = reviewed_call(&server, &token, call(3, "zulip__get_messages", json!({"channel": "general", "topic": "lunch", "limit": 5})), "share", vec![]).await;
    let messages: Value = serde_json::from_str(messages["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(messages[0]["content"], "Noon?");
    assert_eq!(messages[0]["place"], json!({"channel": "general", "topic": "lunch"}));

    reviewed_call(&server, &token, call(4, "zulip__search_messages", json!({"query": "lunch plans"})), "share", vec![]).await;
    let (sent, _, _) = reviewed_call(&server, &token, call(5, "zulip__send_message", json!({"channel": "general", "topic": "lunch", "content": "Yes, noon."})), "share", vec![]).await;
    let (direct, _, _) = reviewed_call(&server, &token, call(6, "zulip__send_message", json!({"direct_to": ["grace@example.com"], "content": "Hi"})), "share", vec![]).await;
    let neither = server.approved_agent_mcp(&token, json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": "zulip__send_message", "arguments": {"content": "Hi"}}})).await;

    let seen = zulip.seen.lock().unwrap().clone();
    let fetched = seen.iter().filter(|(_, path, _)| path == "/api/v1/messages").map(|(_, _, body)| body.clone()).collect::<Vec<_>>();
    assert!(fetched[0].contains("num_before=5") && fetched[0].contains("anchor=newest") && fetched[0].contains("%22operator%22%3A%22stream%22") && fetched[0].contains("%22operand%22%3A%22lunch%22"));
    assert!(fetched[1].contains("%22operator%22%3A%22search%22") && fetched[1].contains("lunch+plans"));
    assert_eq!(zulip.sent(), ["type=stream&to=general&topic=lunch&content=Yes%2C+noon.", "type=private&to=%5B%22grace%40example.com%22%5D&content=Hi"]);
    assert_eq!(sent["result"]["isError"], false);
    assert_eq!(direct["result"]["isError"], false);
    assert_eq!(neither["result"]["isError"], true);
    assert_eq!(zulip.sent().len(), 2);
}

#[tokio::test]
async fn a_switched_off_zulip_integration_refuses_calls_and_stays_out_of_the_tool_list() {
    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (router, zulip) = fake_zulip();
    let base = serve(router).await;
    let (_, added) = add_zulip(&server, &base, "Zulip").await;
    server.user.json(server.user.patch(&format!("/api/v1/chat-tool-servers/{}", added["id"].as_str().unwrap())).json(&json!({"enabled": false}))).await;

    let listed = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
    let called = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "zulip__list_channels", "arguments": {}}})).await;

    assert!(listed["result"]["tools"].as_array().unwrap().iter().all(|tool| !tool["name"].as_str().unwrap().starts_with("zulip__")));
    assert_eq!(called["result"]["isError"], true);
    assert_eq!(zulip.paths(), ["/api/v1/users/me"]);
}

#[tokio::test]
async fn every_zulip_read_from_a_workspace_waits_for_approval_and_a_review() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (router, zulip) = fake_zulip();
    let base = serve(router).await;
    add_zulip(&server, &base, "Zulip").await;
    let conversation = server.node_conversation(&node.token).await;
    let body = call(1, "zulip__get_messages", json!({"channel": "general", "topic": "lunch"}));

    let refused = mcp(&server, &node.token, body.clone()).await;
    assert_eq!(refused["result"]["isError"], true);
    assert_eq!(zulip.paths(), ["/api/v1/users/me"]);

    let (reply, ()) = tokio::join!(server.agent_mcp(&node.token, conversation, body), async {
        let permission = server.open_request(conversation).await;
        assert_eq!(zulip.paths(), ["/api/v1/users/me"]);
        server.answer(conversation, &permission, "allow_once").await;
        let review = open_review(&server, conversation).await;

        assert_eq!(server.conversation(conversation).await["status"], "input");
        assert_eq!(answer_review(&server, conversation, &review, "share", vec![]).await, 204);
    });
    let messages: Value = serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let (_, calls) = server.user.json(server.user.get("/api/v1/integration-calls?review=shared")).await;

    assert_eq!(reply["result"]["isError"], false);
    assert_eq!(messages[0]["content"], "Noon?");
    assert_eq!(calls.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn withheld_zulip_result_is_never_stored_in_events() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (router, _) = fake_zulip();
    let base = serve(router).await;
    add_zulip(&server, &base, "Zulip").await;
    let body = call(1, "call_integration_tool", json!({"name": "zulip__get_messages", "arguments": {"channel": "general", "topic": "lunch"}}));

    let (reply, conversation, shown) = reviewed_call(&server, &node.token, body, "withhold", vec![]).await;
    let events = server.events(conversation).await;
    let stored = serde_json::to_string(&events).unwrap();
    let resolved = events.iter().find(|event| event["kind"] == "review_resolved").unwrap();

    assert_eq!(shown["items"].as_array().unwrap().len(), 2);
    assert!(reply["result"]["content"][0]["text"].as_str().unwrap().contains("withheld"));
    assert!(!stored.contains("secret-recipe") && !stored.contains("Noon?"));
    assert_eq!(resolved["payload"]["decision"], "withhold");
}

#[tokio::test]
async fn removed_items_reach_the_agent_with_a_note() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (router, _) = fake_zulip();
    let base = serve(router).await;
    add_zulip(&server, &base, "Zulip").await;

    let (reply, conversation, _) = reviewed_call(&server, &node.token, call(1, "zulip__get_messages", json!({"channel": "general", "topic": "lunch"})), "remove", vec![1]).await;
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    let stored = serde_json::to_string(&server.events(conversation).await).unwrap();

    assert!(text.contains("Noon?") && !text.contains("secret-recipe"));
    assert!(text.ends_with("[The person removed 1 of 2 items before you saw them.]"));
    assert!(!stored.contains("Noon?"));
}

#[tokio::test]
async fn review_items_are_gone_once_answered() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (router, _) = fake_zulip();
    let base = serve(router).await;
    add_zulip(&server, &base, "Zulip").await;
    let conversation = server.node_conversation(&node.token).await;
    let body = call(1, "zulip__get_messages", json!({"channel": "general", "topic": "lunch"}));

    let (_, request) = tokio::join!(server.agent_mcp(&node.token, conversation, body), async {
        let permission = server.open_request(conversation).await;
        server.answer(conversation, &permission, "allow_once").await;
        let review = open_review(&server, conversation).await;
        let nothing = answer_review(&server, conversation, &review, "remove", vec![]).await;
        let outside = answer_review(&server, conversation, &review, "remove", vec![9]).await;

        assert_eq!((nothing, outside), (400, 400));
        assert_eq!(answer_review(&server, conversation, &review, "share", vec![]).await, 204);

        review
    });
    let path = format!("/api/v1/conversations/{conversation}/reviews/{}", request["request_id"].as_str().unwrap());
    let (gone, error) = server.user.json(server.user.get(&path)).await;
    let again = answer_review(&server, conversation, &request, "share", vec![]).await;

    assert_eq!((gone, again), (404, 409));
    assert_eq!(error["message"], "This result is no longer waiting for review.");
}
