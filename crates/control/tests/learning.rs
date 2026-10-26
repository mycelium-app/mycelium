mod common;

use common::{NodeLink, Server};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::{Value, json};
use uuid::Uuid;

async fn started_conversation(server: &Server, link: &mut NodeLink) -> Uuid {
    let conversation = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;

    conversation.id
}

async fn teach_me(server: &Server, id: Uuid, on: bool) -> (u16, Value) {
    server.user.json(server.user.patch(&format!("/api/v1/conversations/{id}")).json(&json!({"teach_me": on}))).await
}

async fn prompt(server: &Server, id: Uuid, text: &str) {
    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/prompt")).json(&json!({"text": text}))).await;
    assert!(status < 300, "{body}");
}

#[tokio::test]
async fn teach_me_adds_the_extension_to_the_session_layer() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    let (status, body) = teach_me(&server, id, true).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["teach_me"], true);

    prompt(&server, id, "How do I reverse a list?").await;

    match link.next().await {
        ControlToNode::StartSession { instructions, resume, .. } => {
            assert!(instructions.unwrap().contains("Teach me mode is on"));
            assert_eq!(resume.as_deref(), Some("acp-1"));
        }
        other => panic!("expected a restarted session, got {other:?}"),
    }
}

#[tokio::test]
async fn turning_teach_me_off_drops_the_extension_from_the_next_session() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    teach_me(&server, id, true).await;
    teach_me(&server, id, false).await;

    prompt(&server, id, "Just do it").await;

    match link.next().await {
        ControlToNode::StartSession { instructions, .. } => assert!(!instructions.unwrap_or_default().contains("Teach me mode is on")),
        other => panic!("expected a restarted session, got {other:?}"),
    }
}

#[tokio::test]
async fn toggling_teach_me_records_a_divider_event() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;

    teach_me(&server, id, true).await;
    teach_me(&server, id, true).await;
    teach_me(&server, id, false).await;

    let dividers: Vec<Value> = server.events(id).await.into_iter().filter(|event| event["kind"] == "teach_mode").map(|event| event["payload"]["on"].clone()).collect();
    assert_eq!(dividers, [json!(true), json!(false)]);
}

#[tokio::test]
async fn another_persons_conversation_cannot_be_toggled() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    let other = server.second_user("other@example.com").await;

    let (status, _) = other.json(other.patch(&format!("/api/v1/conversations/{id}")).json(&json!({"teach_me": true}))).await;

    assert_eq!(status, 404);
    assert_eq!(server.conversation(id).await["teach_me"], Value::Null);
}

async fn walkthrough(server: &Server, link: &mut NodeLink, conversation: Uuid, extra: Value) -> Value {
    let token = link.token.clone();

    walkthrough_reading(server, Some(link), token, conversation, extra).await
}

async fn walkthrough_reading(server: &Server, link: Option<&mut NodeLink>, token: String, conversation: Uuid, extra: Value) -> Value {
    let mut arguments = json!({"steps": [
        {"title": "Add the strings file", "files": [{"path": "src/strings.ts", "diff": "+export const strings = {}"}]},
        {"title": "Read the title from strings", "files": [{"path": "src/Screen.tsx", "diff": "+const title = strings.title"}]},
    ]});
    arguments.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    let diff = "diff --git a/src/strings.ts b/src/strings.ts\n--- a/src/strings.ts\n+++ b/src/strings.ts\n@@ -1 +1 @@\n+export const strings = {}\ndiff --git a/src/Screen.tsx b/src/Screen.tsx\n--- a/src/Screen.tsx\n+++ b/src/Screen.tsx\n@@ -1 +1 @@\n+const title = strings.title\n";
    let anonymous = server.anonymous();
    let asking = tokio::spawn(async move {
        let request = anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).header("x-mycelium-conversation", conversation.to_string()).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "walkthrough", "arguments": arguments}}));
        let (_, body) = anonymous.json(request).await;

        body["result"].clone()
    });

    if let Some(link) = link {
        loop {
            if let ControlToNode::Git { request_id, .. } = link.next().await {
                link.send(NodeToControl::GitResult { request_id, ok: true, output: json!({"diff": diff, "truncated": false}), error: None }).await;

                break;
            }
        }
    }

    asking.await.unwrap()
}

