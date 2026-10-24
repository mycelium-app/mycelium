mod common;
use common::{Server, eventually};
use serde_json::{Value, json};

async fn get(server: &Server, id: &str) -> Value {
    let (status, result) = server
        .user
        .json(server.user.get(&format!("/api/v1/workflows/{id}")))
        .await;
    assert_eq!(status, 200);
    result
}

async fn wait_status(server: &Server, id: &str, expected: &str) -> Value {
    eventually("workflow state", || async {
        let result = get(server, id).await;
        (result["status"] == expected).then_some(result)
    })
    .await
}

#[tokio::test]
async fn gates_release_dependencies_and_preserve_the_persons_decision() {
    let server = Server::start().await;
    let (status, workflow) = server.user.json(server.user.post("/api/v1/workflows").json(&json!({"title":"Release","steps":[{"key":"design","title":"Approve design","gate":true},{"key":"review","title":"Approve review","gate":true,"depends_on":["design"]}]}))).await;
    assert_eq!(status, 200, "{workflow}");
    let id = workflow["id"].as_str().unwrap();
    eventually("first gate", || async {
        let w = get(&server, id).await;
        (w["steps"][0]["status"] == "gate").then_some(())
    })
    .await;
    let workflow = get(&server, id).await;
    assert_eq!(workflow["steps"][1]["status"], "waiting");
    let (status, _) = server
        .user
        .json(
            server
                .user
                .post(&format!("/api/v1/workflows/{id}/control"))
                .json(&json!({"action":"approve","step":"design","output":"Use option B"})),
        )
        .await;
    assert_eq!(status, 200);
    eventually("dependent gate", || async {
        let w = get(&server, id).await;
        (w["steps"][1]["status"] == "gate").then_some(())
    })
    .await;
    let (_, _) = server
        .user
        .json(
            server
                .user
                .post(&format!("/api/v1/workflows/{id}/control"))
                .json(&json!({"action":"approve","step":"review","output":"Ready"})),
        )
        .await;
    let finished = wait_status(&server, id, "done").await;
    assert_eq!(finished["steps"][0]["output"], "Use option B");
    assert!(finished["output"].as_str().unwrap().contains("Ready"));
}

#[tokio::test]
async fn nested_workflows_wait_for_gates_and_stop_with_the_parent() {
    let server = Server::start().await;
    let (status,w)=server.user.json(server.user.post("/api/v1/workflows").json(&json!({"title":"Parent","steps":[{"key":"child","title":"Child","workflow":{"title":"Nested","steps":[{"key":"gate","title":"Choose","gate":true}]}}]}))).await;
    assert_eq!(status, 200, "{w}");
    let id = w["id"].as_str().unwrap();
    let child: String = eventually("nested workflow", || async {
        let w = get(&server, id).await;
        w["steps"][0]["workflow_id"].as_str().map(str::to_owned)
    })
    .await;
    assert_eq!(get(&server, &child).await["parent_id"], id);
    server
        .user
        .json(
            server
                .user
                .post(&format!("/api/v1/workflows/{id}/control"))
                .json(&json!({"action":"stop"})),
        )
        .await;
    assert_eq!(get(&server, id).await["status"], "stopped");
    assert_eq!(get(&server, &child).await["status"], "stopped");
}

#[tokio::test]
async fn cycles_are_rejected_before_any_work_is_started() {
    let server = Server::start().await;
    let (status,_)=server.user.json(server.user.post("/api/v1/workflows").json(&json!({"title":"Cycle","steps":[{"key":"a","title":"A","gate":true,"depends_on":["a"]}]}))).await;
    assert_eq!(status, 400);
    let (_, list) = server.user.json(server.user.get("/api/v1/workflows")).await;
    assert_eq!(list, json!([]));
}

