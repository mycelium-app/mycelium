mod common;

use common::{NodeLink, Server};
use mycelium_proto::link::{ControlToNode, GitCommand, NodeToControl};
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

async fn next_matching<T>(link: &mut NodeLink, pick: impl Fn(&ControlToNode) -> Option<T>) -> T {
    loop {
        let message = link.next().await;

        if let Some(found) = pick(&message) {
            return found;
        }
    }
}

async fn wellness_on(server: &Server, link: &NodeLink) -> String {
    let (status, assistant) = server.user.json(server.user.get("/api/v1/assistants/builtin/wellness")).await;
    assert_eq!(status, 200, "{assistant}");
    let id = assistant["id"].as_str().unwrap().to_string();
    let (status, patched) = server.user.json(server.user.patch(&format!("/api/v1/assistants/{id}")).json(&json!({"preferences": {"models": [{"agent": "claude"}], "nodes": [link.id]}}))).await;
    assert_eq!(status, 200, "{patched}");

    id
}

async fn action_id(server: &Server, assistant: &str, name: &str) -> String {
    let (_, actions) = server.user.json(server.user.get(&format!("/api/v1/assistants/{assistant}/actions"))).await;

    actions.as_array().unwrap().iter().find(|action| action["name"] == name).unwrap()["id"].as_str().unwrap().to_string()
}

async fn run_action(server: &Server, link: &mut NodeLink, assistant: &str, name: &str, inputs: Value) -> (Uuid, String) {
    let id = action_id(server, assistant, name).await;
    let (status, started) = server.user.json(server.user.post(&format!("/api/v1/assistants/{assistant}/actions/{id}/run")).json(&json!({"inputs": inputs}))).await;
    assert_eq!(status, 200, "{started}");
    let conversation: Uuid = started["id"].as_str().unwrap().parse().unwrap();
    let starting = next_matching(link, |message| matches!(message, ControlToNode::StartSession { .. }).then_some(())).await;
    let _ = starting;
    link.start_session(conversation, "acp-wellness").await;
    let (_, text) = link.expect_prompt().await;

    (conversation, text)
}

async fn save_reflection(server: &Server, token: &str, conversation: Uuid, arguments: Value) -> Value {
    let body = server.agent_mcp(token, conversation, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "save_reflection", "arguments": arguments}})).await;

    body["result"].clone()
}

async fn reflections(server: &Server) -> Vec<Value> {
    let (_, listed) = server.user.json(server.user.get("/api/v1/reflections")).await;

    listed.as_array().unwrap().clone()
}

#[tokio::test]
async fn the_wellness_assistant_is_created_once_with_its_actions_and_schedules() {
    let server = Server::start().await;

    let (status, first) = server.user.json(server.user.get("/api/v1/assistants/builtin/wellness")).await;
    let (_, second) = server.user.json(server.user.get("/api/v1/assistants/builtin/wellness")).await;
    assert_eq!((status, first["builtin_key"].clone(), first["name"].clone()), (200, json!("wellness"), json!("Wellness")));
    assert_eq!(first["id"], second["id"]);

    let id = first["id"].as_str().unwrap();
    let (_, actions) = server.user.json(server.user.get(&format!("/api/v1/assistants/{id}/actions"))).await;
    let names: Vec<&str> = actions.as_array().unwrap().iter().map(|action| action["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Weekly reflection", "End-of-day wrap-up", "Review an exercise"]);
    let (_, schedules) = server.user.json(server.user.get("/api/v1/schedules")).await;
    let crons: Vec<&str> = schedules.as_array().unwrap().iter().map(|schedule| schedule["cron"].as_str().unwrap()).collect();
    assert_eq!(crons, ["0 16 * * 5", "30 17 * * 1-5"]);
    assert!(schedules.as_array().unwrap().iter().all(|schedule| schedule["assistant_id"] == json!(id) && schedule["node_id"].is_null()));
}