fn question() -> Value {
    json!({"checks": [{"step": 2, "question": "Where does the title come from?", "choices": ["A literal", "The strings file"], "answer": 1, "explanation": "Step 2 reads it from strings."}]})
}

async fn latest_walkthrough(server: &Server, conversation: Uuid) -> Value {
    server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/walkthrough"))).await.1
}

async fn answer(server: &Server, walkthrough: &str, step: i32, choice: i32) -> (u16, Value) {
    server.user.json(server.user.put(&format!("/api/v1/walkthroughs/{walkthrough}/answers/{step}")).json(&json!({"choice": choice}))).await
}

#[tokio::test]
async fn a_check_answer_says_correct_or_not_quite() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    let stored = walkthrough(&server, &mut link, id, question()).await;
    assert_eq!(stored["isError"], false, "{stored}");
    let shown = latest_walkthrough(&server, id).await;
    let walkthrough_id = shown["id"].as_str().unwrap().to_string();
    assert_eq!(shown["checks"][0]["choices"], json!(["A literal", "The strings file"]));
    assert!(shown["checks"][0].get("answer").is_none());

    let (status, wrong) = answer(&server, &walkthrough_id, 2, 0).await;
    assert_eq!((status, wrong["correct"].clone()), (200, json!(false)));
    let (_, right) = answer(&server, &walkthrough_id, 2, 1).await;
    assert_eq!((right["correct"].clone(), right["explanation"].clone()), (json!(true), json!("Step 2 reads it from strings.")));

    let (_, recorded) = server.user.json(server.user.get(&format!("/api/v1/walkthroughs/{walkthrough_id}/answers"))).await;
    assert_eq!(recorded, json!([{"step": 2, "choice": 1, "correct": true, "explanation": "Step 2 reads it from strings."}]));
}

#[tokio::test]
async fn answering_a_step_without_a_check_is_refused_and_records_nothing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    walkthrough(&server, &mut link, id, question()).await;
    let walkthrough_id = latest_walkthrough(&server, id).await["id"].as_str().unwrap().to_string();

    let (status, body) = answer(&server, &walkthrough_id, 1, 0).await;

    assert_eq!((status, body["error"].clone()), (400, json!("no_such_check")));
    let other = server.second_user("other@example.com").await;
    let (status, _) = other.json(other.put(&format!("/api/v1/walkthroughs/{walkthrough_id}/answers/2")).json(&json!({"choice": 1}))).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn walkthrough_with_a_bad_check_stores_nothing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;

    for checks in [
        json!([{"step": 3, "question": "q", "choices": ["a", "b"], "answer": 0}]),
        json!([{"step": 1, "question": "q", "choices": ["a"], "answer": 0}]),
        json!([{"step": 1, "question": "q", "choices": ["a", "b"], "answer": 2}]),
    ] {
        let result = walkthrough_reading(&server, None, link.token.clone(), id, json!({"checks": checks})).await;

        assert_eq!(result["isError"], true, "{result}");
        assert!(result["content"][0]["text"].as_str().unwrap().starts_with("Nothing was stored."));
    }

    assert_eq!(latest_walkthrough(&server, id).await, Value::Null);
}

async fn set_up_reflection(server: &Server, link: &NodeLink, enabled: bool) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/reflections/schedule").json(&json!({"node_id": link.id, "agent": "claude", "cwd": "/work/app", "enabled": enabled}))).await
}

async fn save_reflection(server: &Server, token: &str, conversation: Uuid, arguments: Value) -> Value {
    let body = server.agent_mcp(token, conversation, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "save_reflection", "arguments": arguments}})).await;

    body["result"].clone()
}

async fn reflections(server: &Server) -> Vec<Value> {
    let (_, listed) = server.user.json(server.user.get("/api/v1/reflections")).await;

    listed.as_array().unwrap().clone()
}

async fn first_drafting_run(server: &Server, link: &mut NodeLink, schedule: &str) -> Uuid {
    let (status, run) = server.user.json(server.user.post(&format!("/api/v1/schedules/{schedule}/run"))).await;
    assert!(status < 300, "{run}");

    link.expect_start_session().await
}

