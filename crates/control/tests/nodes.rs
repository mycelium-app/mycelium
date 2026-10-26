mod common;

use common::Server;
use mycelium_proto::link::{ControlToNode, ListeningPort, NodeToControl, ProcessUsage, ResourceSample};
use mycelium_proto::stream::StreamMessage;
use serde_json::json;

fn sample(at_ms: i64, cpu_percent: f32) -> ResourceSample {
    ResourceSample {
        at_ms,
        available: true,
        cpu_percent,
        cores: 4,
        load: [0.5, 0.4, 0.3],
        memory_total_bytes: 8,
        memory_available_bytes: 4,
        swap_total_bytes: 0,
        swap_free_bytes: 0,
        disks: vec![],
        processes: vec![],
        ports: vec![ListeningPort { port: 3000, address: "0.0.0.0".into(), pid: None, process: Some("node".into()), conversation_id: None, cwd: None }],
        docker: None,
    }
}

#[tokio::test]
async fn a_node_can_be_renamed_and_the_new_name_is_listed() {
    let server = Server::start().await;
    let (id, _) = server.join("node", "willow-3").await;

    let (status, _) = server.user.json(server.user.patch(&format!("/api/v1/nodes/{id}")).json(&json!({"name": "  willow  "}))).await;
    assert_eq!(status, 204);

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    assert_eq!(nodes[0]["name"], "willow");
}

#[tokio::test]
async fn renaming_to_a_live_nodes_name_or_to_nothing_is_refused_with_a_reason() {
    let server = Server::start().await;
    server.join("node", "laptop").await;
    let (id, _) = server.join("node", "desk").await;

    let (taken, body) = server.user.json(server.user.patch(&format!("/api/v1/nodes/{id}")).json(&json!({"name": "laptop"}))).await;
    assert_eq!(taken, 409);
    assert_eq!(body["message"], "Another node is already called laptop. Choose a different name.");

    let (empty, body) = server.user.json(server.user.patch(&format!("/api/v1/nodes/{id}")).json(&json!({"name": " "}))).await;
    assert_eq!(empty, 400);
    assert_eq!(body["message"], "Enter a name for this node.");
}

#[tokio::test]
async fn a_machine_joining_again_after_a_revoke_gets_its_old_name_back() {
    let server = Server::start().await;
    let (old, _) = server.join("node", "willow").await;

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/nodes/{old}"))).await;
    assert_eq!(status, 204);

    server.join("node", "willow").await;

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    assert_eq!(nodes.as_array().unwrap().len(), 1);
    assert_eq!(nodes[0]["name"], "willow");
}

#[tokio::test]
async fn the_modes_models_and_settings_a_session_offers_are_kept_on_the_conversation() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let created = server.create_conversation(link.id, None).await;
    let conversation_id = link.expect_start_session().await;
    assert_eq!(conversation_id, created.id);

    let models = json!({"currentModelId": "opus", "availableModels": [{"modelId": "opus", "name": "Opus 5.5"}, {"modelId": "sonnet", "name": "Sonnet 5.5"}]});
    let settings = json!([{"id": "effort", "name": "Effort", "currentValue": "medium", "options": [{"value": "low", "name": "Low"}, {"value": "medium", "name": "Medium"}]}]);
    link.send(NodeToControl::SessionStarted { conversation_id, acp_session_id: "s1".into(), modes: None, models: Some(models.clone()), config_options: Some(settings.clone()) }).await;

    let user = server.user.clone();
    let conversation = common::eventually("the session options", || async {
        let (_, body) = user.json(user.get(&format!("/api/v1/conversations/{conversation_id}"))).await;

        (!body["options"].is_null()).then_some(body)
    })
    .await;

    assert_eq!(conversation["model"], "opus");
    assert_eq!(conversation["options"]["models"], models);
    assert_eq!(conversation["options"]["config_options"], settings);
}

