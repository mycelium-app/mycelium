mod common;

use common::{Client, NodeLink, Server};
use mycelium_proto::link::{ControlToNode, GitCommand, NodeToControl};
use serde_json::{Value, json};
use uuid::Uuid;

async fn call_raw_with(anonymous: &Client, token: &str, conversation: Option<Uuid>, arguments: Value) -> Value {
    let mut request = anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "walkthrough", "arguments": arguments}}));

    if let Some(conversation) = conversation {
        request = request.header("x-mycelium-conversation", conversation.to_string());
    }

    let (status, body) = anonymous.json(request).await;
    assert_eq!(status, 200, "{body}");

    body["result"].clone()
}

async fn call_raw(server: &Server, token: &str, conversation: Option<Uuid>, arguments: Value) -> Value {
    call_raw_with(&server.anonymous(), token, conversation, arguments).await
}

fn diff_echoing(arguments: &Value) -> String {
    let mut diff = String::new();

    for step in arguments["steps"].as_array().unwrap() {
        for file in step["files"].as_array().into_iter().flatten() {
            let text = file["diff"].as_str().map(str::to_string).unwrap_or_else(|| file["hunks"].as_array().map(|hunks| hunks.iter().map(|hunk| hunk.as_str().unwrap()).collect::<Vec<_>>().join("\n")).unwrap_or_default());
            let path = file["path"].as_str().unwrap();

            if !diff.contains(&format!("diff --git a/{path} ")) {
                diff += &format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n{text}\n");
            }
        }
    }

    diff
}

async fn call_with_diff(server: &Server, link: &mut NodeLink, conversation: Option<Uuid>, arguments: Value, diff: &str) -> Value {
    let anonymous = server.anonymous();
    let token = link.token.clone();
    let asking = tokio::spawn(async move { call_raw_with(&anonymous, &token, conversation, arguments).await });

    loop {
        if let ControlToNode::Git { request_id, command, .. } = link.next().await {
            assert!(matches!(command, GitCommand::ScopeDiff { .. }), "{command:?}");
            link.send(NodeToControl::GitResult { request_id, ok: true, output: json!({"diff": diff, "truncated": false}), error: None }).await;

            break;
        }
    }

    asking.await.unwrap()
}

async fn call(server: &Server, link: &mut NodeLink, conversation: Option<Uuid>, arguments: Value) -> Value {
    let diff = diff_echoing(&arguments);

    call_with_diff(server, link, conversation, arguments, &diff).await
}

async fn latest(server: &Server, conversation: Uuid) -> Value {
    let (status, body) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/walkthrough"))).await;
    assert_eq!(status, 200, "{body}");

    body
}

fn steps() -> Value {
    json!({"steps": [
        {"title": "Add the shared strings file", "explanation": "New file.", "files": [{"path": "src/strings.ts", "diff": "+export const strings = {}"}]},
        {"title": "Restore reads its copy from strings", "key": true, "explanation": "The change itself.", "files": [{"path": "src/Screen.tsx", "hunks": ["-const title = \"Restore\"", "+const title = strings.title"]}]},
    ]})
}

#[tokio::test]
async fn the_walkthrough_tool_is_listed_with_a_description_that_says_when_to_use_it() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let anonymous = server.anonymous();

    let (_, tools) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))).await;

    let tool = tools["result"]["tools"].as_array().unwrap().iter().find(|tool| tool["name"] == "walkthrough").unwrap().clone();
    assert!(tool["description"].as_str().unwrap().contains("when the person asks for one"));
}

#[tokio::test]
async fn a_walkthrough_is_stored_for_the_conversation_with_steps_in_order_and_the_key_step_marked() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;

    let result = call(&server, &mut link, Some(conversation.id), steps()).await;

    assert_eq!(result["isError"], false, "{result}");
    let stored = latest(&server, conversation.id).await;
    let titles: Vec<&str> = stored["steps"].as_array().unwrap().iter().map(|step| step["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Add the shared strings file", "Restore reads its copy from strings"]);
    assert_eq!((stored["steps"][0]["key"].clone(), stored["steps"][1]["key"].clone()), (json!(false), json!(true)));
    assert_eq!(stored["steps"][0]["files"][0], json!({"path": "src/strings.ts", "diff": "@@ -1 +1 @@\n+export const strings = {}", "unchanged": false}));
}

#[tokio::test]
async fn hunks_are_joined_into_the_files_diff() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;

    call(&server, &mut link, Some(conversation.id), steps()).await;

    let stored = latest(&server, conversation.id).await;
    assert_eq!(stored["steps"][1]["files"][0]["diff"], "@@ -1 +1 @@\n-const title = \"Restore\"\n+const title = strings.title");
}