#[tokio::test]
async fn reflection_schedule_is_created_once() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;

    let (status, first) = set_up_reflection(&server, &link, true).await;
    assert_eq!(status, 200, "{first}");
    let (_, second) = set_up_reflection(&server, &link, true).await;
    let (_, paused) = set_up_reflection(&server, &link, false).await;

    assert_eq!(first["schedule_id"], second["schedule_id"]);
    assert_eq!((first["cron"].clone(), paused["enabled"].clone(), paused["schedule_id"].clone()), (json!("0 16 * * 5"), json!(false), first["schedule_id"].clone()));
    let count = sqlx::query_scalar::<_, i64>("select count(*) from schedules").fetch_one(&server.pool).await.unwrap();
    assert_eq!(count, 1);
    let (_, shown) = server.user.json(server.user.get("/api/v1/reflections/schedule")).await;
    assert_eq!(shown["enabled"], false);
}

#[tokio::test]
async fn save_reflection_keeps_edited_sections() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (_, schedule) = set_up_reflection(&server, &link, true).await;
    let scheduled = first_drafting_run(&server, &mut link, schedule["schedule_id"].as_str().unwrap()).await;
    let token = link.token.clone();
    let saved = save_reflection(&server, &token, scheduled, json!({"week": "2026-10-28", "done": "Shipped A", "learned": "L1", "waiting": "W1", "next_week": "N1"})).await;
    assert_eq!(saved["isError"], false, "{saved}");
    let stored = reflections(&server).await;
    assert_eq!(stored[0]["week"], "2026-10-26");
    let id = stored[0]["id"].as_str().unwrap().to_string();
    server.user.json(server.user.patch(&format!("/api/v1/reflections/{id}")).json(&json!({"learned": "Mine"}))).await;

    let (status, redraft) = server.user.json(server.user.post(&format!("/api/v1/reflections/{id}/redraft"))).await;
    assert_eq!(status, 202, "{redraft}");
    let redrafting = link.expect_start_session().await;
    assert_eq!(redrafting.to_string(), redraft["conversation_id"]);
    save_reflection(&server, &token, redrafting, json!({"week": "2026-10-26", "done": "Shipped B", "learned": "L2", "waiting": "W2", "next_week": "N2"})).await;

    let after = reflections(&server).await;
    assert_eq!(after.len(), 1);
    let (done, learned, waiting, edited) = (after[0]["done"].clone(), after[0]["learned"].clone(), after[0]["waiting"].clone(), after[0]["edited"].clone());
    assert_eq!((done, learned, waiting, edited), (json!("Shipped B"), json!("Mine"), json!("W2"), json!(["learned"])));
}

#[tokio::test]
async fn a_conversation_that_is_not_drafting_cannot_save_a_reflection() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    let token = link.token.clone();

    let refused = save_reflection(&server, &token, id, json!({"done": "Anything"})).await;

    assert_eq!(refused["isError"], true, "{refused}");
    assert!(reflections(&server).await.is_empty());
}

#[tokio::test]
async fn patch_marks_sections_edited() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (_, schedule) = set_up_reflection(&server, &link, true).await;
    let scheduled = first_drafting_run(&server, &mut link, schedule["schedule_id"].as_str().unwrap()).await;
    let token = link.token.clone();
    save_reflection(&server, &token, scheduled, json!({"done": "A", "learned": "B"})).await;
    let id = reflections(&server).await[0]["id"].as_str().unwrap().to_string();

    let (status, unchanged) = server.user.json(server.user.patch(&format!("/api/v1/reflections/{id}")).json(&json!({"done": "A"}))).await;
    assert_eq!((status, unchanged["edited"].clone()), (200, json!([])));
    let (_, edited) = server.user.json(server.user.patch(&format!("/api/v1/reflections/{id}")).json(&json!({"waiting": "Review from Sam", "learned": "B2"}))).await;

    let mut marked: Vec<String> = edited["edited"].as_array().unwrap().iter().map(|name| name.as_str().unwrap().to_string()).collect();
    marked.sort();
    assert_eq!(marked, ["learned", "waiting"]);
    let other = server.second_user("other@example.com").await;
    let (status, _) = other.json(other.patch(&format!("/api/v1/reflections/{id}")).json(&json!({"done": "Nope"}))).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn redrafting_before_the_reflection_is_set_up_says_what_to_do() {
    let server = Server::start().await;
    let user = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    let id = sqlx::query_scalar::<_, Uuid>("insert into reflections (owner, week) values ($1, '2026-10-26') returning id").bind(user).fetch_one(&server.pool).await.unwrap();

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/reflections/{id}/redraft"))).await;

    assert_eq!((status, body["error"].clone()), (409, json!("reflection_not_set_up")));
}