#[tokio::test]
async fn the_wellness_assistant_cannot_be_renamed_archived_or_have_actions_deleted() {
    let server = Server::start().await;
    let (_, wellness) = server.user.json(server.user.get("/api/v1/assistants/builtin/wellness")).await;
    let id = wellness["id"].as_str().unwrap();

    let (renamed, body) = server.user.json(server.user.patch(&format!("/api/v1/assistants/{id}")).json(&json!({"name": "Health"}))).await;
    let (archived, _) = server.user.json(server.user.patch(&format!("/api/v1/assistants/{id}")).json(&json!({"archived": true}))).await;
    let action = action_id(&server, id, "Weekly reflection").await;
    let (deleted, _) = server.user.json(server.user.delete(&format!("/api/v1/assistants/{id}/actions/{action}"))).await;

    assert_eq!((renamed, body["error"].clone(), archived, deleted), (409, json!("assistant_built_in"), 409, 409));
}

#[tokio::test]
async fn a_built_in_action_can_be_edited_and_reset_to_its_default() {
    let server = Server::start().await;
    let (_, wellness) = server.user.json(server.user.get("/api/v1/assistants/builtin/wellness")).await;
    let id = wellness["id"].as_str().unwrap();
    let action = action_id(&server, id, "End-of-day wrap-up").await;
    let path = format!("/api/v1/assistants/{id}/actions/{action}");

    let (status, edited) = server.user.json(server.user.put(&path).json(&json!({"name": "End-of-day wrap-up", "prompt": "Keep it to one line.", "inputs": [], "pinned": false}))).await;
    assert_eq!((status, edited["prompt"].clone(), edited["builtin_key"].clone()), (200, json!("Keep it to one line."), json!("end_of_day")));
    let (_, restored) = server.user.json(server.user.post(&format!("{path}/reset"))).await;

    assert_eq!((restored["pinned"].clone(), restored["prompt"].as_str().unwrap().starts_with("Wrap up my day")), (json!(true), true));
}

#[tokio::test]
async fn save_reflection_keeps_edited_sections() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let assistant = wellness_on(&server, &link).await;
    let (scheduled, asked) = run_action(&server, &mut link, &assistant, "Weekly reflection", json!({"week": "2026-10-28"})).await;
    assert!(asked.contains("the week containing 2026-10-28") && asked.contains("save_reflection"), "{asked}");
    let token = link.token.clone();
    let saved = save_reflection(&server, &token, scheduled, json!({"week": "2026-10-28", "done": "Shipped A", "learned": "L1", "waiting": "W1", "next_week": "N1"})).await;
    assert_eq!(saved["isError"], false, "{saved}");
    let stored = reflections(&server).await;
    assert_eq!(stored[0]["week"], "2026-10-26");
    let id = stored[0]["id"].as_str().unwrap().to_string();
    server.user.json(server.user.patch(&format!("/api/v1/reflections/{id}")).json(&json!({"learned": "Mine"}))).await;
    link.send(NodeToControl::TurnEnd { conversation_id: scheduled, seq: 3, stop_reason: "end_turn".into(), usage: None }).await;

    let (status, redraft) = server.user.json(server.user.post(&format!("/api/v1/reflections/{id}/redraft"))).await;
    assert_eq!(status, 202, "{redraft}");
    let (_, redraft_prompt) = link.expect_prompt().await;
    assert!(redraft_prompt.contains("the week containing 2026-10-26"), "{redraft_prompt}");
    assert_eq!(scheduled.to_string(), redraft["conversation_id"]);
    save_reflection(&server, &token, scheduled, json!({"week": "2026-10-26", "done": "Shipped B", "learned": "L2", "waiting": "W2", "next_week": "N2"})).await;

    let after = reflections(&server).await;
    assert_eq!(after.len(), 1);
    let (done, learned, waiting, edited) = (after[0]["done"].clone(), after[0]["learned"].clone(), after[0]["waiting"].clone(), after[0]["edited"].clone());
    assert_eq!((done, learned, waiting, edited), (json!("Shipped B"), json!("Mine"), json!("W2"), json!(["learned"])));
}

