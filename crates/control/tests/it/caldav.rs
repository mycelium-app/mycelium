use crate::common;
#[path = "common/radicale.rs"]
mod radicale;

use axum::Router;
use axum::response::Redirect;
use axum::routing::any;
use crate::common::Server;
use radicale::{PASSWORD, Radicale, USERNAME};
use serde_json::{Value, json};

const WEEKLY: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//test//EN\r\nBEGIN:VEVENT\r\nUID:standup\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:20260302T090000Z\r\nDTEND:20260302T093000Z\r\nRRULE:FREQ=WEEKLY;COUNT=10\r\nSUMMARY:Standup\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
const LUNCH: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//test//EN\r\nBEGIN:VEVENT\r\nUID:lunch\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:20260304T120000Z\r\nDTEND:20260304T130000Z\r\nSUMMARY:Lunch with Alex\r\nLOCATION:Canteen\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

async fn add_calendar(server: &Server, address: &str, password: &str) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Calendar", "kind": "caldav", "url": address, "account": USERNAME, "bearer_token": password}))).await
}

async fn read(server: &Server, token: &str, id: u64, tool: &str, arguments: Value) -> Value {
    let anonymous = server.anonymous();
    let (status, body) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": format!("calendar__{tool}"), "arguments": arguments}}))).await;

    assert_eq!(status, 200, "{body}");

    body["result"].clone()
}

