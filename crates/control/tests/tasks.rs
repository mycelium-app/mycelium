mod common;

use common::Server;
use mycelium_proto::link::ControlToNode;
use serde_json::{Value, json};
use uuid::Uuid;

async fn add_task(server: &Server, node_id: Uuid, body: Value) -> (u16, Value) {
    let mut body = body;
    body["node_id"] = json!(node_id);
    body["agent"] = json!("claude");

    server.user.json(server.user.post("/api/v1/tasks").json(&body)).await
}

async fn open_tasks(server: &Server) -> Vec<Value> {
    let (_, tasks) = server.user.json(server.user.get("/api/v1/tasks")).await;

    tasks.as_array().unwrap().iter().filter(|task| task["status"] == "open").cloned().collect()
}

async fn tasks_with_status(server: &Server, status: &str) -> Vec<Value> {
    let (_, tasks) = server.user.json(server.user.get("/api/v1/tasks")).await;

    tasks.as_array().unwrap().iter().filter(|task| task["status"] == status).cloned().collect()
}

async fn tool(server: &Server, token: &str, conversation: Option<Uuid>, name: &str, arguments: Value) -> Value {
    let anonymous = server.anonymous();
    let mut request = anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}}));

    if let Some(conversation) = conversation {
        request = request.header("x-mycelium-conversation", conversation.to_string());
    }

    let (status, body) = anonymous.json(request).await;
    assert_eq!(status, 200, "{body}");

    body["result"].clone()
}

async fn asking_conversation(server: &Server, link: &mut common::NodeLink) -> Uuid {
    let conversation = server.create_conversation(link.id, Some("Plan the release")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, "acp-1").await;
    link.expect_prompt().await;

    conversation.id
}

fn path_of(task: &Value, action: &str) -> String {
    format!("/api/v1/tasks/{}/{action}", task["id"].as_str().unwrap())
}

#[tokio::test]
async fn a_task_waits_in_the_backlog_until_it_is_started() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;

    let (status, task) = add_task(&server, link.id, json!({"title": "Rotate the S3 keys", "cwd": "/work/app"})).await;
    assert_eq!(status, 201, "{task}");

    let tasks = open_tasks(&server).await;
    assert_eq!((tasks.len(), tasks[0]["title"].clone(), tasks[0]["cwd"].clone()), (1, json!("Rotate the S3 keys"), json!("/work/app")));
}

#[tokio::test]
async fn starting_a_task_starts_a_conversation_with_its_title_and_prompt_and_leaves_the_backlog() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (_, task) = add_task(&server, link.id, json!({"title": "Rebuild install.md", "cwd": "/work/app", "prompt": "Rebuild install.md for the new flags"})).await;

    let (status, conversation) = server.user.json(server.user.post(&format!("/api/v1/tasks/{}/start", task["id"].as_str().unwrap()))).await;
    assert_eq!((status, conversation["title"].clone(), conversation["cwd"].clone(), conversation["status"].clone()), (201, json!("Rebuild install.md"), json!("/work/app"), json!("working")));

    match link.next().await {
        ControlToNode::StartSession { cwd, .. } => assert_eq!(cwd, "/work/app"),
        other => panic!("expected start_session, got {other:?}"),
    }

    assert_eq!(open_tasks(&server).await.len(), 0);

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/tasks/{}/start", task["id"].as_str().unwrap()))).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn a_task_without_a_prompt_starts_with_its_title_as_the_prompt() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (_, task) = add_task(&server, link.id, json!({"title": "Verify v0.1.11 from S3", "cwd": "/work/app"})).await;

    let (_, conversation) = server.user.json(server.user.post(&format!("/api/v1/tasks/{}/start", task["id"].as_str().unwrap()))).await;
    link.expect_start_session().await;

    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{}/queue", conversation["id"].as_str().unwrap()))).await;
    assert_eq!(queue[0]["text"], json!("Verify v0.1.11 from S3"));
}

#[tokio::test]
async fn a_task_on_an_offline_node_stays_in_the_backlog_with_a_reason() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let (_, task) = add_task(&server, link.id, json!({"title": "Tidy", "cwd": "/work/app"})).await;
    drop(link);

    common::eventually("the node going offline", || async {
        let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;

        (nodes[0]["online"] == false).then_some(())
    })
    .await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/tasks/{}/start", task["id"].as_str().unwrap()))).await;
    assert_eq!((status, body["error"].clone()), (409, json!("node_offline")));
    assert_eq!(open_tasks(&server).await.len(), 1);
}