#[tokio::test]
async fn a_conversation_that_is_not_the_wellness_assistant_cannot_save_a_reflection() {
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
    let assistant = wellness_on(&server, &link).await;
    let (scheduled, _) = run_action(&server, &mut link, &assistant, "Weekly reflection", json!({})).await;
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
async fn redrafting_before_the_wellness_assistant_has_a_model_says_what_to_do() {
    let server = Server::start().await;
    let user = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    let id = sqlx::query_scalar::<_, Uuid>("insert into reflections (owner, week) values ($1, '2026-10-26') returning id").bind(user).fetch_one(&server.pool).await.unwrap();

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/reflections/{id}/redraft"))).await;

    assert_eq!((status, body["error"].clone()), (400, json!("model_required")));
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

    let (_, listed) = server.user.json(server.user.get("/api/v1/skills?hidden=true")).await;
    let listed = listed.as_array().unwrap().clone();
    let shown = skill(&listed, "language", "Rust").expect("hidden skill is listed under hidden");
    assert_eq!(shown["hidden"], json!(true));

    let (status, _) = server.user.json(server.user.delete("/api/v1/skills/hidden").json(&hidden)).await;
    assert_eq!(status, 204);
    assert!(skill(&skills(&server).await, "language", "Rust").is_some());

    let (status, body) = server.user.json(server.user.put("/api/v1/skills/hidden").json(&json!({"kind": "nonsense", "name": "Rust"}))).await;
    assert_eq!((status, body["error"].clone()), (400, json!("skill_invalid")));
}

async fn project_on(server: &Server, link: &NodeLink) {
    sqlx::query("insert into projects (owner, node_id, name, path) select id, $1, 'app', '/work/app' from users limit 1").bind(link.id).execute(&server.pool).await.unwrap();
}

async fn tool(server: &Server, token: &str, conversation: Uuid, name: &str, arguments: Value) -> Value {
    let body = server.agent_mcp(token, conversation, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}})).await;

    body["result"].clone()
}

async fn proposed_exercise(server: &Server, link: &NodeLink, source: Uuid) -> String {
    let result = tool(server, &link.token, source, "propose_exercise", json!({"title": "Parse a duration", "brief": "Write parse_duration.", "skill_kind": "language", "skill_name": "Rust", "minutes": 20})).await;
    assert_ne!(result["isError"], true, "{result}");
    let (_, listed) = server.user.json(server.user.get("/api/v1/exercises")).await;

    listed[0]["id"].as_str().unwrap().to_string()
}

async fn answer_git(link: &mut NodeLink, output: Value) -> (String, GitCommand) {
    loop {
        match link.next().await {
            ControlToNode::Git { request_id, cwd, command } => {
                link.send(NodeToControl::GitResult { request_id, ok: true, output, error: None }).await;

                return (cwd, command);
            }
            ControlToNode::StartSession { .. } => continue,
            other => panic!("expected git, got {other:?}"),
        }
    }
}

async fn start_exercise(server: &Server, link: &mut NodeLink, exercise: &str) -> (Uuid, Value) {
    let user = server.user.clone();
    let path = format!("/api/v1/exercises/{exercise}/start");
    let starting = tokio::spawn(async move { user.json(user.post(&path)).await });
    let (cwd, command) = answer_git(link, json!({"path": "/work/app-worktrees/learn-parse", "branch": "learn/x"})).await;
    let (status, body) = starting.await.unwrap();

    assert_eq!((status, cwd.as_str()), (201, "/work/app"), "{body}");
    assert!(matches!(command, GitCommand::AddWorktree { ref branch } if branch.starts_with("learn/parse-a-duration-")), "{command:?}");

    let conversation: Uuid = body["conversation_id"].as_str().unwrap().parse().unwrap();
    link.expect_start_session().await;
    link.start_session(conversation, "acp-learn").await;

    (conversation, body)
}