async fn skills(server: &Server) -> Vec<Value> {
    let (status, body) = server.user.json(server.user.get("/api/v1/skills")).await;
    assert_eq!(status, 200, "{body}");

    body.as_array().unwrap().clone()
}

fn skill<'a>(all: &'a [Value], kind: &str, name: &str) -> Option<&'a Value> {
    all.iter().find(|skill| skill["kind"] == kind && skill["name"] == name)
}

async fn agent_writes(link: &mut NodeLink, conversation: Uuid, seq: i64, path: &str, new_text: &str) {
    let update = json!({"update": {"sessionUpdate": "tool_call", "toolCallId": format!("call-{seq}"), "status": "completed", "content": [{"type": "diff", "path": path, "oldText": null, "newText": new_text}]}});

    link.send(NodeToControl::AcpUpdate { conversation_id: conversation, seq, update }).await;
}

#[tokio::test]
async fn agent_diff_events_count_lines_per_language_and_area() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;

    agent_writes(&mut link, id, 1, "/work/app/crates/control/src/a.rs", "fn a() {}\nfn b() {}\nfn c() {}\n").await;
    agent_writes(&mut link, id, 2, "/work/app/web/src/App.svelte", "<p>hi</p>\n").await;

    let all = common::eventually("agent lines counted", || async { Some(skills(&server).await).filter(|all| skill(all, "language", "Svelte").is_some()) }).await;

    assert_eq!(skill(&all, "language", "Rust").unwrap()["agent_lines"], 3);
    assert_eq!(skill(&all, "area", "crates/control").unwrap()["agent_lines"], 3);
    assert_eq!(skill(&all, "area", "web/src").unwrap()["agent_lines"], 1);
    assert_eq!(skill(&all, "language", "Rust").unwrap()["own_share"], 0.0);
}

#[tokio::test]
async fn saving_in_changes_counts_as_written_by_you() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    agent_writes(&mut link, id, 1, "/work/app/src/lib.rs", "fn a() {}\nfn b() {}\n").await;
    common::eventually("agent lines counted", || async { Some(skills(&server).await).filter(|all| skill(all, "language", "Rust").is_some()) }).await;

    let user = server.user.clone();
    let saving = tokio::spawn(async move { user.json(user.post(&format!("/api/v1/conversations/{id}/git/file")).json(&json!({"path": "src/lib.rs", "content": "fn a() {}\nfn b() {}\nfn mine() {}\nfn mine_too() {}\n", "expected": "fn a() {}\nfn b() {}\n"}))).await });
    let request_id = match link.next().await {
        ControlToNode::Git { request_id, .. } => request_id,
        other => panic!("expected git, got {other:?}"),
    };
    link.send(NodeToControl::GitResult { request_id, ok: true, output: json!({}), error: None }).await;
    assert_eq!(saving.await.unwrap().0, 200);

    let all = skills(&server).await;
    let rust = skill(&all, "language", "Rust").unwrap();

    assert_eq!((rust["own_lines"].clone(), rust["agent_lines"].clone(), rust["own_share"].clone()), (json!(2), json!(2), json!(0.5)));
    assert_eq!(rust["weekly_own_lines"].as_array().unwrap().last().unwrap(), 2);
}

#[tokio::test]
async fn hidden_skills_are_left_out() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    agent_writes(&mut link, id, 1, "/work/app/src/lib.rs", "fn a() {}\n").await;
    common::eventually("agent lines counted", || async { Some(skills(&server).await).filter(|all| skill(all, "language", "Rust").is_some()) }).await;

    let hidden = json!({"kind": "language", "name": "Rust"});
    let (status, _) = server.user.json(server.user.put("/api/v1/skills/hidden").json(&hidden)).await;
    assert_eq!(status, 204);
    assert!(skill(&skills(&server).await, "language", "Rust").is_none());

    let (status, _) = server.user.json(server.user.delete("/api/v1/skills/hidden").json(&hidden)).await;
    assert_eq!(status, 204);
    assert!(skill(&skills(&server).await, "language", "Rust").is_some());

    let (status, body) = server.user.json(server.user.put("/api/v1/skills/hidden").json(&json!({"kind": "nonsense", "name": "Rust"}))).await;
    assert_eq!((status, body["error"].clone()), (400, json!("skill_invalid")));
}