#[tokio::test]
async fn a_task_needs_a_title_a_folder_or_project_and_a_project_on_its_node() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let other = server.online_node("desktop").await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    let project = sqlx::query_scalar::<_, Uuid>("insert into projects (owner, node_id, name, path) values ($1, $2, 'app', '/work/app') returning id").bind(owner).bind(link.id).fetch_one(&server.pool).await.unwrap();

    let (status, body) = add_task(&server, link.id, json!({"title": "  ", "cwd": "/work/app"})).await;
    assert_eq!((status, body["error"].clone()), (400, json!("title_required")));

    let (status, body) = add_task(&server, link.id, json!({"title": "Tidy"})).await;
    assert_eq!((status, body["error"].clone()), (400, json!("cwd_required")));

    let (status, body) = add_task(&server, other.id, json!({"title": "Tidy", "project_id": project})).await;
    assert_eq!((status, body["error"].clone()), (400, json!("project_not_on_node")));

    let (status, body) = add_task(&server, link.id, json!({"title": "Tidy", "cwd": "/work/app", "branch": "tidy"})).await;
    assert_eq!((status, body["error"].clone()), (400, json!("project_required")));

    let (status, task) = add_task(&server, link.id, json!({"title": "Tidy", "project_id": project, "branch": "tidy"})).await;
    assert_eq!((status, task["project_id"].clone(), task["branch"].clone()), (201, json!(project), json!("tidy")));
}

#[tokio::test]
async fn a_task_can_be_deleted_and_only_by_its_owner() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;
    let (_, task) = add_task(&server, link.id, json!({"title": "Tidy", "cwd": "/work/app"})).await;
    let path = format!("/api/v1/tasks/{}", task["id"].as_str().unwrap());
    let other = server.second_user("other@example.com").await;

    let (status, _) = other.json(other.delete(&path)).await;
    assert_eq!(status, 404);
    let (status, _) = other.json(other.post(&format!("{path}/start"))).await;
    assert_eq!(status, 404);
    let (_, theirs) = other.json(other.get("/api/v1/tasks")).await;
    assert_eq!(theirs, json!([]));

    let (status, _) = server.user.json(server.user.delete(&path)).await;
    assert_eq!(status, 204);
    assert_eq!(open_tasks(&server).await.len(), 0);
}

#[tokio::test]
async fn an_agent_proposes_a_task_that_waits_as_proposed_with_its_conversation_as_the_source() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = asking_conversation(&server, &mut link).await;

    let result = tool(&server, &link.token, Some(conversation), "propose_task", json!({"title": "Cache cargo in the release jobs", "body": "Add a cargo cache to both release jobs"})).await;
    assert_eq!(result["isError"], false, "{result}");

    let proposed = tasks_with_status(&server, "proposed").await;
    assert_eq!(proposed.len(), 1);
    assert_eq!((proposed[0]["title"].clone(), proposed[0]["prompt"].clone(), proposed[0]["source_title"].clone(), proposed[0]["cwd"].clone()), (json!("Cache cargo in the release jobs"), json!("Add a cargo cache to both release jobs"), json!("Plan the release"), json!("/work/app")));
    assert_eq!(open_tasks(&server).await.len(), 0);

    let (status, _) = server.user.json(server.user.post(&path_of(&proposed[0], "start"))).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn a_proposed_task_belongs_to_the_named_project_and_an_unknown_project_is_refused() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = asking_conversation(&server, &mut link).await;
    let owner = sqlx::query_scalar::<_, Uuid>("select id from users limit 1").fetch_one(&server.pool).await.unwrap();
    let project = sqlx::query_scalar::<_, Uuid>("insert into projects (owner, node_id, name, path) values ($1, $2, 'mycelium', '/work/mycelium') returning id").bind(owner).bind(link.id).fetch_one(&server.pool).await.unwrap();

    let result = tool(&server, &link.token, Some(conversation), "propose_task", json!({"title": "Tidy", "project": "Mycelium"})).await;
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(tasks_with_status(&server, "proposed").await[0]["project_id"], json!(project));

    let refused = tool(&server, &link.token, Some(conversation), "propose_task", json!({"title": "Tidy", "project": "nowhere"})).await;
    assert_eq!(refused["isError"], true);
    assert_eq!(tasks_with_status(&server, "proposed").await.len(), 1);
}