#[tokio::test]
async fn another_owner_cannot_read_or_control_a_workflow() {
    let server = Server::start().await;
    let (_, w) = server
        .user
        .json(server.user.post("/api/v1/workflows").json(
            &json!({"title":"Private","steps":[{"key":"gate","title":"Choose","gate":true}]}),
        ))
        .await;
    let id = w["id"].as_str().unwrap();
    let other = server.second_user("other@example.test").await;
    let (status, _) = other
        .json(other.get(&format!("/api/v1/workflows/{id}")))
        .await;
    assert_eq!(status, 404);
    let (status, _) = other
        .json(
            other
                .post(&format!("/api/v1/workflows/{id}/control"))
                .json(&json!({"action":"stop"})),
        )
        .await;
    assert_eq!(status, 404);
    let (_, list) = other.json(other.get("/api/v1/workflows")).await;
    assert_eq!(list, json!([]));
}

#[tokio::test]
async fn a_native_agent_step_opens_an_ordinary_conversation_and_stops_it() {
    use mycelium_proto::link::ControlToNode;
    let server = Server::start().await;
    let mut link = server.online_node("worker").await;
    let (status,w)=server.user.json(server.user.post("/api/v1/workflows").json(&json!({"title":"Native agent","steps":[{"key":"work","title":"Build","prompt":"Implement the change","node_id":link.id,"agent":"codex","cwd":"/work/app","model":"test-model","difficulty":"medium"}]}))).await;
    assert_eq!(status, 200, "{w}");
    let id = w["id"].as_str().unwrap();
    let conversation = match link.next().await {
        ControlToNode::StartSession {
            conversation_id,
            cwd,
            agent,
            ..
        } => {
            assert_eq!(cwd, "/work/app");
            assert_eq!(agent, mycelium_proto::conversation::AgentId::Codex);
            conversation_id
        }
        other => panic!("expected start, got {other:?}"),
    };
    eventually("conversation linked", || async {
        let w = get(&server, id).await;
        (w["steps"][0]["conversation_id"] == json!(conversation)).then_some(())
    })
    .await;
    let (status, thread) = server
        .user
        .json(
            server
                .user
                .get(&format!("/api/v1/conversations/{conversation}")),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(thread["title"], "Build");
    server
        .user
        .json(
            server
                .user
                .post(&format!("/api/v1/workflows/{id}/control"))
                .json(&json!({"action":"stop"})),
        )
        .await;
    match link.next().await {
        ControlToNode::Cancel { conversation_id } => assert_eq!(conversation_id, conversation),
        other => panic!("expected cancel, got {other:?}"),
    }
}

#[tokio::test]
async fn an_ambiguous_dispatch_stops_instead_of_starting_duplicate_work() {
    let server = Server::start().await;
    let link = server.online_node("worker").await;
    let (_,w)=server.user.json(server.user.post("/api/v1/workflows").json(&json!({"title":"Interrupted","steps":[{"key":"gate","title":"Choose","gate":true},{"key":"work","title":"Build","prompt":"Build","node_id":link.id,"agent":"codex","cwd":"/work/app","depends_on":["gate"]}]}))).await;
    let id = w["id"].as_str().unwrap();
    let workflow: uuid::Uuid = id.parse().unwrap();
    sqlx::query("insert into workflow_dispatches(workflow_id,round,step_key,operation) values($1,1,'work','start')").bind(workflow).execute(&server.pool).await.unwrap();
    eventually("gate ready", || async {
        let w = get(&server, id).await;
        (w["steps"][0]["status"] == "gate").then_some(())
    })
    .await;
    server
        .user
        .json(
            server
                .user
                .post(&format!("/api/v1/workflows/{id}/control"))
                .json(&json!({"action":"approve","step":"gate"})),
        )
        .await;
    let failed = wait_status(&server, id, "failed").await;
    assert!(
        failed["output"]
            .as_str()
            .unwrap()
            .contains("avoid running the same work twice")
    );
    let count: i64 = sqlx::query_scalar("select count(*) from conversations where title='Build'")
        .fetch_one(&server.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn independent_steps_run_on_two_nodes_and_their_outputs_feed_a_dependent_step() {
    use mycelium_proto::link::NodeToControl;
    let server = Server::start().await;
    let mut build_node = server.online_node("build").await;
    let mut review_node = server.online_node("review").await;
    let (_, workflow) = server.user.json(server.user.post("/api/v1/workflows").json(&json!({"title":"Parallel review","steps":[
        {"key":"build","title":"Build","prompt":"Build it","node_id":build_node.id,"agent":"claude","cwd":"/work/app"},
        {"key":"audit","title":"Audit","prompt":"Audit it","node_id":review_node.id,"agent":"claude","cwd":"/work/app"},
        {"key":"review","title":"Review","prompt":"Review {{build.output}} and {{audit.output}}","depends_on":["build","audit"],"node_id":review_node.id,"agent":"claude","cwd":"/work/app"}
    ]}))).await;
    let id = workflow["id"].as_str().unwrap();
    let build = build_node.expect_start_session().await;
    let audit = review_node.expect_start_session().await;
    build_node.start_session(build, "build-session").await;
    review_node.start_session(audit, "audit-session").await;
    assert_eq!(build_node.expect_prompt().await.1, "Build it");
    assert_eq!(review_node.expect_prompt().await.1, "Audit it");
    for (node, conversation, output) in [
        (&mut build_node, build, "Implementation"),
        (&mut review_node, audit, "Findings"),
    ] {
        node.send(NodeToControl::AcpUpdate { conversation_id:conversation,seq:1,update:json!({"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":output}}}) }).await;
        node.send(NodeToControl::TurnEnd {
            conversation_id: conversation,
            seq: 2,
            stop_reason: "end_turn".into(),
            usage: None,
        })
        .await;
    }
    let review = review_node.expect_start_session().await;
    review_node.start_session(review, "review-session").await;
    assert_eq!(
        review_node.expect_prompt().await.1,
        "Review Implementation and Findings"
    );
    review_node.send(NodeToControl::AcpUpdate { conversation_id:review,seq:1,update:json!({"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Approved"}}}) }).await;
    review_node
        .send(NodeToControl::TurnEnd {
            conversation_id: review,
            seq: 2,
            stop_reason: "end_turn".into(),
            usage: None,
        })
        .await;
    let finished = wait_status(&server, id, "done").await;
    assert!(finished["output"].as_str().unwrap().contains("Approved"));
}