#[tokio::test]
async fn starting_an_exercise_adds_a_learn_branch_worktree_and_conversation() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    project_on(&server, &link).await;
    let source = started_conversation(&server, &mut link).await;
    let exercise = proposed_exercise(&server, &link, source).await;

    let (conversation, started) = start_exercise(&server, &mut link, &exercise).await;

    assert_eq!(started["state"], "started");
    assert!(started["branch"].as_str().unwrap().starts_with("learn/parse-a-duration-"));
    let opened = server.conversation(conversation).await;
    assert_eq!(opened["cwd"], "/work/app-worktrees/learn-parse");
    assert_eq!(opened["branch"], started["branch"]);
}

#[tokio::test]
async fn starting_an_exercise_without_a_project_says_what_to_do() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = started_conversation(&server, &mut link).await;
    let exercise = proposed_exercise(&server, &link, source).await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/exercises/{exercise}/start"))).await;

    assert_eq!((status, body["error"].clone()), (409, json!("exercise_needs_project")));
}

async fn suggested_count(server: &Server) -> usize {
    let (_, body) = server.user.json(server.user.get("/api/v1/exercises/suggested")).await;

    body.as_array().unwrap().len()
}

#[tokio::test]
async fn not_now_hides_a_suggestion_until_tomorrow() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let source = started_conversation(&server, &mut link).await;
    let exercise = proposed_exercise(&server, &link, source).await;
    assert_eq!(suggested_count(&server).await, 1);

    let (status, hidden) = server.user.json(server.user.post(&format!("/api/v1/exercises/{exercise}/not-now"))).await;
    assert_eq!(status, 200, "{hidden}");
    assert_eq!(suggested_count(&server).await, 0);

    sqlx::query("update exercises set not_now_until = now() - interval '1 minute'").execute(&server.pool).await.unwrap();
    assert_eq!(suggested_count(&server).await, 1);
}

#[tokio::test]
async fn a_project_test_command_can_be_set_and_cleared() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    project_on(&server, &link).await;
    let project: Uuid = sqlx::query_scalar("select id from projects").fetch_one(&server.pool).await.unwrap();
    let path = format!("/api/v1/projects/{project}");
    let stored = || async { sqlx::query_scalar::<_, Option<String>>("select test_command from projects").fetch_one(&server.pool).await.unwrap() };

    let (status, body) = server.user.json(server.user.patch(&path).json(&json!({"test_command": " make check "}))).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(stored().await.as_deref(), Some("make check"));

    let (status, body) = server.user.json(server.user.patch(&path).json(&json!({"test_command": ""}))).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(stored().await, None);
}

#[tokio::test]
async fn run_tests_relays_the_project_test_command() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    project_on(&server, &link).await;
    sqlx::query("update projects set test_command = 'cargo nextest run'").execute(&server.pool).await.unwrap();
    let source = started_conversation(&server, &mut link).await;
    let exercise = proposed_exercise(&server, &link, source).await;
    start_exercise(&server, &mut link, &exercise).await;
    let user = server.user.clone();
    let path = format!("/api/v1/exercises/{exercise}/tests");
    let running = tokio::spawn(async move { user.json(user.post(&path)).await });

    let result = json!({"exit": 101, "passed": 5, "failed": 1, "failing": ["a::broken"], "output_tail": ""});
    let (cwd, command) = answer_git(&mut link, result.clone()).await;
    let (status, body) = running.await.unwrap();

    assert_eq!((cwd.as_str(), command), ("/work/app-worktrees/learn-parse", GitCommand::RunTests { command: Some("cargo nextest run".into()), timeout_secs: 600 }));
    assert_eq!((status, body), (200, result.clone()));
    let (_, listed) = server.user.json(server.user.get("/api/v1/exercises?skill=Rust")).await;
    assert_eq!(listed[0]["tests"], result);
}