#[tokio::test]
async fn a_node_icon_can_be_chosen_from_the_set_and_anything_else_is_refused() {
    let server = Server::start().await;
    let (id, _) = server.join("node", "pi").await;

    let (status, _) = server.user.json(server.user.patch(&format!("/api/v1/nodes/{id}")).json(&json!({"icon": "chip"}))).await;
    assert_eq!(status, 204);

    let (refused, body) = server.user.json(server.user.patch(&format!("/api/v1/nodes/{id}")).json(&json!({"icon": "rocket"}))).await;
    assert_eq!(refused, 400);
    assert_eq!(body["message"], "rocket is not a node icon. Choose one of server, laptop, desktop, cloud, terminal, chip.");

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    assert_eq!(nodes[0]["icon"], "chip");
    assert_eq!(nodes[0]["name"], "pi");
}

#[tokio::test]
async fn a_node_colour_can_be_chosen_from_the_set_and_anything_else_is_refused() {
    let server = Server::start().await;
    let (id, _) = server.join("node", "pi").await;

    let (status, _) = server.user.json(server.user.patch(&format!("/api/v1/nodes/{id}")).json(&json!({"colour": "rose"}))).await;
    assert_eq!(status, 204);

    let (refused, body) = server.user.json(server.user.patch(&format!("/api/v1/nodes/{id}")).json(&json!({"colour": "teal"}))).await;
    assert_eq!(refused, 400);
    assert_eq!(body["message"], "teal is not a node colour. Choose one of blue, violet, amber, rose, green.");

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    assert_eq!(nodes[0]["colour"], "rose");
}

async fn node_sessions(server: &Server, node_id: uuid::Uuid) -> Vec<String> {
    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;
    let node = nodes.as_array().unwrap().iter().find(|node| node["id"] == json!(node_id)).unwrap();

    node["sessions"].as_array().unwrap().iter().map(|id| id.as_str().unwrap().to_string()).collect()
}

async fn node_with_sessions(stream: &mut common::StreamClient, node_id: uuid::Uuid, wanted: Vec<uuid::Uuid>) {
    stream
        .until(|message| match message {
            mycelium_proto::stream::StreamMessage::Node(node) if node.id == node_id && node.sessions == wanted => Some(()),
            _ => None,
        })
        .await;
}

#[tokio::test]
async fn a_session_that_starts_is_listed_on_its_node_and_streamed() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;
    assert!(node_sessions(&server, link.id).await.is_empty());

    let created = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    assert!(node_sessions(&server, link.id).await.is_empty());

    link.start_session(created.id, "s1").await;

    node_with_sessions(&mut stream, link.id, vec![created.id]).await;
    assert_eq!(node_sessions(&server, link.id).await, [created.id.to_string()]);
}

#[tokio::test]
async fn a_session_that_fails_is_streamed_as_no_longer_running() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let created = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(created.id, "s1").await;
    common::eventually("the session", || async { (!node_sessions(&server, link.id).await.is_empty()).then_some(()) }).await;
    let mut stream = server.user.stream().await;

    link.send(NodeToControl::SessionFailed { conversation_id: created.id, seq: 1, message: "The agent stopped.".into() }).await;

    node_with_sessions(&mut stream, link.id, vec![]).await;
    assert!(node_sessions(&server, link.id).await.is_empty());
}

#[tokio::test]
async fn archiving_a_conversation_closes_its_session_and_streams_the_change() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let created = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;
    link.start_session(created.id, "s1").await;
    common::eventually("the session", || async { (!node_sessions(&server, link.id).await.is_empty()).then_some(()) }).await;
    let mut stream = server.user.stream().await;

    let (status, _) = server.user.json(server.user.patch(&format!("/api/v1/conversations/{}", created.id)).json(&json!({"archived": true}))).await;
    assert_eq!(status, 200);

    node_with_sessions(&mut stream, link.id, vec![]).await;
}

