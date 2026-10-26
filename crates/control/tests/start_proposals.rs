mod common;

use common::{NodeLink, Server, eventually};
use serde_json::{Value, json};
use uuid::Uuid;

async fn call(server: &Server, link: &NodeLink, conversation: Uuid, tool: &str, arguments: Value) -> Value {
    let anonymous = server.anonymous();
    let request = anonymous
        .post("/api/v1/mcp")
        .header("authorization", format!("Bearer {}", link.token))
        .header("x-mycelium-conversation", conversation.to_string())
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": tool, "arguments": arguments}}));
    let (status, body) = anonymous.json(request).await;
    assert_eq!(status, 200, "{body}");

    body["result"].clone()
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_string()
}

async fn caller(server: &Server, link: &mut NodeLink) -> Uuid {
    let conversation = server.create_conversation(link.id, Some("Plan the release")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, &format!("acp-{}", conversation.id)).await;
    link.expect_prompt().await;

    conversation.id
}

async fn sign_in(server: &Server, node: Uuid, account: &str) {
    sqlx::query("insert into logins (node_id, agent, account, method, state) values ($1, 'claude', $2, 'oauth', 'signed_in')").bind(node).bind(account).execute(&server.pool).await.unwrap();
}

async fn proposals(server: &Server) -> Vec<Value> {
    let (_, proposals) = server.user.json(server.user.get("/api/v1/start-proposals")).await;

    proposals.as_array().unwrap().clone()
}

async fn workflows(server: &Server) -> Vec<Value> {
    let (_, workflows) = server.user.json(server.user.get("/api/v1/workflows")).await;

    workflows.as_array().unwrap().clone()
}

fn subagent(node: &str) -> Value {
    json!({"step": {"title": "Review the schema", "prompt": "Read the schema", "node": node, "agent": "claude", "cwd": "/tmp"}})
}

#[tokio::test]
async fn start_subagent_proposes_and_creates_nothing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = caller(&server, &mut link).await;

    let result = call(&server, &link, conversation, "start_subagent", subagent("laptop")).await;

    assert_eq!(result["isError"], false, "{result}");
    assert!(text(&result).contains("Waiting for the person to approve"), "{}", text(&result));
    assert!(workflows(&server).await.is_empty());
    let proposals = proposals(&server).await;
    assert_eq!((proposals.len(), proposals[0]["state"].clone(), proposals[0]["steps"][0]["node"]["name"].clone(), proposals[0]["steps"][0]["node"]["online"].clone()), (1, json!("proposed"), json!("laptop"), json!(true)));
    assert_eq!(proposals[0]["steps"][0]["first_message"], "Read the schema");
}

#[tokio::test]
async fn approving_starts_the_workflow() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = caller(&server, &mut link).await;
    call(&server, &link, conversation, "start_subagent", subagent("laptop")).await;
    let id = proposals(&server).await[0]["id"].as_str().unwrap().to_string();

    let (status, approved) = server.user.json(server.user.post(&format!("/api/v1/start-proposals/{id}/approve")).json(&json!({}))).await;

    assert_eq!((status, approved["state"].clone()), (200, json!("started")), "{approved}");
    assert_eq!(workflows(&server).await.len(), 1);
    assert_eq!(approved["workflow_id"], workflows(&server).await[0]["id"]);
    link.expect_start_session().await;
}

#[tokio::test]
async fn create_and_connect_connects_the_caller_to_every_step_conversation() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = caller(&server, &mut link).await;
    call(&server, &link, conversation, "start_subagent", subagent("laptop")).await;
    let id = proposals(&server).await[0]["id"].as_str().unwrap().to_string();

    let (status, approved) = server.user.json(server.user.post(&format!("/api/v1/start-proposals/{id}/connect")).json(&json!({}))).await;
    assert_eq!((status, approved["connect"].clone()), (200, json!(true)), "{approved}");
    link.expect_start_session().await;

    let connected = eventually("connection to the step conversation", || async {
        let (_, connections) = server.user.json(server.user.get("/api/v1/conversation-connections")).await;
        connections.as_array().unwrap().iter().find(|connection| connection["source_id"] == json!(conversation) || connection["target_id"] == json!(conversation)).cloned()
    })
    .await;
    let titles = [connected["source_title"].clone(), connected["target_title"].clone()];
    assert!(titles.contains(&json!("Review the schema")), "{connected}");
}

#[tokio::test]
async fn declining_tells_the_caller_and_starts_nothing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = caller(&server, &mut link).await;
    call(&server, &link, conversation, "start_subagent", subagent("laptop")).await;
    let id = proposals(&server).await[0]["id"].as_str().unwrap().to_string();

    let (status, declined) = server.user.json(server.user.post(&format!("/api/v1/start-proposals/{id}/decline"))).await;

    assert_eq!((status, declined["state"].clone()), (200, json!("declined")), "{declined}");
    assert!(workflows(&server).await.is_empty());
    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/queue"))).await;
    assert!(queue[0]["text"].as_str().unwrap().contains("I declined starting Review the schema"), "{queue}");
}