#[tokio::test]
async fn a_later_walkthrough_replaces_the_earlier_one_and_the_earlier_one_is_kept() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    call(&server, &mut link, Some(conversation.id), steps()).await;

    call(&server, &mut link, Some(conversation.id), json!({"steps": [{"title": "Only step", "files": [{"path": "a.rs", "diff": "+a"}]}]})).await;

    let stored = latest(&server, conversation.id).await;
    assert_eq!(stored["steps"].as_array().unwrap().len(), 1);
    let kept = sqlx::query_scalar::<_, i64>("select count(*) from walkthroughs where conversation_id = $1").bind(conversation.id).fetch_one(&server.pool).await.unwrap();
    assert_eq!(kept, 2);
}

#[tokio::test]
async fn a_conversation_without_a_walkthrough_has_none() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;

    assert_eq!(latest(&server, conversation.id).await, Value::Null);
}

#[tokio::test]
async fn a_walkthrough_without_steps_is_refused_with_how_to_fix_it() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;

    let result = call_raw(&server, &link.token, Some(conversation.id), json!({"steps": []})).await;

    assert_eq!(result["isError"], true);
    assert!(result["content"][0]["text"].as_str().unwrap().contains("Give the walkthrough's steps"));
    assert_eq!(latest(&server, conversation.id).await, Value::Null);
}

#[tokio::test]
async fn a_step_without_a_title_is_refused_and_nothing_is_stored() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;

    let untitled = call_raw(&server, &link.token, Some(conversation.id), json!({"steps": [{"explanation": "x"}]})).await;
    assert_eq!(untitled["isError"], true);
    assert!(untitled["content"][0]["text"].as_str().unwrap().contains("Step 1 has no title"));

    assert_eq!(latest(&server, conversation.id).await, Value::Null);
}

#[tokio::test]
async fn a_call_that_does_not_come_from_a_conversation_is_refused() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;

    let result = call_raw(&server, &token, None, steps()).await;

    assert_eq!(result["isError"], true);
}

#[tokio::test]
async fn sections_and_their_icons_are_stored_on_each_step_and_unknown_icons_are_dropped() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    let arguments = json!({"steps": [
        {"title": "Retry once", "section": "Retry logic", "icon": "logic", "files": [{"path": "a.rs", "diff": "+a"}]},
        {"title": "Cover it", "section": " Tests ", "icon": "sparkles", "files": [{"path": "b.rs", "diff": "+b"}]},
        {"title": "Loose end", "files": [{"path": "c.rs", "diff": "+c"}]},
    ]});

    call(&server, &mut link, Some(conversation.id), arguments).await;

    let stored = latest(&server, conversation.id).await;
    assert_eq!((stored["steps"][0]["section"].clone(), stored["steps"][0]["icon"].clone()), (json!("Retry logic"), json!("logic")));
    assert_eq!((stored["steps"][1]["section"].clone(), stored["steps"][1]["icon"].clone()), (json!("Tests"), Value::Null));
    assert_eq!(stored["steps"][2]["section"], Value::Null);
}

#[tokio::test]
async fn the_tool_description_tells_agents_to_group_steps_into_sections_and_mark_one_key_change() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let anonymous = server.anonymous();

    let (_, tools) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))).await;

    let tool = tools["result"]["tools"].as_array().unwrap().iter().find(|tool| tool["name"] == "walkthrough").unwrap().clone();
    let description = tool["description"].as_str().unwrap();
    assert!(description.contains("named sections") && description.contains("exactly one step as the key change"));
    assert!(tool["inputSchema"]["properties"]["steps"]["items"]["properties"]["icon"]["enum"].as_array().unwrap().contains(&json!("tests")));
}

async fn stored_id(server: &Server, conversation: Uuid) -> Uuid {
    latest(server, conversation).await["id"].as_str().unwrap().parse().unwrap()
}

#[tokio::test]
async fn read_steps_are_remembered_for_the_person_and_marking_twice_changes_nothing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    call(&server, &mut link, Some(conversation.id), steps()).await;
    let id = stored_id(&server, conversation.id).await;

    let (empty_status, empty) = server.user.json(server.user.get(&format!("/api/v1/walkthroughs/{id}/reads"))).await;
    assert_eq!((empty_status, empty), (200, json!([])));

    for step in [1, 1, 0] {
        let (status, _) = server.user.json(server.user.put(&format!("/api/v1/walkthroughs/{id}/reads/{step}"))).await;
        assert_eq!(status, 204);
    }

    let (_, reads) = server.user.json(server.user.get(&format!("/api/v1/walkthroughs/{id}/reads"))).await;
    assert_eq!(reads, json!([0, 1]));
}