async fn write(server: &Server, token: &str, id: u64, tool: &str, arguments: Value) -> Value {
    let reply = server.approved_agent_mcp(token, json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": format!("calendar__{tool}"), "arguments": arguments}})).await;

    reply["result"].clone()
}

fn payload(result: &Value) -> Value {
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn message(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn a_calendar_account_is_discovered_through_well_known_and_stored_without_returning_the_password() {
    let server = Server::start().await;
    let radicale = Radicale::start(&["Work"]).await;
    let target = radicale.address.clone();
    let front = common::serve(Router::new().route("/.well-known/caldav", any(move || async move { Redirect::permanent(&target) }))).await;

    let (status, added) = add_calendar(&server, &front, PASSWORD).await;
    let stored = server.user.get("/api/v1/chat-tool-servers").send().await.unwrap().text().await.unwrap();

    assert_eq!(status, 201, "{added}");
    assert_eq!(added["kind"], "caldav");
    assert_eq!(added["account"], USERNAME);
    assert!(added["settings"]["calendar_home"].as_str().unwrap().starts_with(&radicale.address));
    assert!(added["settings"]["principal"].as_str().is_some());
    assert!(!stored.contains(PASSWORD));
    assert_eq!(added["policy"]["preset"], "ask_writes");
}

#[tokio::test]
async fn a_calendar_account_is_discovered_from_the_base_address_when_there_is_no_well_known() {
    let server = Server::start().await;
    let radicale = Radicale::start(&["Work"]).await;

    let (status, added) = add_calendar(&server, &radicale.address, PASSWORD).await;

    assert_eq!(status, 201, "{added}");
}

#[tokio::test]
async fn a_wrong_app_password_is_refused_with_advice_to_check_it() {
    let server = Server::start().await;
    let radicale = Radicale::start(&["Work"]).await;

    let (status, refused) = add_calendar(&server, &radicale.address, "wrong").await;

    assert_eq!(status, 400);
    assert!(refused["message"].as_str().unwrap().contains("did not accept the username or app password"), "{refused}");
}

#[tokio::test]
async fn an_account_with_no_calendars_is_refused() {
    let server = Server::start().await;
    let radicale = Radicale::start(&[]).await;

    let (status, refused) = add_calendar(&server, &radicale.address, PASSWORD).await;

    assert_eq!(status, 400);
    assert!(refused["message"].as_str().unwrap().contains("found no calendars for sam"), "{refused}");
}

#[tokio::test]
async fn testing_a_calendar_account_names_the_step_to_go_back_to_when_it_fails() {
    let server = Server::start().await;
    let radicale = Radicale::start(&["Work", "Home"]).await;
    let test = |password: &str, url: &str| server.user.json(server.user.post("/api/v1/chat-tool-servers/caldav-test").json(&json!({"url": url, "username": USERNAME, "password": password})));

    let (status, found) = test(PASSWORD, &radicale.address).await;
    let (_, wrong) = test("wrong", &radicale.address).await;
    let (_, unreachable) = test(PASSWORD, "http://127.0.0.1:9").await;

    assert_eq!(status, 200, "{found}");
    assert_eq!(found["calendars"].as_array().unwrap().len(), 2);
    assert_eq!(wrong["error"], "caldav_credential");
    assert_eq!(unreachable["error"], "caldav_address");
    assert!(unreachable["message"].as_str().unwrap().starts_with("Could not reach 127.0.0.1"));
}

#[tokio::test]
async fn an_account_page_lists_the_calendars_live() {
    let server = Server::start().await;
    let radicale = Radicale::start(&["Work", "Home"]).await;
    let (_, added) = add_calendar(&server, &radicale.address, PASSWORD).await;

    let (status, calendars) = server.user.json(server.user.get(&format!("/api/v1/integrations/{}/calendars", added["id"].as_str().unwrap()))).await;

    assert_eq!(status, 200, "{calendars}");
    assert_eq!(calendars.as_array().unwrap().iter().map(|calendar| calendar["name"].as_str().unwrap()).collect::<Vec<_>>().len(), 2);
}

#[tokio::test]
async fn an_agent_lists_calendars_and_the_events_in_a_range_including_a_recurring_event_expanded() {
    let server = Server::start().await;
    let radicale = Radicale::start(&["Work"]).await;
    let node = server.online_node("laptop").await;

    add_calendar(&server, &radicale.address, PASSWORD).await;
    radicale.put("Work", "standup", WEEKLY).await;
    radicale.put("Work", "lunch", LUNCH).await;

    let calendars = payload(&read(&server, &node.token, 1, "list_calendars", json!({})).await);
    let events = payload(&read(&server, &node.token, 2, "list_events", json!({"start": "2026-03-01T00:00:00Z", "end": "2026-03-22T00:00:00Z"})).await);
    let standups: Vec<&Value> = events.as_array().unwrap().iter().filter(|event| event["summary"] == "Standup").collect();

    assert_eq!(calendars[0]["name"], "Work");
    assert_eq!(calendars[0]["read_only"], false);
    assert_eq!(standups.len(), 3, "{events}");
    assert_eq!(standups[1]["start"], "2026-03-09T09:00:00Z");
    assert_eq!(events.as_array().unwrap().len(), 4);
}

#[tokio::test]
async fn searching_events_matches_title_location_and_description_within_the_range() {
    let server = Server::start().await;
    let radicale = Radicale::start(&["Work"]).await;
    let node = server.online_node("laptop").await;

    add_calendar(&server, &radicale.address, PASSWORD).await;
    radicale.put("Work", "lunch", LUNCH).await;

    let by_location = payload(&read(&server, &node.token, 1, "search_events", json!({"query": "canteen", "start": "2026-03-01", "end": "2026-03-31"})).await);
    let outside = payload(&read(&server, &node.token, 2, "search_events", json!({"query": "canteen", "start": "2026-04-01", "end": "2026-04-30"})).await);
    let unrelated = payload(&read(&server, &node.token, 3, "search_events", json!({"query": "dentist", "start": "2026-03-01", "end": "2026-03-31"})).await);

    assert_eq!(by_location.as_array().unwrap().len(), 1);
    assert_eq!(by_location[0]["summary"], "Lunch with Alex");
    assert!(outside.as_array().unwrap().is_empty());
    assert!(unrelated.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn an_event_can_be_created_changed_and_deleted_and_a_stale_etag_is_a_conflict() {
    let server = Server::start().await;
    let radicale = Radicale::start(&["Work"]).await;
    let node = server.online_node("laptop").await;
    let range = json!({"start": "2026-05-01", "end": "2026-05-31"});

    add_calendar(&server, &radicale.address, PASSWORD).await;

    let created = payload(&write(&server, &node.token, 1, "create_event", json!({"calendar": "Work", "summary": "Dentist", "start": "2026-05-04T09:00:00", "end": "2026-05-04T10:00:00", "timezone": "Europe/London", "location": "High Street"})).await);
    let listed = payload(&read(&server, &node.token, 2, "list_events", range.clone()).await);

    assert_eq!(listed[0]["summary"], "Dentist");
    assert_eq!(listed[0]["start"], "2026-05-04T08:00:00Z");
    assert_eq!(listed[0]["href"], created["href"]);

    let changed = write(&server, &node.token, 3, "update_event", json!({"href": created["href"], "etag": created["etag"], "summary": "Dentist (moved)"})).await;
    let after = payload(&read(&server, &node.token, 4, "list_events", range.clone()).await);

    assert_eq!(changed["isError"], false, "{changed}");
    assert_eq!(after[0]["summary"], "Dentist (moved)");
    assert_eq!(after[0]["location"], "High Street");
    assert_eq!(after[0]["start"], "2026-05-04T08:00:00Z");

    let stale = write(&server, &node.token, 5, "update_event", json!({"href": created["href"], "etag": created["etag"], "summary": "Again"})).await;
    let stale_delete = write(&server, &node.token, 6, "delete_event", json!({"href": created["href"], "etag": created["etag"]})).await;

    assert_eq!(stale["isError"], true);
    assert_eq!(message(&stale), "The event changed on the server since you read it. List it again and retry.");
    assert_eq!(stale_delete["isError"], true);

    let deleted = write(&server, &node.token, 7, "delete_event", json!({"href": after[0]["href"], "etag": after[0]["etag"]})).await;
    let gone = payload(&read(&server, &node.token, 8, "list_events", range).await);

    assert_eq!(deleted["isError"], false, "{deleted}");
    assert!(gone.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn an_agent_creating_an_event_waits_for_the_person_under_the_default_preset() {
    let server = Server::start().await;
    let radicale = Radicale::start(&["Work"]).await;
    let node = server.online_node("laptop").await;

    add_calendar(&server, &radicale.address, PASSWORD).await;

    let conversation = server.node_conversation(&node.token).await;
    let arguments = json!({"calendar": "Work", "summary": "Dentist", "start": "2026-05-04T09:00:00Z", "end": "2026-05-04T10:00:00Z"});
    let request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "calendar__create_event", "arguments": arguments}});

    let (reply, ()) = tokio::join!(server.agent_mcp(&node.token, conversation, request), async {
        let request = server.open_request(conversation).await;

        assert_eq!(request["toolCall"]["title"], "calendar__create_event");

        server.answer(conversation, &request, "deny").await;
    });
    let listed = payload(&read(&server, &node.token, 2, "list_events", json!({"start": "2026-05-01", "end": "2026-05-31"})).await);

    assert_eq!(reply["result"]["isError"], true);
    assert!(listed.as_array().unwrap().is_empty());
}
