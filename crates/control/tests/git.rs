mod common;

use common::{NodeLink, Server};
use mycelium_proto::link::{CommitOptions, ControlToNode, GitCommand, NodeToControl};
use serde_json::{Value, json};
use uuid::Uuid;

async fn conversation_on(server: &Server) -> (NodeLink, Uuid) {
    let mut link = server.online_node("laptop").await;
    let created = server.create_conversation(link.id, None).await;
    link.expect_start_session().await;

    (link, created.id)
}

async fn answer_next(link: &mut NodeLink, ok: bool, output: Value, error: Option<&str>) -> (String, GitCommand) {
    let (request_id, cwd, command) = match link.next().await {
        ControlToNode::Git { request_id, cwd, command } => (request_id, cwd, command),
        other => panic!("expected git, got {other:?}"),
    };

    link.send(NodeToControl::GitResult { request_id, ok, output, error: error.map(String::from) }).await;

    (cwd, command)
}

#[tokio::test]
async fn git_status_is_read_by_the_node_in_the_conversations_folder() {
    let server = Server::start().await;
    let (mut link, id) = conversation_on(&server).await;
    let user = server.user.clone();
    let asking = tokio::spawn(async move { user.json(user.get(&format!("/api/v1/conversations/{id}/git/status"))).await });

    let status = json!({"branch": "main", "upstream": null, "ahead": 0, "behind": 0, "files": []});
    let (cwd, command) = answer_next(&mut link, true, status.clone(), None).await;

    assert_eq!((cwd.as_str(), command), ("/work/app", GitCommand::Status));
    assert_eq!(asking.await.unwrap(), (200, status));
}

#[tokio::test]
async fn git_graph_show_branches_and_switch_carry_their_arguments_to_the_node() {
    let server = Server::start().await;
    let (mut link, id) = conversation_on(&server).await;
    let user = server.user.clone();
    let asking = tokio::spawn(async move {
        let graph = user.json(user.get(&format!("/api/v1/conversations/{id}/git/graph?skip=200"))).await;
        let show = user.json(user.get(&format!("/api/v1/conversations/{id}/git/show?hash=abc1234"))).await;
        let branches = user.json(user.get(&format!("/api/v1/conversations/{id}/git/branches"))).await;
        let switch = user.json(user.post(&format!("/api/v1/conversations/{id}/git/switch")).json(&json!({"branch": " topic ", "create": true, "track": "origin/topic"}))).await;
        let blank = user.json(user.post(&format!("/api/v1/conversations/{id}/git/switch")).json(&json!({"branch": " "}))).await;

        [graph, show, branches, switch, blank].map(|(status, _)| status)
    });

    let mut commands = Vec::new();

    for _ in 0..4 {
        commands.push(answer_next(&mut link, true, json!({}), None).await.1);
    }

    assert_eq!(
        commands,
        vec![
            GitCommand::Graph { limit: 200, skip: 200 },
            GitCommand::Show { hash: "abc1234".into() },
            GitCommand::Branches,
            GitCommand::Switch { branch: "topic".into(), create: true, track: Some("origin/topic".into()) },
        ]
    );
    assert_eq!(asking.await.unwrap(), [200, 200, 200, 200, 400]);
}

#[tokio::test]
async fn git_log_diff_and_stage_carry_their_arguments_to_the_node() {
    let server = Server::start().await;
    let (mut link, id) = conversation_on(&server).await;
    let user = server.user.clone();
    let asking = tokio::spawn(async move {
        let log = user.json(user.get(&format!("/api/v1/conversations/{id}/git/log?limit=5"))).await;
        let capped = user.json(user.get(&format!("/api/v1/conversations/{id}/git/log?limit=9999"))).await;
        let diff = user.json(user.get(&format!("/api/v1/conversations/{id}/git/diff?path=src/a.rs&staged=true"))).await;
        let stage = user.json(user.post(&format!("/api/v1/conversations/{id}/git/stage")).json(&json!({"paths": ["a", "b"]}))).await;
        let unstage = user.json(user.post(&format!("/api/v1/conversations/{id}/git/unstage")).json(&json!({"paths": ["a"]}))).await;
        let all = user.json(user.post(&format!("/api/v1/conversations/{id}/git/stage-all"))).await;
        let undo = user.json(user.post(&format!("/api/v1/conversations/{id}/git/undo"))).await;

        [log, capped, diff, stage, unstage, all, undo].map(|(status, _)| status)
    });

    let mut commands = Vec::new();

    for _ in 0..7 {
        commands.push(answer_next(&mut link, true, json!({}), None).await.1);
    }

    assert_eq!(
        commands,
        vec![
            GitCommand::Log { limit: 5 },
            GitCommand::Log { limit: 200 },
            GitCommand::Diff { path: "src/a.rs".into(), staged: true },
            GitCommand::Stage { paths: vec!["a".into(), "b".into()] },
            GitCommand::Unstage { paths: vec!["a".into()] },
            GitCommand::StageAll,
            GitCommand::UndoLastCommit,
        ]
    );
    assert_eq!(asking.await.unwrap(), [200; 7]);
}