#[tokio::test]
async fn a_step_the_walkthrough_does_not_have_is_refused_with_the_range_to_choose_from() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    call(&server, &mut link, Some(conversation.id), steps()).await;
    let id = stored_id(&server, conversation.id).await;

    let (status, body) = server.user.json(server.user.put(&format!("/api/v1/walkthroughs/{id}/reads/2"))).await;

    assert_eq!(status, 400);
    assert!(body["message"].as_str().unwrap().contains("Choose a step from 0 to 1"), "{body}");
}

#[tokio::test]
async fn only_the_owner_can_read_or_mark_a_walkthroughs_steps() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    call(&server, &mut link, Some(conversation.id), steps()).await;
    let id = stored_id(&server, conversation.id).await;
    let other = server.second_user("other@example.com").await;

    let (read_status, _) = other.json(other.get(&format!("/api/v1/walkthroughs/{id}/reads"))).await;
    let (mark_status, _) = other.json(other.put(&format!("/api/v1/walkthroughs/{id}/reads/0"))).await;

    assert_eq!((read_status, mark_status), (404, 404));
}

#[tokio::test]
async fn dismissing_a_walkthrough_hides_it_until_it_is_reopened_and_is_published_each_time() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    call(&server, &mut link, Some(conversation.id), steps()).await;
    let id = stored_id(&server, conversation.id).await;
    assert_eq!(latest(&server, conversation.id).await["dismissed"], false);
    let mut stream = server.user.stream().await;

    let (dismissed_status, _) = server.user.json(server.user.put(&format!("/api/v1/walkthroughs/{id}/dismissal"))).await;
    let dismissed = stream.until(|message| match message {
        mycelium_proto::stream::StreamMessage::Walkthrough(walkthrough) => Some(walkthrough.dismissed),
        _ => None,
    }).await;

    assert_eq!((dismissed_status, dismissed), (204, true));
    assert_eq!(latest(&server, conversation.id).await["dismissed"], true);

    let (reopened_status, _) = server.user.json(server.user.delete(&format!("/api/v1/walkthroughs/{id}/dismissal"))).await;

    assert_eq!(reopened_status, 204);
    assert_eq!(latest(&server, conversation.id).await["dismissed"], false);
}

#[tokio::test]
async fn a_new_walkthrough_is_shown_even_when_the_one_before_was_dismissed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    call(&server, &mut link, Some(conversation.id), steps()).await;
    let id = stored_id(&server, conversation.id).await;
    server.user.json(server.user.put(&format!("/api/v1/walkthroughs/{id}/dismissal"))).await;

    call(&server, &mut link, Some(conversation.id), steps()).await;

    assert_eq!(latest(&server, conversation.id).await["dismissed"], false);
}

#[tokio::test]
async fn only_the_owner_can_dismiss_a_walkthrough() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    call(&server, &mut link, Some(conversation.id), steps()).await;
    let id = stored_id(&server, conversation.id).await;
    let other = server.second_user("other@example.com").await;

    let (status, _) = other.json(other.put(&format!("/api/v1/walkthroughs/{id}/dismissal"))).await;

    assert_eq!(status, 404);
}

const SCREEN: &str = "diff --git a/src/Screen.tsx b/src/Screen.tsx
--- a/src/Screen.tsx
+++ b/src/Screen.tsx
@@ -1,3 +1,3 @@ import
 import { strings } from './strings'
-const title = \"Restore\"
+const title = strings.title
 export const Screen = 1
@@ -40,2 +40,3 @@ footer
 const footer = 1
+const caption = strings.caption
 export default Screen
diff --git a/src/strings.ts b/src/strings.ts
new file mode 100644
--- /dev/null
+++ b/src/strings.ts
@@ -0,0 +1 @@
+export const strings = {}
";

#[tokio::test]
async fn a_file_given_only_by_path_gets_its_real_hunks_from_the_nodes_diff() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    let arguments = json!({"steps": [{"title": "Everything", "files": [{"path": "src/Screen.tsx"}, {"path": "src/strings.ts"}]}]});

    let result = call_with_diff(&server, &mut link, Some(conversation.id), arguments, SCREEN).await;

    assert_eq!(result["isError"], false, "{result}");
    let files = latest(&server, conversation.id).await["steps"][0]["files"].clone();
    let screen = files[0]["diff"].as_str().unwrap();
    assert!(screen.starts_with("@@ -1,3 +1,3 @@ import\n import { strings }") && screen.contains("+const caption = strings.caption"), "{screen}");
    assert_eq!(files[1]["diff"], "@@ -0,0 +1 @@\n+export const strings = {}");
    assert_eq!(files[0]["unchanged"], false);
}

