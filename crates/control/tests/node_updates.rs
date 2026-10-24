mod common;

use common::{NodeLink, Server, eventually};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::json;
use std::time::Duration;

const QUIET: Duration = Duration::from_millis(500);

fn control_version() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

async fn update_offered(link: &mut NodeLink) -> Option<ControlToNode> {
    loop {
        match tokio::time::timeout(QUIET, link.next()).await {
            Ok(message @ ControlToNode::Update { .. }) => return Some(message),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
}

async fn hello_from(server: &Server, version: &str) -> NodeLink {
    let (id, token) = server.join("node", "laptop").await;
    let mut link = server.connect_node(id, &token).await;
    link.hello_as(version, vec![]).await;

    let user = server.user.clone();
    eventually("the node to be online", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        nodes.as_array().unwrap().iter().any(|node| node["id"] == json!(id) && node["online"] == true).then_some(())
    })
    .await;

    link
}

#[tokio::test]
async fn a_node_older_than_the_control_plane_is_told_to_update_when_it_says_hello() {
    let server = Server::start_with(|config| config.releases_url = "http://releases.test/mycelium/".into()).await;
    let mut link = hello_from(&server, "v0.0.1").await;

    let offered = update_offered(&mut link).await;

    assert_eq!(offered, Some(ControlToNode::Update { releases_url: "http://releases.test/mycelium".into(), version: control_version() }));
}

#[tokio::test]
async fn a_node_on_the_control_planes_version_is_not_told_to_update() {
    let server = Server::start().await;
    let mut link = hello_from(&server, &control_version()).await;

    assert_eq!(update_offered(&mut link).await, None);
}

#[tokio::test]
async fn a_node_newer_than_the_control_plane_is_not_told_to_update() {
    let server = Server::start().await;
    let mut link = hello_from(&server, "v99.0.0").await;

    assert_eq!(update_offered(&mut link).await, None);
}

#[tokio::test]
async fn the_update_route_sends_the_order_to_a_node_that_is_up_to_date() {
    let server = Server::start().await;
    let mut link = hello_from(&server, &control_version()).await;

    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/nodes/{}/update", link.id))).await;

    assert_eq!(status, 204);
    assert!(matches!(update_offered(&mut link).await, Some(ControlToNode::Update { version, .. }) if version == control_version()));
}

#[tokio::test]
async fn the_update_route_refuses_an_offline_node() {
    let server = Server::start().await;
    let link = hello_from(&server, &control_version()).await;
    let id = link.id;
    drop(link);

    let user = server.user.clone();
    eventually("the node to go offline", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        (nodes[0]["online"] == false).then_some(())
    })
    .await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/nodes/{id}/update"))).await;

    assert_eq!((status, body["error"].as_str()), (409, Some("node_offline")));
}

#[tokio::test]
async fn the_nodes_update_result_is_shown_on_its_summary() {
    let server = Server::start().await;
    let mut link = hello_from(&server, &control_version()).await;

    link.send(NodeToControl::UpdateResult { ok: false, message: "This node was installed by Nix; update it through the flake".into() }).await;

    let user = server.user.clone();
    eventually("the update message", || async {
        let (_, nodes) = user.json(user.get("/api/v1/nodes")).await;

        (nodes[0]["update_message"] == "This node was installed by Nix; update it through the flake").then_some(())
    })
    .await;
}
