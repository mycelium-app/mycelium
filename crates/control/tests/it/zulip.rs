use crate::common;
#[path = "common/fake_zulip.rs"]
mod fake_zulip;

use crate::common::{Server, serve};
use fake_zulip::{ACCOUNT, API_KEY, DAY, START, fake_zulip};
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

async fn shown(server: &Server, conversation: Uuid, request: &Value) -> Value {
    let (_, shown) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/responses/{}", request["request_id"].as_str().unwrap()))).await;

    shown
}

async fn send_response(server: &Server, conversation: Uuid, request: &Value, send: Value) -> u16 {
    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/conversations/{conversation}/responses/{}", request["request_id"].as_str().unwrap())).json(&json!({"send": send}))).await;

    status
}

async fn edited_call(server: &Server, token: &str, body: Value, edit: impl FnOnce(&Value) -> Value) -> (Value, Uuid, Value) {
    let conversation = server.node_conversation(token).await;
    let (reply, seen) = tokio::join!(server.agent_mcp(token, conversation, body), async {
        let permission = server.open_request(conversation).await;
        server.answer(conversation, &permission, "approve_edit").await;
        let held = open_review(server, conversation).await;
        let seen = shown(server, conversation, &held).await;

        assert_eq!(send_response(server, conversation, &held, edit(&seen["value"])).await, 204);

        seen
    });

    (reply, conversation, seen)
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
    let (_, renamed) = server.user.json(server.user.patch(&format!("/api/v1/chat-tool-servers/{id}")).json(&json!({"name": "Work chat"}))).await;
    let (removed, _) = server.user.json(server.user.delete(&format!("/api/v1/chat-tool-servers/{id}"))).await;
    let (_, after) = server.user.json(server.user.get("/api/v1/chat-tool-servers")).await;

    assert_eq!(status, 201);
    assert_eq!((added["kind"].clone(), added["account"].clone(), added["token_hint"].clone()), (json!("zulip"), json!(ACCOUNT), json!("••••cdef")));
    let names: Vec<&str> = added["tools"].as_array().unwrap().iter().map(|tool| tool["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["list_channels", "get_messages", "search_messages", "send_message"]);
    assert!(!stored.contains(API_KEY));
    assert_eq!((wrong["error"].clone(), no_account["error"].clone(), no_key["error"].clone(), unknown["error"].clone()), (json!("tool_server_failed"), json!("account_required"), json!("api_key_required"), json!("kind_invalid")));
    assert!(wrong["message"].as_str().unwrap().contains("API key"));
    assert_eq!((renamed["name"].clone(), renamed["policy"]["preset"].clone()), (json!("Work chat"), json!("ask_all_edit")));
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

    let (channels, _, _) = edited_call(&server, &token, call(2, "zulip__list_channels", json!({})), Value::clone).await;
    let channels: Value = serde_json::from_str(channels["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(channels, json!([{"id": 7, "name": "general", "description": "Everything"}]));

    let (messages, _, _) = edited_call(&server, &token, call(3, "zulip__get_messages", json!({"channel": "general", "topic": "lunch", "before": 5})), Value::clone).await;
    let messages: Value = serde_json::from_str(messages["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(messages["messages"][0]["content"], "Noon?");
    assert_eq!(messages["messages"][0]["place"], json!({"channel": "general", "topic": "lunch"}));

    edited_call(&server, &token, call(4, "zulip__search_messages", json!({"query": "lunch plans"})), Value::clone).await;
    let (sent, _, _) = edited_call(&server, &token, call(5, "zulip__send_message", json!({"channel": "general", "topic": "lunch", "content": "Yes, noon."})), Value::clone).await;
    let (direct, _, _) = edited_call(&server, &token, call(6, "zulip__send_message", json!({"direct_to": ["grace@example.com"], "content": "Hi"})), Value::clone).await;
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
async fn every_zulip_read_from_a_workspace_waits_for_approval_and_an_edit() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (router, zulip) = fake_zulip();
    let base = serve(router).await;
    add_zulip(&server, &base, "Zulip").await;
    let conversation = server.node_conversation(&node.token).await;
    let body = call(1, "zulip__get_messages", json!({"channel": "general", "topic": "lunch"}));

    let (reply, ()) = tokio::join!(server.agent_mcp(&node.token, conversation, body), async {
        let permission = server.open_request(conversation).await;
        assert_eq!(zulip.paths(), ["/api/v1/users/me"]);
        server.answer(conversation, &permission, "approve_edit").await;
        let held = open_review(&server, conversation).await;

        assert_eq!(server.conversation(conversation).await["status"], "input");
        assert_eq!(send_response(&server, conversation, &held, shown(&server, conversation, &held).await["value"].clone()).await, 204);
    });
    let messages: Value = serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let (_, calls) = server.user.json(server.user.get("/api/v1/integration-calls?review=shared")).await;

    assert_eq!(reply["result"]["isError"], false);
    assert_eq!(messages["messages"][0]["content"], "Noon?");
    assert_eq!(calls.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_zulip_result_sent_as_nothing_is_never_stored_in_events() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (router, _) = fake_zulip();
    let base = serve(router).await;
    add_zulip(&server, &base, "Zulip").await;
    let body = call(1, "call_integration_tool", json!({"name": "zulip__get_messages", "arguments": {"channel": "general", "topic": "lunch"}}));

    let (reply, conversation, seen) = edited_call(&server, &node.token, body, |_| Value::Null).await;
    let events = server.events(conversation).await;
    let stored = serde_json::to_string(&events).unwrap();
    let resolved = events.iter().find(|event| event["kind"] == "review_resolved").unwrap();

    assert_eq!(seen["value"]["messages"].as_array().unwrap().len(), 2);
    assert_eq!(reply["result"]["content"][0]["text"], "The person sent nothing from this response.");
    assert!(!stored.contains("secret-recipe") && !stored.contains("Noon?"));
    assert_eq!(resolved["payload"]["outcome"], "nothing");
}

#[tokio::test]
async fn a_dropped_message_never_reaches_the_agent_and_the_edit_is_noted() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (router, _) = fake_zulip();
    let base = serve(router).await;
    add_zulip(&server, &base, "Zulip").await;

    let (reply, conversation, _) = edited_call(&server, &node.token, call(1, "zulip__get_messages", json!({"channel": "general", "topic": "lunch"})), |value| json!({"messages": [value["messages"][0].clone()], "coverage": value["coverage"]})).await;
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    let stored = serde_json::to_string(&server.events(conversation).await).unwrap();

    assert!(text.contains("Noon?") && !text.contains("secret-recipe"));
    assert!(text.ends_with("[The person edited this response before you saw it.]"));
    assert!(!stored.contains("Noon?"));
}

#[tokio::test]
async fn an_agent_filter_is_stripped_before_the_call_and_preselects_what_it_matches() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (router, zulip) = fake_zulip();
    let base = serve(router).await;
    add_zulip(&server, &base, "Zulip").await;
    let body = call(1, "zulip__get_messages", json!({"channel": "general", "topic": "lunch", "response_filter": ".messages[] | select(.content | test(\"Noon\"))"}));

    let (_, _, seen) = edited_call(&server, &node.token, body, Value::clone).await;

    assert_eq!(seen["filter"], ".messages[] | select(.content | test(\"Noon\"))");
    assert_eq!(seen["selected_paths"], json!([["messages", 0]]));
    assert!(seen.get("filter_error").is_none());
    assert!(zulip.seen.lock().unwrap().iter().all(|(_, _, body)| !body.contains("response_filter")));
}

#[tokio::test]
async fn editing_the_filter_returns_new_paths_a_replacement_value_or_a_readable_error() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (router, _) = fake_zulip();
    let base = serve(router).await;
    add_zulip(&server, &base, "Zulip").await;
    let conversation = server.node_conversation(&node.token).await;
    let body = call(1, "zulip__get_messages", json!({"channel": "general", "topic": "lunch"}));

    let (_, outcome) = tokio::join!(server.agent_mcp(&node.token, conversation, body), async {
        let permission = server.open_request(conversation).await;
        server.answer(conversation, &permission, "approve_edit").await;
        let held = open_review(&server, conversation).await;
        let path = format!("/api/v1/conversations/{conversation}/responses/{}/filter", held["request_id"].as_str().unwrap());
        let ask = |filter: &str| server.user.json(server.user.post(&path).json(&json!({"filter": filter})));
        let paths = ask(".messages[1]").await;
        let replaced = ask(".messages | map(.content)").await;
        let cleared = ask("  ").await;
        let broken = ask(".[").await;

        send_response(&server, conversation, &held, Value::Null).await;

        (paths, replaced, cleared, broken)
    });
    let (paths, replaced, cleared, broken) = outcome;

    assert_eq!((paths.0, paths.1), (200, json!({"selected_paths": [["messages", 1]]})));
    assert_eq!(replaced.1["value"], json!(["Noon?", "Pizza secret-recipe?"]));
    assert_eq!(cleared.1["value"]["messages"].as_array().unwrap().len(), 2);
    assert_eq!(broken.0, 400);
    assert!(broken.1["message"].as_str().unwrap().starts_with("The filter could not be read: ") && broken.1["message"].as_str().unwrap().ends_with(". Fix it or clear it."));
}

#[tokio::test]
async fn a_response_is_gone_once_answered() {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let (router, _) = fake_zulip();
    let base = serve(router).await;
    add_zulip(&server, &base, "Zulip").await;
    let conversation = server.node_conversation(&node.token).await;
    let body = call(1, "zulip__get_messages", json!({"channel": "general", "topic": "lunch"}));

    let (_, request) = tokio::join!(server.agent_mcp(&node.token, conversation, body), async {
        let permission = server.open_request(conversation).await;
        server.answer(conversation, &permission, "approve_edit").await;
        let held = open_review(&server, conversation).await;

        assert_eq!(send_response(&server, conversation, &held, json!([])).await, 204);

        held
    });
    let (gone, error) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/responses/{}", request["request_id"].as_str().unwrap()))).await;
    let again = send_response(&server, conversation, &request, json!([])).await;

    assert_eq!((gone, again), (404, 409));
    assert_eq!(error["message"], "This response is no longer waiting for you.");
}

async fn read(server: &Server, token: &str, tool: &str, arguments: Value) -> Value {
    let (reply, _, _) = edited_call(server, token, call(1, &format!("zulip__{tool}"), arguments), Value::clone).await;

    assert_eq!(reply["result"]["isError"], false, "{reply}");

    serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn ids(result: &Value) -> Vec<u64> {
    result["messages"].as_array().unwrap().iter().map(|message| message["id"].as_u64().unwrap()).collect()
}

async fn zulip_agent() -> (Server, String, std::sync::Arc<fake_zulip::FakeZulip>, crate::common::NodeLink) {
    let server = Server::start().await;
    let node = server.online_node("laptop").await;
    let token = node.token.clone();
    let (router, zulip) = fake_zulip();
    let base = serve(router).await;
    add_zulip(&server, &base, "Zulip").await;

    (server, token, zulip, node)
}

#[tokio::test]
async fn a_message_id_returns_that_message_with_its_neighbours_in_the_same_topic() {
    let (server, token, _zulip, _node) = zulip_agent().await;

    let result = read(&server, &token, "get_messages", json!({"message": 54, "before": 1, "after": 1})).await;

    assert_eq!(ids(&result), [53, 54, 55]);
    assert_eq!(result["coverage"], json!({"returned": 3, "found_oldest": false, "found_newest": false, "oldest_id": 53, "newest_id": 55, "next": 53}));
}

#[tokio::test]
async fn an_older_limit_argument_still_sets_how_many_messages_come_back() {
    let (server, token, _zulip, _node) = zulip_agent().await;

    let limited = read(&server, &token, "get_messages", json!({"limit": 2})).await;
    let counted = read(&server, &token, "get_messages", json!({"before": 2})).await;

    assert_eq!(ids(&limited), ids(&counted));
    assert_eq!(limited["coverage"]["returned"], 2);
}

#[tokio::test]
async fn a_permalink_returns_its_message_and_the_replies_around_it_in_the_topic_and_not_the_channels_other_topics() {
    let (server, token, zulip, _node) = zulip_agent().await;
    let link = "https://zulip.example.com/#narrow/channel/12-dev/topic/release/near/57";

    let result = read(&server, &token, "get_messages", json!({"message": link})).await;

    assert_eq!(ids(&result), [51, 52, 53, 54, 55, 57, 58]);
    assert_eq!(result["coverage"]["found_oldest"], true);
    assert_eq!(result["coverage"]["found_newest"], true);
    assert_eq!(result["coverage"]["next"], Value::Null);
    assert!(zulip.paths().contains(&"/api/v1/messages/57".to_string()));
}

#[tokio::test]
async fn a_link_without_a_message_id_is_refused_without_reaching_zulip() {
    let (server, token, _zulip, node) = zulip_agent().await;
    let conversation = server.node_conversation(&token).await;
    let body = call(1, "zulip__get_messages", json!({"message": "https://zulip.example.com/#narrow/channel/12-dev/topic/release"}));

    let (reply, ()) = tokio::join!(server.agent_mcp(&node.token, conversation, body), async {
        let permission = server.open_request(conversation).await;
        server.answer(conversation, &permission, "approve_edit").await;
    });

    assert_eq!(reply["result"]["isError"], true);
    assert!(reply["result"]["content"][0]["text"].as_str().unwrap().contains("/near/"));
}

#[tokio::test]
async fn the_next_cursor_pages_back_through_history_without_repeating_a_message() {
    let (server, token, _zulip, _node) = zulip_agent().await;

    let first = read(&server, &token, "get_messages", json!({"channel": "dev", "before": 3})).await;
    let second = read(&server, &token, "get_messages", json!({"channel": "dev", "before": 3, "anchor": first["coverage"]["next"]})).await;
    let third = read(&server, &token, "get_messages", json!({"channel": "dev", "before": 3, "anchor": second["coverage"]["next"]})).await;

    assert_eq!((ids(&first), first["coverage"]["found_oldest"].clone(), first["coverage"]["found_newest"].clone()), (vec![56, 57, 58], json!(false), json!(true)));
    assert_eq!(ids(&second), [53, 54, 55]);
    assert_eq!(ids(&third), [51, 52]);
    assert_eq!((third["coverage"]["found_oldest"].clone(), third["coverage"]["next"].clone()), (json!(true), Value::Null));
}

#[tokio::test]
async fn reading_forward_from_the_oldest_message_pages_with_next() {
    let (server, token, _zulip, _node) = zulip_agent().await;

    let first = read(&server, &token, "get_messages", json!({"channel": "dev", "anchor": "oldest", "after": 5})).await;
    let second = read(&server, &token, "get_messages", json!({"channel": "dev", "after": 5, "anchor": first["coverage"]["next"]})).await;

    assert_eq!(ids(&first), [51, 52, 53, 54, 55]);
    assert_eq!(first["coverage"]["next"], 55);
    assert_eq!(ids(&second), [56, 57, 58]);
    assert_eq!((second["coverage"]["found_newest"].clone(), second["coverage"]["next"].clone()), (json!(true), Value::Null));
}

#[tokio::test]
async fn since_stops_at_the_boundary_and_says_nothing_older_is_in_range() {
    let (server, token, zulip, _node) = zulip_agent().await;
    let since = chrono::DateTime::from_timestamp(START + DAY * 6, 0).unwrap().to_rfc3339();

    let result = read(&server, &token, "get_messages", json!({"channel": "dev", "since": since})).await;

    assert_eq!(ids(&result), [56, 57, 58]);
    assert_eq!((result["coverage"]["found_oldest"].clone(), result["coverage"]["next"].clone()), (json!(true), Value::Null));
    assert_eq!(zulip.paths().iter().filter(|path| *path == "/api/v1/messages").count(), 1);
}

#[tokio::test]
async fn until_drops_newer_messages_and_a_date_only_until_includes_that_whole_day() {
    let (server, token, _zulip, _node) = zulip_agent().await;
    let day = |offset: i64| chrono::DateTime::from_timestamp(START + DAY * offset, 0).unwrap().format("%Y-%m-%d").to_string();

    let window = read(&server, &token, "get_messages", json!({"channel": "dev", "since": day(2), "until": day(4)})).await;
    let capped = read(&server, &token, "get_messages", json!({"channel": "dev", "until": day(4), "before": 2})).await;

    assert_eq!(ids(&window), [52, 53, 54]);
    assert_eq!((window["coverage"]["found_oldest"].clone(), window["coverage"]["found_newest"].clone()), (json!(true), json!(true)));
    assert_eq!(ids(&capped), [53, 54]);
    assert_eq!((capped["coverage"]["found_oldest"].clone(), capped["coverage"]["next"].clone()), (json!(false), json!(53)));
}

#[tokio::test]
async fn an_unreadable_date_is_refused_with_the_format_to_use() {
    let (server, token, _zulip, node) = zulip_agent().await;
    let conversation = server.node_conversation(&token).await;
    let body = call(1, "zulip__get_messages", json!({"channel": "dev", "since": "last Thursday"}));

    let (reply, ()) = tokio::join!(server.agent_mcp(&node.token, conversation, body), async {
        let permission = server.open_request(conversation).await;
        server.answer(conversation, &permission, "approve_edit").await;
    });

    assert_eq!(reply["result"]["isError"], true);
    assert!(reply["result"]["content"][0]["text"].as_str().unwrap().contains("ISO 8601"));
}

#[tokio::test]
async fn search_filters_by_topic_sender_and_dates_alongside_keywords() {
    let (server, token, _zulip, _node) = zulip_agent().await;
    let since = chrono::DateTime::from_timestamp(START + DAY * 3, 0).unwrap().format("%Y-%m-%d").to_string();

    let by_topic = read(&server, &token, "search_messages", json!({"channel": "dev", "topic": "ops"})).await;
    let by_name = read(&server, &token, "search_messages", json!({"channel": "dev", "sender": "grace", "query": "release"})).await;
    let by_date = read(&server, &token, "search_messages", json!({"channel": "dev", "sender": "linus@example.com", "since": since})).await;

    assert_eq!(ids(&by_topic), [56]);
    assert_eq!(ids(&by_name), [51, 54, 57]);
    assert_eq!(ids(&by_date), [55, 58]);
}

#[tokio::test]
async fn from_me_and_the_other_native_operators_in_the_query_build_the_narrow() {
    let (server, token, zulip, _node) = zulip_agent().await;

    let mine = read(&server, &token, "search_messages", json!({"query": "from:me"})).await;
    let sender_me = read(&server, &token, "search_messages", json!({"query": "sender:me channel:dev -topic:ops"})).await;
    let quoted = read(&server, &token, "search_messages", json!({"query": "stream:general topic:\"lunch\" pizza"})).await;
    let near = read(&server, &token, "search_messages", json!({"query": "near:53 channel:dev", "before": 1, "after": 1})).await;

    assert_eq!(ids(&mine), [53, 56]);
    assert_eq!(ids(&sender_me), [53]);
    assert_eq!(ids(&quoted), [42]);
    assert_eq!(ids(&near), [52, 53, 54]);
    assert!(zulip.seen.lock().unwrap().iter().any(|(_, _, query)| query.contains("%22operand%22%3A%22ada%40example.com%22")));
}

#[tokio::test]
async fn an_operator_zulip_search_is_not_given_here_is_refused_and_named() {
    let (server, token, zulip, node) = zulip_agent().await;
    let conversation = server.node_conversation(&token).await;
    let body = call(1, "zulip__search_messages", json!({"query": "frm:me lunch"}));

    let (reply, ()) = tokio::join!(server.agent_mcp(&node.token, conversation, body), async {
        let permission = server.open_request(conversation).await;
        server.answer(conversation, &permission, "approve_edit").await;
    });
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();

    assert_eq!(reply["result"]["isError"], true);
    assert!(text.contains("frm:") && text.contains("not supported") && text.contains("from"));
    assert_eq!(zulip.paths(), ["/api/v1/users/me"]);
}

#[tokio::test]
async fn the_tool_descriptions_name_the_arguments_and_operators_agents_can_use() {
    let (server, token, _zulip, _node) = zulip_agent().await;

    let listed = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
    let tools = listed["result"]["tools"].as_array().unwrap();
    let described = |name: &str| tools.iter().find(|tool| tool["name"] == name).unwrap().to_string();

    assert!(["anchor", "before", "after", "since", "until", "sender", "message", "next"].iter().all(|word| described("zulip__get_messages").contains(word)));
    assert!(["from:", "stream:", "topic:", "has:", "is:", "near:", "id:", "me"].iter().all(|word| described("zulip__search_messages").contains(word)));
}