#[tokio::test]
async fn a_workflow_started_by_an_agent_returns_its_result_to_the_calling_conversation_once() {
    let server = Server::start().await;
    let mut node = server.online_node("laptop").await;
    let caller = server
        .create_conversation(node.id, Some("Delegate the review"))
        .await;
    node.expect_start_session().await;
    node.start_session(caller.id, "caller-session").await;
    node.expect_prompt().await;
    let anonymous = server.anonymous();
    let (_, response) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization",format!("Bearer {}",node.token)).header("x-mycelium-conversation",caller.id.to_string()).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"start_workflow","arguments":{"plan":{"title":"Review","steps":[{"key":"decision","title":"Approve","gate":true}]}}}}))).await;
    assert_eq!(response["result"]["isError"], false, "{response}");
    let (_, listed) = server.user.json(server.user.get("/api/v1/workflows")).await;
    let id = listed[0]["id"].as_str().unwrap();
    eventually("waiting gate", || async {
        (get(&server, id).await["steps"][0]["status"] == "gate").then_some(())
    })
    .await;
    server.user.json(server.user.post(&format!("/api/v1/workflows/{id}/control")).json(&json!({"action":"approve","step":"decision","output":"Approved the implementation"}))).await;
    wait_status(&server, id, "done").await;
    let queue = eventually("parent notification", || async {
        let (_, queue) = server
            .user
            .json(
                server
                    .user
                    .get(&format!("/api/v1/conversations/{}/queue", caller.id)),
            )
            .await;
        queue.as_array().filter(|queue| !queue.is_empty()).cloned()
    })
    .await;
    assert_eq!(queue.len(), 1);
    assert!(
        queue[0]["text"]
            .as_str()
            .unwrap()
            .contains("Approved the implementation")
    );
    let delivered = sqlx::query_scalar::<_,i64>("select count(*) from workflow_dispatches where workflow_id=$1 and operation='notify' and result is not null").bind(id.parse::<uuid::Uuid>().unwrap()).fetch_one(&server.pool).await.unwrap();
    assert_eq!(delivered, 1);
}