#[tokio::test]
async fn a_node_reporting_live_sessions_when_it_connects_lists_them_and_loses_them_when_it_leaves() {
    let server = Server::start().await;
    let (id, token) = server.join("node", "laptop").await;
    let conversation = uuid::Uuid::new_v4();
    let mut link = server.connect_node(id, &token).await;
    let mut stream = server.user.stream().await;

    link.hello(vec![(conversation, 0)]).await;
    node_with_sessions(&mut stream, id, vec![conversation]).await;

    drop(link);
    node_with_sessions(&mut stream, id, vec![]).await;
    assert!(node_sessions(&server, id).await.is_empty());
}

#[tokio::test]
async fn watching_a_node_asks_it_to_sample_for_a_while() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/nodes/{}/watch", link.id))).await;

    assert_eq!(status, 200, "{body}");
    assert_eq!(body, json!({"latest": null, "history": []}));
    assert_eq!(link.next().await, ControlToNode::Watch { seconds: 30 });
}

#[tokio::test]
async fn a_sample_from_the_node_is_streamed_and_kept_with_its_recent_history() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let mut stream = server.user.stream().await;

    link.send(NodeToControl::Resources { sample: sample(1000, 10.0) }).await;
    link.send(NodeToControl::Resources { sample: sample(4000, 30.0) }).await;

    let streamed = stream.until(|message| matches!(&message, StreamMessage::NodeResources { sample, .. } if sample.at_ms == 4000).then_some(message)).await;
    assert_eq!(streamed, StreamMessage::NodeResources { node_id: link.id, sample: sample(4000, 30.0) });

    let (_, body) = server.user.json(server.user.post(&format!("/api/v1/nodes/{}/watch", link.id))).await;
    assert_eq!(body["latest"]["ports"][0]["port"], 3000);
    assert_eq!(body["history"], json!([{"at_ms": 1000, "cpu_percent": 10.0}, {"at_ms": 4000, "cpu_percent": 30.0}]));
}

#[tokio::test]
async fn a_node_that_leaves_takes_its_samples_with_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let (id, token) = (link.id, link.token.clone());
    link.send(NodeToControl::Resources { sample: sample(1000, 10.0) }).await;
    drop(link);

    common::eventually("the node going offline", || async {
        let (status, body) = server.user.json(server.user.post(&format!("/api/v1/nodes/{id}/watch"))).await;

        (status == 409).then_some(body)
    })
    .await;

    let mut again = server.connect_node(id, &token).await;
    again.hello(vec![]).await;
    common::eventually("the node coming back", || async {
        let (status, body) = server.user.json(server.user.post(&format!("/api/v1/nodes/{id}/watch"))).await;

        (status == 200).then_some(body)
    })
    .await;
    let (_, body) = server.user.json(server.user.post(&format!("/api/v1/nodes/{id}/watch"))).await;

    assert_eq!(body, json!({"latest": null, "history": []}));
}

#[tokio::test]
async fn watching_an_offline_or_unknown_node_is_refused_with_a_reason() {
    let server = Server::start().await;
    let (offline, _) = server.join("node", "laptop").await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/nodes/{offline}/watch"))).await;
    assert_eq!((status, body["error"].as_str()), (409, Some("node_offline")));

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/nodes/{}/watch", uuid::Uuid::new_v4()))).await;
    assert_eq!((status, body["error"].as_str()), (404, Some("not_found")));
}

async fn node_reporting_a_process(server: &Server, stoppable: bool) -> common::NodeLink {
    let mut sample = sample(1000, 10.0);
    sample.processes = vec![ProcessUsage { pid: 42, name: "gradle".into(), command: "gradle".into(), cpu_percent: 5.0, memory_bytes: 1, agent: false, stoppable, conversation_id: None, cwd: None }];
    let mut link = server.online_node("laptop").await;
    link.send(NodeToControl::Resources { sample }).await;
    common::eventually("the sample", || async { (server.user.json(server.user.post(&format!("/api/v1/nodes/{}/watch", link.id))).await.1["latest"] != json!(null)).then_some(()) }).await;

    link
}

async fn answer_stop(link: &mut common::NodeLink, ok: bool, error: Option<&str>) -> (u32, String) {
    loop {
        if let ControlToNode::StopProcess { request_id, pid, name } = link.next().await {
            link.send(NodeToControl::ProcessStopped { request_id, ok, error: error.map(str::to_string) }).await;

            return (pid, name);
        }
    }
}