#[tokio::test]
async fn a_commit_is_sent_to_the_node_with_its_message_trimmed() {
    let server = Server::start().await;
    let (mut link, id) = conversation_on(&server).await;
    let user = server.user.clone();
    let asking = tokio::spawn(async move { user.json(user.post(&format!("/api/v1/conversations/{id}/git/commit")).json(&json!({"message": "  Fix the thing\n"}))).await });

    let (_, command) = answer_next(&mut link, true, json!({}), None).await;

    assert_eq!(command, GitCommand::Commit { message: "Fix the thing".into(), options: CommitOptions::default() });
    assert_eq!(asking.await.unwrap().0, 200);
}

#[tokio::test]
async fn an_amend_may_keep_the_last_message_and_carries_its_options_to_the_node() {
    let server = Server::start().await;
    let (mut link, id) = conversation_on(&server).await;
    let user = server.user.clone();
    let asking = tokio::spawn(async move { user.json(user.post(&format!("/api/v1/conversations/{id}/git/commit")).json(&json!({"message": "", "amend": true, "signoff": true, "no_verify": true}))).await });

    let (_, command) = answer_next(&mut link, true, json!({}), None).await;

    assert_eq!(command, GitCommand::Commit { message: String::new(), options: CommitOptions { amend: true, signoff: true, no_verify: true } });
    assert_eq!(asking.await.unwrap().0, 200);
}

#[tokio::test]
async fn a_commit_without_a_message_is_refused_before_it_reaches_the_node() {
    let server = Server::start().await;
    let (_link, id) = conversation_on(&server).await;

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/conversations/{id}/git/commit")).json(&json!({"message": "  "}))).await;

    assert_eq!(status, 400);
    assert_eq!(body["message"], "Enter a commit message.");
}

#[tokio::test]
async fn gits_own_error_comes_back_as_a_conflict() {
    let server = Server::start().await;
    let (mut link, id) = conversation_on(&server).await;
    let user = server.user.clone();
    let asking = tokio::spawn(async move { user.json(user.post(&format!("/api/v1/conversations/{id}/git/fetch"))).await });

    let (_, command) = answer_next(&mut link, false, Value::Null, Some("fatal: no remote repository specified")).await;

    assert_eq!(command, GitCommand::Fetch);
    let (status, body) = asking.await.unwrap();
    assert_eq!(status, 409);
    assert_eq!(body["error"], "git_failed");
    assert_eq!(body["message"], "fatal: no remote repository specified");
}

#[tokio::test]
async fn git_on_an_offline_node_says_so() {
    let server = Server::start().await;
    let (link, id) = conversation_on(&server).await;
    drop(link);

    let path = format!("/api/v1/conversations/{id}/git/status");
    let body = common::eventually("the node to go offline", || async {
        let (status, body) = server.user.json(server.user.get(&path)).await;

        (status == 409).then_some(body)
    })
    .await;

    assert_eq!(body["error"], "node_offline");
}

#[tokio::test]
async fn another_user_cannot_run_git_in_a_conversation() {
    let server = Server::start().await;
    let (_link, id) = conversation_on(&server).await;
    let other = server.second_user("eve@example.com").await;

    assert_eq!(other.json(other.get(&format!("/api/v1/conversations/{id}/git/status"))).await.0, 404);
    assert_eq!(other.json(other.post(&format!("/api/v1/conversations/{id}/git/stage-all"))).await.0, 404);
}