#[tokio::test]
async fn ambiguous_account_needs_a_choice_before_starting() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = caller(&server, &mut link).await;
    sign_in(&server, link.id, "a@example.com").await;
    sign_in(&server, link.id, "b@example.com").await;
    call(&server, &link, conversation, "start_subagent", subagent("laptop")).await;
    let proposal = proposals(&server).await[0].clone();
    let id = proposal["id"].as_str().unwrap();
    assert_eq!(proposal["steps"][0]["account_choices"], json!(["a@example.com", "b@example.com"]));

    let (status, refused) = server.user.json(server.user.post(&format!("/api/v1/start-proposals/{id}/approve")).json(&json!({}))).await;
    assert_eq!((status, refused["error"].clone()), (409, json!("choice_needed")), "{refused}");
    assert!(workflows(&server).await.is_empty());

    let (status, approved) = server.user.json(server.user.post(&format!("/api/v1/start-proposals/{id}/approve")).json(&json!({"choices": {"subagent": {"account": "b@example.com"}}}))).await;
    assert_eq!((status, approved["state"].clone()), (200, json!("started")), "{approved}");
    assert_eq!(workflows(&server).await[0]["plan"]["steps"][0]["account"], "b@example.com");
}

#[tokio::test]
async fn a_single_signed_in_account_is_filled_in() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = caller(&server, &mut link).await;
    sign_in(&server, link.id, "only@example.com").await;

    call(&server, &link, conversation, "start_subagent", subagent("laptop")).await;

    assert_eq!(proposals(&server).await[0]["steps"][0]["account"], "only@example.com");
}

#[tokio::test]
async fn unknown_node_name_returns_an_error_to_the_agent() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = caller(&server, &mut link).await;

    let result = call(&server, &link, conversation, "start_subagent", subagent("desktop")).await;

    assert_eq!(result["isError"], true);
    assert!(text(&result).contains("None of the person's nodes is named desktop"), "{}", text(&result));
    assert!(proposals(&server).await.is_empty());
}

#[tokio::test]
async fn a_call_from_no_conversation_proposes_nothing() {
    let server = Server::start().await;
    let (_, token) = server.join("node", "laptop").await;
    let anonymous = server.anonymous();
    let request = anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "start_subagent", "arguments": {"step": {"title": "Pause", "gate": true}}}}));
    let (_, body) = anonymous.json(request).await;

    assert_eq!(body["result"]["isError"], true, "{body}");
    assert!(workflows(&server).await.is_empty());
}

#[tokio::test]
async fn start_workflow_proposal_lists_every_step() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = caller(&server, &mut link).await;
    let plan = json!({"title": "Release", "steps": [
        {"key": "build", "title": "Build", "prompt": "Build it", "node": "laptop", "agent": "claude", "cwd": "/tmp"},
        {"key": "sign_off", "title": "Sign off", "gate": true, "depends_on": ["build"]},
    ]});

    call(&server, &link, conversation, "start_workflow", json!({"plan": plan})).await;

    let proposal = proposals(&server).await[0].clone();
    let keys: Vec<&str> = proposal["steps"].as_array().unwrap().iter().map(|step| step["key"].as_str().unwrap()).collect();
    assert_eq!((proposal["kind"].clone(), proposal["title"].clone(), keys), (json!("workflow"), json!("Release"), vec!["build", "sign_off"]));
    assert_eq!(proposal["steps"][1]["depends_on"], json!(["build"]));
    assert!(workflows(&server).await.is_empty());
}

#[tokio::test]
async fn deciding_twice_conflicts() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = caller(&server, &mut link).await;
    call(&server, &link, conversation, "start_subagent", subagent("laptop")).await;
    let id = proposals(&server).await[0]["id"].as_str().unwrap().to_string();
    server.user.json(server.user.post(&format!("/api/v1/start-proposals/{id}/decline"))).await;

    let (status, again) = server.user.json(server.user.post(&format!("/api/v1/start-proposals/{id}/approve")).json(&json!({}))).await;

    assert_eq!((status, again["error"].clone()), (409, json!("proposal_not_proposed")), "{again}");
}

#[tokio::test]
async fn proposals_belong_to_their_owner() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = caller(&server, &mut link).await;
    call(&server, &link, conversation, "start_subagent", subagent("laptop")).await;
    let id = proposals(&server).await[0]["id"].as_str().unwrap().to_string();
    let other = server.second_user("other@example.com").await;

    let (status, _) = other.json(other.post(&format!("/api/v1/start-proposals/{id}/approve")).json(&json!({}))).await;

    assert_eq!(status, 404);
}

#[tokio::test]
async fn list_places_reports_online_nodes_and_signed_in_accounts() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = caller(&server, &mut link).await;
    let (_, _) = server.join("node", "desktop").await;
    sign_in(&server, link.id, "a@example.com").await;

    let result = call(&server, &link, conversation, "list_places", json!({})).await;

    let places: Value = serde_json::from_str(&text(&result)).unwrap();
    let nodes = places["nodes"].as_array().unwrap();
    let laptop = nodes.iter().find(|node| node["name"] == "laptop").unwrap();
    let desktop = nodes.iter().find(|node| node["name"] == "desktop").unwrap();
    assert_eq!((laptop["online"].clone(), desktop["online"].clone()), (json!(true), json!(false)));
    assert_eq!(laptop["signed_in_accounts"], json!([{"agent": "claude", "account": "a@example.com"}]));
}