#[tokio::test]
async fn a_file_split_across_steps_gets_only_the_hunks_each_step_is_about() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    let arguments = json!({"steps": [
        {"title": "Title", "files": [{"path": "src/Screen.tsx", "diff": "-const title = \"Restore\"\n+const title = strings.title"}, {"path": "src/strings.ts"}]},
        {"title": "Footer", "files": [{"path": "src/Screen.tsx", "hunks": ["@@ -40,2 +40,3 @@", "+anything"]}]},
    ]});

    call_with_diff(&server, &mut link, Some(conversation.id), arguments, SCREEN).await;

    let steps = latest(&server, conversation.id).await["steps"].clone();
    let title = steps[0]["files"][0]["diff"].as_str().unwrap();
    let footer = steps[1]["files"][0]["diff"].as_str().unwrap();
    assert!(title.contains("+const title") && !title.contains("caption"), "{title}");
    assert!(footer.starts_with("@@ -40,2 +40,3 @@") && footer.contains("+const caption") && !footer.contains("+const title"), "{footer}");
}

#[tokio::test]
async fn a_file_with_no_change_in_the_scope_is_marked_unchanged_and_carries_no_diff() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    let arguments = json!({"steps": [{"title": "Screen", "files": [{"path": "src/Screen.tsx"}, {"path": "src/strings.ts"}, {"path": "src/untouched.ts", "diff": "+x"}]}]});

    call_with_diff(&server, &mut link, Some(conversation.id), arguments, SCREEN).await;

    let file = latest(&server, conversation.id).await["steps"][0]["files"][2].clone();
    assert_eq!((file["unchanged"].clone(), file["diff"].clone()), (json!(true), json!("")));
}

#[tokio::test]
async fn when_the_node_cannot_give_the_diff_the_walkthrough_is_stored_as_written_with_a_note() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    let anonymous = server.anonymous();
    let token = link.token.clone();
    let id = conversation.id;
    let asking = tokio::spawn(async move { call_raw_with(&anonymous, &token, Some(id), json!({"steps": [{"title": "t", "files": [{"path": "a.rs", "diff": "+a"}]}]})).await });

    loop {
        if let ControlToNode::Git { request_id, .. } = link.next().await {
            link.send(NodeToControl::GitResult { request_id, ok: false, output: json!({}), error: Some("not a repository".into()) }).await;
            break;
        }
    }

    assert_eq!(asking.await.unwrap()["isError"], false);
    let stored = latest(&server, conversation.id).await;
    assert_eq!(stored["steps"][0]["files"][0]["diff"], "+a");
    assert!(stored["note"].as_str().unwrap().contains("could not be read"), "{stored}");
}

#[tokio::test]
async fn a_file_without_a_diff_is_refused_when_the_node_cannot_give_one_either() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    let anonymous = server.anonymous();
    let token = link.token.clone();
    let id = conversation.id;
    let asking = tokio::spawn(async move { call_raw_with(&anonymous, &token, Some(id), json!({"steps": [{"title": "t", "files": [{"path": "a.rs"}]}]})).await });

    loop {
        if let ControlToNode::Git { request_id, .. } = link.next().await {
            link.send(NodeToControl::GitResult { request_id, ok: false, output: json!({}), error: Some("not a repository".into()) }).await;
            break;
        }
    }

    let result = asking.await.unwrap();
    assert_eq!(result["isError"], true);
    assert!(result["content"][0]["text"].as_str().unwrap().contains("a.rs in step 1 has no diff"));
}

#[tokio::test]
async fn the_scope_is_sent_to_the_node_and_kept_with_the_walkthrough() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, None).await;
    let anonymous = server.anonymous();
    let token = link.token.clone();
    let id = conversation.id;
    let asking = tokio::spawn(async move { call_raw_with(&anonymous, &token, Some(id), json!({"scope": "last_commit", "steps": [{"title": "t", "files": [{"path": "src/strings.ts"}]}]})).await });

    let asked = loop {
        if let ControlToNode::Git { request_id, command, .. } = link.next().await {
            link.send(NodeToControl::GitResult { request_id, ok: true, output: json!({"diff": SCREEN.split("diff --git a/src/strings.ts").nth(1).map(|rest| format!("diff --git a/src/strings.ts{rest}")).unwrap(), "truncated": false}), error: None }).await;
            break command;
        }
    };

    asking.await.unwrap();
    assert_eq!(asked, GitCommand::ScopeDiff { scope: "last_commit".into() });
    assert_eq!(latest(&server, conversation.id).await["scope"], "last_commit");
}