async fn stop(server: &Server, node: uuid::Uuid, pid: u32) -> (u16, serde_json::Value) {
    server.user.json(server.user.post(&format!("/api/v1/nodes/{node}/processes/{pid}/stop"))).await
}

#[tokio::test]
async fn stopping_a_process_asks_its_node_to_stop_it_by_the_name_it_reported() {
    let server = Server::start().await;
    let mut link = node_reporting_a_process(&server, true).await;
    let node = link.id;

    let ((status, body), asked) = tokio::join!(stop(&server, node, 42), answer_stop(&mut link, true, None));

    assert_eq!(status, 204, "{body}");
    assert_eq!(asked, (42, "gradle".to_string()));
}

#[tokio::test]
async fn a_process_the_node_refuses_to_stop_reports_why() {
    let server = Server::start().await;
    let mut link = node_reporting_a_process(&server, true).await;
    let node = link.id;

    let ((status, body), _) = tokio::join!(stop(&server, node, 42), answer_stop(&mut link, false, Some("gradle has already exited.")));

    assert_eq!((status, body["error"].as_str(), body["message"].as_str()), (409, Some("stop_failed"), Some("gradle has already exited.")));
}

#[tokio::test]
async fn a_process_that_is_not_stoppable_or_not_reported_is_never_sent_to_the_node() {
    let server = Server::start().await;
    let link = node_reporting_a_process(&server, false).await;

    let (status, body) = stop(&server, link.id, 42).await;
    assert_eq!((status, body["error"].as_str()), (409, Some("process_not_stoppable")));

    let (status, body) = stop(&server, link.id, 7).await;
    assert_eq!((status, body["error"].as_str()), (404, Some("not_found")));
}

async fn node_at(server: &Server, version: &str) -> common::NodeLink {
    let (id, token) = server.join("node", "laptop").await;
    let mut link = server.connect_node(id, &token).await;

    link.hello_as(version, vec![]).await;

    let user = server.user.clone();
    common::eventually("node online", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        nodes.as_array().unwrap().iter().any(|node| node["id"] == json!(id) && node["online"] == true).then_some(())
    })
    .await;

    link
}

#[tokio::test]
async fn the_folders_on_a_node_are_listed_through_the_node() {
    let server = Server::start().await;
    let mut link = node_at(&server, "v0.10.0").await;
    let user = server.user.clone();
    let id = link.id;
    let asking = tokio::spawn(async move { user.json(user.get(&format!("/api/v1/nodes/{id}/directories?path=%2Fwork"))).await });

    let (request_id, cwd, command) = loop {
        match link.next().await {
            ControlToNode::Git { request_id, cwd, command } => break (request_id, cwd, command),
            _ => continue,
        }
    };
    let listing = json!({"root": "/work", "path": "/work", "parent": null, "directories": [{"name": "app", "path": "/work/app", "repository": true}]});
    link.send(NodeToControl::GitResult { request_id, ok: true, output: listing.clone(), error: None }).await;

    let (status, body) = asking.await.unwrap();

    assert_eq!((status, cwd.as_str(), command), (200, "/work", mycelium_proto::link::GitCommand::Directories));
    assert_eq!(body, listing);
}

#[tokio::test]
async fn listing_folders_on_an_offline_node_says_it_is_offline() {
    let server = Server::start().await;
    let (id, _) = server.join("node", "laptop").await;

    let (status, body) = server.user.json(server.user.get(&format!("/api/v1/nodes/{id}/directories"))).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("node_offline")));
}

#[tokio::test]
async fn listing_folders_on_a_node_too_old_to_browse_asks_for_an_update() {
    let server = Server::start().await;
    let link = node_at(&server, "v0.9.1").await;

    let (status, body) = server.user.json(server.user.get(&format!("/api/v1/nodes/{}/directories", link.id))).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("node_outdated")));
    assert!(body["message"].as_str().unwrap().contains("to browse its folders, or enter the path."), "{body}");
}