#[tokio::test]
async fn review_runs_through_the_wellness_assistant_and_saves_comments_and_test_results() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    project_on(&server, &link).await;
    let assistant = wellness_on(&server, &link).await;
    let source = started_conversation(&server, &mut link).await;
    let exercise = proposed_exercise(&server, &link, source).await;
    start_exercise(&server, &mut link, &exercise).await;
    let tests = json!({"exit": 0, "passed": 6, "failed": 0, "failing": []});
    sqlx::query("update exercises set last_tests = $1").bind(&tests).execute(&server.pool).await.unwrap();
    let token = link.token.clone();
    let user = server.user.clone();
    let path = format!("/api/v1/exercises/{exercise}/review");
    let reviewing = tokio::spawn(async move { user.json(user.post(&path)).await });

    let (_, command) = answer_git(&mut link, json!({"diff": "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,1 +1,2 @@\n+fn parse_duration() {}\n"})).await;
    let (status, _) = reviewing.await.unwrap();
    assert_eq!((status, command), (202, GitCommand::ScopeDiff { scope: "branch".into() }));
    let conversation = next_matching(&mut link, |message| match message {
        ControlToNode::StartSession { conversation_id, .. } => Some(*conversation_id),
        _ => None,
    })
    .await;
    link.start_session(conversation, "acp-wellness").await;
    let (_, asked) = link.expect_prompt().await;
    assert!(asked.contains("save_exercise_review") && asked.contains("Do not edit") && asked.contains("Parse a duration") && asked.contains("parse_duration"), "{asked}");
    let (_, held) = server.user.json(server.user.get(&format!("/api/v1/assistants/{assistant}"))).await;
    assert_eq!(held["conversation_id"], json!(conversation));

    let saved = tool(&server, &token, conversation, "save_exercise_review", json!({"summary": "Close", "comments": [{"path": "src/lib.rs", "line": 4, "body": "Handle an empty string."}, {"path": "src/lib.rs", "line": 9, "body": "Nice use of split_once.", "praise": true}]})).await;
    assert_ne!(saved["isError"], true, "{saved}");

    let (_, listed) = server.user.json(server.user.get("/api/v1/exercises")).await;
    let review = &listed[0]["review"];
    assert_eq!((listed[0]["state"].clone(), review["summary"].clone(), review["tests"].clone()), (json!("reviewed"), json!("Close"), tests));
    assert_eq!((review["comments"][0]["line"].clone(), review["comments"][1]["praise"].clone()), (json!(4), json!(true)));
}

#[tokio::test]
async fn only_the_wellness_assistant_can_save_an_exercise_review() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    project_on(&server, &link).await;
    let source = started_conversation(&server, &mut link).await;
    let exercise = proposed_exercise(&server, &link, source).await;
    let (conversation, _) = start_exercise(&server, &mut link, &exercise).await;
    sqlx::query("update exercises set reviewing = true").execute(&server.pool).await.unwrap();
    let token = link.token.clone();

    let refused = tool(&server, &token, conversation, "save_exercise_review", json!({"summary": "Looks fine"})).await;

    assert_eq!(refused["isError"], true, "{refused}");
}

#[tokio::test]
async fn a_learned_note_is_kept_only_while_teach_me_is_on() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let id = started_conversation(&server, &mut link).await;
    let note = json!({"text": "Why pg_advisory_lock needs a session, not a transaction"});

    let refused = tool(&server, &link.token, id, "note_learned", note.clone()).await;
    assert_eq!(refused["isError"], true, "{refused}");

    teach_me(&server, id, true).await;
    let saved = tool(&server, &link.token, id, "note_learned", note).await;
    assert_ne!(saved["isError"], true, "{saved}");

    let (status, listed) = server.user.json(server.user.get("/api/v1/wellness/learned?limit=5")).await;
    assert_eq!(status, 200);
    assert_eq!((listed[0]["text"].clone(), listed[0]["source_kind"].clone(), listed[0]["conversation_id"].clone()), (json!("Why pg_advisory_lock needs a session, not a transaction"), json!("teach_me"), json!(id)));
}
