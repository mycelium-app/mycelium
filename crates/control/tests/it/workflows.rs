use crate::common::{Server, eventually};
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
    let (_, proposals) = server.user.json(server.user.get("/api/v1/start-proposals")).await;
    let proposal = proposals[0]["id"].as_str().unwrap();
    server.user.json(server.user.post(&format!("/api/v1/start-proposals/{proposal}/approve")).json(&json!({}))).await;
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
    let workflow = id.parse::<uuid::Uuid>().unwrap();
    let delivered = eventually("notify recorded", || async {
        let count = sqlx::query_scalar::<_,i64>("select count(*) from workflow_dispatches where workflow_id=$1 and operation='notify' and result is not null").bind(workflow).fetch_one(&server.pool).await.unwrap();
        (count > 0).then_some(count)
    })
    .await;
    assert_eq!(delivered, 1);
}

async fn control(server: &Server, id: &str, body: Value) -> (u16, Value) {
    server
        .user
        .json(server.user.post(&format!("/api/v1/workflows/{id}/control")).json(&body))
        .await
}

async fn start_gated(server: &Server, max_rounds: u32) -> String {
    let (status, workflow) = server
        .user
        .json(server.user.post("/api/v1/workflows").json(&json!({"title":"Review","max_rounds":max_rounds,"steps":[{"key":"check","title":"Check","gate":true}]})))
        .await;
    assert_eq!(status, 200, "{workflow}");
    let id = workflow["id"].as_str().unwrap().to_string();
    wait_status(server, &id, "waiting").await;

    id
}

#[tokio::test]
async fn an_open_gate_makes_the_workflow_wait_for_you() {
    let server = Server::start().await;
    let id = start_gated(&server, 1).await;
    let workflow = get(&server, &id).await;

    assert_eq!((workflow["status"].clone(), workflow["steps"][0]["status"].clone()), (json!("waiting"), json!("gate")));
    assert!(workflow["steps"][0]["started_at"].is_string());
}

#[tokio::test]
async fn approving_a_gate_records_who_decided_and_the_note() {
    let server = Server::start().await;
    let id = start_gated(&server, 1).await;

    control(&server, &id, json!({"action":"approve","step":"check","reason":"Looks right"})).await;
    let finished = wait_status(&server, &id, "done").await;

    assert_eq!((finished["decisions"][0]["decision"].clone(), finished["decisions"][0]["reason"].clone(), finished["steps"][0]["status"].clone()), (json!("approve"), json!("Looks right"), json!("approved")));
    assert_eq!(finished["decisions"][0]["by"]["kind"], "person");
    assert_eq!(finished["end_reason"]["reason"], "finished");
}

#[tokio::test]
async fn sending_back_needs_a_reason() {
    let server = Server::start().await;
    let id = start_gated(&server, 3).await;

    let (status, body) = control(&server, &id, json!({"action":"send_back","step":"check","reason":"  "})).await;

    assert_eq!((status, body["error"].clone()), (400, json!("reason_required")), "{body}");
    assert_eq!(get(&server, &id).await["round"], 1);
}

#[tokio::test]
async fn sending_back_starts_the_next_round_and_keeps_the_earlier_one() {
    let server = Server::start().await;
    let id = start_gated(&server, 3).await;

    let (status, body) = control(&server, &id, json!({"action":"send_back","step":"check","reason":"Cover the empty case"})).await;
    assert_eq!(status, 200, "{body}");
    let second = eventually("second round gate", || async {
        let workflow = get(&server, &id).await;
        (workflow["round"] == 2 && workflow["steps"][0]["status"] == "gate").then_some(workflow)
    })
    .await;

    assert_eq!(second["rounds"].as_array().unwrap().len(), 2);
    assert_eq!(second["rounds"][0]["steps"][0]["status"], "sent_back");
    assert_eq!((second["rounds"][1]["reason"].clone(), second["rounds"][1]["decided_by"]["kind"].clone()), (json!("Cover the empty case"), json!("person")));
    assert_eq!(second["decisions"][0]["decision"], "send_back");
}

#[tokio::test]
async fn sending_back_on_the_last_round_ends_the_workflow() {
    let server = Server::start().await;
    let id = start_gated(&server, 1).await;

    control(&server, &id, json!({"action":"send_back","step":"check","reason":"Not good enough"})).await;
    let ended = wait_status(&server, &id, "done").await;

    assert_eq!((ended["end_reason"]["reason"].clone(), ended["round"].clone()), (json!("round_limit"), json!(1)));
}

