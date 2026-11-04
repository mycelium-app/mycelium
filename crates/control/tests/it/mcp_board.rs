use crate::common::{NodeLink, Server};
use mycelium_proto::link::{ControlToNode, GitCommand, NodeToControl};
use serde_json::{Value, json};
use uuid::Uuid;

struct Rig {
    server: Server,
    link: NodeLink,
    token: String,
    conversation: Uuid,
}

async fn rig() -> Rig {
    let server = Server::start().await;
    let (link, token) = server.online_node_with_token("laptop", "v0.11.0").await;
    let conversation = server.create_conversation(link.id, None).await.id;

    Rig { server, link, token, conversation }
}

fn tool(name: &str, arguments: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}})
}

fn text(reply: &Value) -> String {
    reply["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

fn spawn_call(rig: &Rig, name: &str, arguments: Value) -> tokio::task::JoinHandle<Value> {
    let anonymous = rig.server.anonymous();
    let request = anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {}", rig.token)).header("x-mycelium-conversation", rig.conversation.to_string()).json(&tool(name, arguments));

    tokio::spawn(async move { anonymous.json(request).await.1 })
}

impl Rig {
    async fn call(&self, name: &str, arguments: Value) -> Value {
        self.server.agent_mcp(&self.token, self.conversation, tool(name, arguments)).await
    }

    async fn call_answering(&self, name: &str, arguments: Value, option: &str) -> (Value, Value) {
        let asking = spawn_call(self, name, arguments);
        let request = self.server.open_request(self.conversation).await;
        self.server.answer(self.conversation, &request, option).await;

        (asking.await.unwrap(), request)
    }

    async fn propose(&self, title: &str) -> Uuid {
        let reply = self.call("propose_task", json!({"title": title})).await;

        text(&reply).split_whitespace().nth(1).unwrap().parse().unwrap()
    }

    async fn status(&self, id: Uuid) -> Value {
        let (_, tasks) = self.server.user.json(self.server.user.get("/api/v1/tasks")).await;

        tasks.as_array().unwrap().iter().find(|task| task["id"] == json!(id)).unwrap()["status"].clone()
    }
}

#[tokio::test]
async fn list_tasks_shows_each_items_column_age_and_source_without_asking() {
    let rig = rig().await;
    rig.propose("Tidy the release notes").await;

    let listed = text(&rig.call("list_tasks", json!({})).await);

    assert!(listed.contains("[proposed] Tidy the release notes (today, from"), "{listed}");
    assert!(rig.server.events(rig.conversation).await.iter().all(|event| event["kind"] != "permission_request"));
}

#[tokio::test]
async fn closing_tasks_asks_once_for_the_whole_batch_and_closed_tasks_leave_the_board() {
    let rig = rig().await;
    let first = rig.propose("Old idea").await;
    let second = rig.propose("Older idea").await;

    let (reply, request) = rig.call_answering("close_tasks", json!({"ids": [first, second], "reason": "stale"}), "approve").await;

    assert_eq!(request["toolCall"]["rawInput"]["close"], json!(["Old idea", "Older idea"]));
    assert_eq!(request["options"].as_array().unwrap().len(), 2);
    assert_eq!(text(&reply), "Closed 2 tasks.");
    assert_eq!((rig.status(first).await, rig.status(second).await), (json!("closed"), json!("closed")));
    assert_eq!(text(&rig.call("list_tasks", json!({})).await), "No tasks match.");
}

#[tokio::test]
async fn a_declined_card_closes_nothing() {
    let rig = rig().await;
    let id = rig.propose("Keep me").await;

    let (reply, _) = rig.call_answering("close_tasks", json!({"ids": [id]}), "deny").await;

    assert_eq!(reply["result"]["isError"], true);
    assert_eq!(rig.status(id).await, json!("proposed"));
}

#[tokio::test]
async fn approving_and_declining_a_proposed_task_go_through_the_card() {
    let rig = rig().await;
    let approved = rig.propose("Ship it").await;
    let declined = rig.propose("Skip it").await;

    rig.call_answering("approve_task", json!({"id": approved}), "approve").await;
    rig.call_answering("decline_task", json!({"id": declined}), "approve").await;

    assert_eq!((rig.status(approved).await, rig.status(declined).await), (json!("open"), json!("declined")));
}

#[tokio::test]
async fn approving_a_task_that_is_not_proposed_is_refused_without_a_card() {
    let rig = rig().await;
    let id = rig.propose("Ship it").await;
    rig.call_answering("approve_task", json!({"id": id}), "approve").await;

    let refused = rig.call("approve_task", json!({"id": id})).await;

    assert!(text(&refused).contains("only proposed tasks are approved or declined"), "{}", text(&refused));
    assert_eq!(rig.server.events(rig.conversation).await.iter().filter(|event| event["kind"] == "permission_request").count(), 1);
}

#[tokio::test]
async fn create_project_registers_a_folder_after_the_person_approves() {
    let mut rig = rig().await;
    let asking = spawn_call(&rig, "create_project", json!({"node": "laptop", "path": "~/Projects/slop/platform"}));
    let request = rig.server.open_request(rig.conversation).await;
    rig.server.answer(rig.conversation, &request, "approve").await;

    let (request_id, cwd) = loop {
        match rig.link.next().await {
            ControlToNode::Git { request_id, cwd, command: GitCommand::Register } => break (request_id, cwd),
            _ => continue,
        }
    };
    rig.link.send(NodeToControl::GitResult { request_id, ok: true, output: json!({"path": "/home/sam/Projects/slop/platform", "repository": true}), error: None }).await;
    let reply = asking.await.unwrap();

    assert_eq!(cwd, "~/Projects/slop/platform");
    assert!(text(&reply).starts_with("Added the project platform"), "{}", text(&reply));
    let (_, projects) = rig.server.user.json(rig.server.user.get("/api/v1/projects")).await;
    assert_eq!(projects[0]["path"], "/home/sam/Projects/slop/platform");
}

#[tokio::test]
async fn create_project_on_an_unknown_node_is_refused_without_a_card() {
    let rig = rig().await;

    let refused = rig.call("create_project", json!({"node": "nowhere", "path": "~/x"})).await;

    assert!(text(&refused).contains("None of the person's nodes is nowhere"), "{}", text(&refused));
}

#[tokio::test]
async fn update_project_declined_changes_nothing() {
    let rig = rig().await;
    let owner: Uuid = sqlx::query_scalar("select id from users order by created_at limit 1").fetch_one(&rig.server.pool).await.unwrap();
    sqlx::query("insert into projects (owner, node_id, name, path) values ($1, $2, 'platform', '/home/sam/platform')").bind(owner).bind(rig.link.id).execute(&rig.server.pool).await.unwrap();

    let (reply, _) = rig.call_answering("update_project", json!({"project": "platform", "test_command": "cargo test"}), "deny").await;

    assert_eq!(reply["result"]["isError"], true);
    let command: Option<String> = sqlx::query_scalar("select test_command from projects where name = 'platform'").fetch_one(&rig.server.pool).await.unwrap();
    assert_eq!(command, None);
}