#[tokio::test]
async fn proposing_or_asking_needs_a_title_and_a_conversation() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = asking_conversation(&server, &mut link).await;

    let outside = tool(&server, &link.token, None, "propose_task", json!({"title": "Tidy"})).await;
    assert_eq!(outside["isError"], true);

    let blank = tool(&server, &link.token, Some(conversation), "ask_question", json!({"question": "  "})).await;
    assert_eq!(blank["isError"], true);

    let (_, listed) = server.user.json(server.user.get("/api/v1/tasks")).await;
    assert_eq!(listed, json!([]));
}

#[tokio::test]
async fn approving_a_proposed_task_opens_it_and_declining_hides_it_as_declined() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = asking_conversation(&server, &mut link).await;
    tool(&server, &link.token, Some(conversation), "propose_task", json!({"title": "Keep"})).await;
    tool(&server, &link.token, Some(conversation), "propose_task", json!({"title": "Drop"})).await;
    let proposed = tasks_with_status(&server, "proposed").await;

    let (status, approved) = server.user.json(server.user.post(&path_of(&proposed[0], "approve"))).await;
    assert_eq!((status, approved["status"].clone()), (200, json!("open")));
    let (status, declined) = server.user.json(server.user.post(&path_of(&proposed[1], "decline"))).await;
    assert_eq!((status, declined["status"].clone()), (200, json!("declined")));

    let (status, _) = server.user.json(server.user.post(&path_of(&proposed[0], "decline"))).await;
    assert_eq!(status, 404);
    let other = server.second_user("other@example.com").await;
    let (status, _) = other.json(other.post(&path_of(&proposed[1], "approve"))).await;
    assert_eq!(status, 404);
    assert_eq!(open_tasks(&server).await.len(), 1);
}

#[tokio::test]
async fn an_agent_asks_a_question_and_the_answer_reaches_its_conversation_as_a_prompt() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = asking_conversation(&server, &mut link).await;
    let result = tool(&server, &link.token, Some(conversation), "ask_question", json!({"question": "Tag now, or wait?", "options": ["Tag now", "Wait"]})).await;
    assert_eq!(result["isError"], false, "{result}");
    link.send(mycelium_proto::link::NodeToControl::TurnEnd { conversation_id: conversation, seq: 1, stop_reason: "end_turn".into(), usage: None }).await;

    let questions = tasks_with_status(&server, "question").await;
    assert_eq!((questions[0]["title"].clone(), questions[0]["options"].clone(), questions[0]["source_title"].clone()), (json!("Tag now, or wait?"), json!(["Tag now", "Wait"]), json!("Plan the release")));

    let (status, answered) = server.user.json(server.user.post(&path_of(&questions[0], "answer")).json(&json!({"answer": "Wait"}))).await;
    assert_eq!((status, answered["status"].clone(), answered["answer"].clone()), (200, json!("done"), json!("Wait")));

    let (_, text) = link.expect_prompt().await;
    assert_eq!(text, "Answer to your question \"Tag now, or wait?\": Wait");

    let (status, _) = server.user.json(server.user.post(&path_of(&questions[0], "answer")).json(&json!({"answer": "Again"}))).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn an_empty_answer_is_refused_and_the_question_stays_open() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = asking_conversation(&server, &mut link).await;
    tool(&server, &link.token, Some(conversation), "ask_question", json!({"question": "Tag now?"})).await;
    let questions = tasks_with_status(&server, "question").await;

    let (status, body) = server.user.json(server.user.post(&path_of(&questions[0], "answer")).json(&json!({"answer": "  "}))).await;

    assert_eq!((status, body["error"].clone()), (400, json!("answer_required")));
    assert_eq!(tasks_with_status(&server, "question").await.len(), 1);
}

#[tokio::test]
async fn a_question_whose_conversation_is_gone_keeps_waiting_with_a_reason() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = asking_conversation(&server, &mut link).await;
    tool(&server, &link.token, Some(conversation), "ask_question", json!({"question": "Tag now?"})).await;
    sqlx::query("update tasks set source_id = null").execute(&server.pool).await.unwrap();
    let questions = tasks_with_status(&server, "question").await;

    let (status, body) = server.user.json(server.user.post(&path_of(&questions[0], "answer")).json(&json!({"answer": "Yes"}))).await;

    assert_eq!((status, body["error"].clone()), (409, json!("asker_gone")));
    assert_eq!(tasks_with_status(&server, "question").await.len(), 1);
}