#[tokio::test]
async fn stopping_an_ended_workflow_is_refused() {
    let server = Server::start().await;
    let id = start_gated(&server, 1).await;
    control(&server, &id, json!({"action":"stop"})).await;

    let (status, body) = control(&server, &id, json!({"action":"stop"})).await;
    let stopped = get(&server, &id).await;

    assert_eq!((status, body["error"].clone()), (409, json!("workflow_ended")), "{body}");
    assert_eq!((stopped["status"].clone(), stopped["end_reason"]["reason"].clone(), stopped["end_reason"]["by"]["kind"].clone()), (json!("stopped"), json!("stopped"), json!("person")));
    assert_eq!(stopped["steps"][0]["status"], "not_run");
}

#[tokio::test]
async fn re_running_copies_the_plan_into_a_new_workflow_linked_to_the_original() {
    let server = Server::start().await;
    let id = start_gated(&server, 2).await;

    let (active, refused) = server.user.json(server.user.post(&format!("/api/v1/workflows/{id}/rerun"))).await;
    control(&server, &id, json!({"action":"stop"})).await;
    let (status, rerun) = server.user.json(server.user.post(&format!("/api/v1/workflows/{id}/rerun"))).await;

    assert_eq!((active, refused["error"].clone()), (409, json!("workflow_active")));
    assert_eq!(status, 200, "{rerun}");
    assert_ne!(rerun["id"], json!(id));
    assert_eq!((rerun["rerun_of"].clone(), rerun["plan"].clone()), (json!(id), get(&server, &id).await["plan"].clone()));
}

#[tokio::test]
async fn a_failed_step_is_recorded_with_its_error_and_later_steps_did_not_run() {
    let server = Server::start().await;
    let mut link = server.online_node("worker").await;
    let (_, w) = server.user.json(server.user.post("/api/v1/workflows").json(&json!({"title":"Build","steps":[
        {"key":"work","title":"Build","prompt":"Build","node_id":link.id,"agent":"codex","cwd":"/work/app"},
        {"key":"after","title":"After","gate":true,"depends_on":["work"]}]}))).await;
    let id = w["id"].as_str().unwrap();
    let conversation = match link.next().await {
        mycelium_proto::link::ControlToNode::StartSession { conversation_id, .. } => conversation_id,
        other => panic!("expected start, got {other:?}"),
    };
    sqlx::query("update conversations set status='failed' where id=$1").bind(conversation).execute(&server.pool).await.unwrap();

    let failed = wait_status(&server, id, "failed").await;

    assert_eq!((failed["end_reason"]["reason"].clone(), failed["end_reason"]["step"].clone()), (json!("failed"), json!("work")));
    assert!(!failed["end_reason"]["error"].as_str().unwrap_or_default().is_empty(), "{failed}");
    assert_eq!((failed["steps"][0]["status"].clone(), failed["steps"][1]["status"].clone()), (json!("failed"), json!("not_run")));
}

#[tokio::test]
async fn a_claude_code_workflow_run_appears_in_the_list_and_ends_with_its_journal() {
    use mycelium_proto::link::NodeToControl;
    let server = Server::start().await;
    let mut link = server.online_node("worker").await;
    let conversation = server.create_conversation(link.id, Some("Plan")).await;
    let lines = [
        json!({"type":"plan","phases":["Implement","Verify"]}),
        json!({"type":"started","key":"a","label":"Write code","phase":"Implement","agentId":"a1b2c3d4e5f60718"}),
        json!({"type":"result","key":"a","tokens":1200,"turns":3,"elapsed_ms":65000,"summary":"Wrote it"}),
        json!({"type":"ended","state":"completed"}),
    ];

    for line in lines {
        link.send(NodeToControl::Workflow { conversation_id: conversation.id, run_id: "run-1".into(), line }).await;
    }
    let run = eventually("agent run indexed", || async {
        let (_, list) = server.user.json(server.user.get("/api/v1/workflows")).await;
        list.as_array().unwrap().iter().find(|w| w["kind"] == "agent_run" && w["status"] == "done").cloned()
    })
    .await;

    assert_eq!((run["run_id"].clone(), run["conversation_id"].clone(), run["phases"].clone()), (json!("run-1"), json!(conversation.id), json!(["Implement", "Verify"])));
    assert_eq!((run["steps"][0]["label"].clone(), run["steps"][0]["tokens"].clone(), run["steps"][0]["turns"].clone(), run["steps"][0]["summary"].clone()), (json!("Write code"), json!(1200), json!(3), json!("Wrote it")));
}