#[tokio::test]
async fn a_started_task_is_claimed_by_its_conversation_and_can_then_be_marked_done() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (_, task) = add_task(&server, link.id, json!({"title": "Tidy", "cwd": "/work/app"})).await;

    let (status, _) = server.user.json(server.user.post(&path_of(&task, "done"))).await;
    assert_eq!(status, 404);

    let (_, conversation) = server.user.json(server.user.post(&path_of(&task, "start"))).await;
    link.expect_start_session().await;

    let claimed = tasks_with_status(&server, "claimed").await;
    assert_eq!((claimed.len(), claimed[0]["conversation_id"].clone()), (1, conversation["id"].clone()));

    let (status, done) = server.user.json(server.user.post(&path_of(&task, "done"))).await;
    assert_eq!((status, done["status"].clone()), (200, json!("done")));
}

async fn task_by_id(server: &Server, id: &Value) -> Value {
    let (_, tasks) = server.user.json(server.user.get("/api/v1/tasks")).await;

    tasks.as_array().unwrap().iter().find(|task| task["id"] == *id).cloned().unwrap()
}

#[tokio::test]
async fn a_task_that_waits_on_an_unfinished_task_cannot_be_started_until_it_is_done() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (_, first) = add_task(&server, link.id, json!({"title": "Write the migration", "cwd": "/work/app"})).await;
    let (status, second) = add_task(&server, link.id, json!({"title": "Backfill", "cwd": "/work/app", "depends_on": [first["id"]]})).await;
    assert_eq!((status, second["waits_on"].clone(), second["depends_on"].clone()), (201, json!(1), json!([first["id"]])));

    let (status, body) = server.user.json(server.user.post(&path_of(&second, "start"))).await;
    assert_eq!((status, body["error"].clone(), body["message"].clone()), (409, json!("task_waiting"), json!("This task waits on 1 other task. Finish it first.")));
    assert_eq!(task_by_id(&server, &second["id"]).await["status"], json!("open"));

    let (status, _) = server.user.json(server.user.post(&path_of(&first, "start"))).await;
    assert_eq!(status, 201);
    link.expect_start_session().await;
    let (status, _) = server.user.json(server.user.post(&path_of(&first, "done"))).await;
    assert_eq!(status, 200);

    assert_eq!(task_by_id(&server, &second["id"]).await["waits_on"], json!(0));
    let (status, _) = server.user.json(server.user.post(&path_of(&second, "start"))).await;
    assert_eq!(status, 201);
    link.expect_start_session().await;
}

#[tokio::test]
async fn a_task_can_only_wait_on_open_or_started_tasks_of_its_owner() {
    let server = Server::start().await;
    let link = server.online_node("laptop").await;

    let (status, body) = add_task(&server, link.id, json!({"title": "Backfill", "cwd": "/work/app", "depends_on": [Uuid::new_v4()]})).await;
    assert_eq!((status, body["error"].clone()), (400, json!("dependency_not_found")));
}

#[tokio::test]
async fn a_proposed_task_can_wait_on_other_tasks_through_the_mcp_tool() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = asking_conversation(&server, &mut link).await;
    let (_, first) = add_task(&server, link.id, json!({"title": "Write the migration", "cwd": "/work/app"})).await;

    let result = tool(&server, &link.token, Some(conversation), "propose_task", json!({"title": "Backfill", "depends_on": [first["id"]]})).await;
    assert_eq!(result["isError"], json!(false), "{result}");
    let proposed = &tasks_with_status(&server, "proposed").await[0];
    assert_eq!((proposed["depends_on"].clone(), proposed["waits_on"].clone()), (json!([first["id"]]), json!(1)));

    let refused = tool(&server, &link.token, Some(conversation), "propose_task", json!({"title": "Tidy", "depends_on": [Uuid::new_v4()]})).await;
    assert_eq!(refused["isError"], json!(true));
    assert_eq!(tasks_with_status(&server, "proposed").await.len(), 1);
}
