use crate::common::{self, NodeLink, Server};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

struct Setup {
    server: Server,
    link: NodeLink,
    manager: Uuid,
}

async fn setup() -> Setup {
    let server = Server::start_with(|config| config.scheduler_tick = Duration::from_millis(50)).await;
    let link = server.online_node("laptop").await;
    let manager = server.create_conversation(link.id, None).await.id;

    Setup { server, link, manager }
}

async fn managed_chat(setup: &Setup, idle_days: i32) -> Uuid {
    let id = setup.server.create_conversation(setup.link.id, None).await.id;
    sqlx::query("update conversations set managed_by = $2 where id = $1").bind(id).bind(setup.manager).execute(&setup.server.pool).await.unwrap();
    connect(&setup.server, setup.manager, id).await;
    idle_for(&setup.server, id, idle_days).await;

    id
}

async fn prompted_chat(setup: &mut Setup, session: &str) -> Uuid {
    let id = setup.server.create_conversation(setup.link.id, Some("Run the migration")).await.id;

    while setup.link.expect_start_session().await != id {}

    setup.link.start_session(id, session).await;
    setup.link.expect_prompt().await;

    id
}

async fn connect(server: &Server, first: Uuid, second: Uuid) {
    sqlx::query("insert into conversation_connections (owner, source_id, target_id) select owner, least($1::uuid, $2::uuid), greatest($1::uuid, $2::uuid) from conversations where id = $1").bind(first).bind(second).execute(&server.pool).await.unwrap();
}

async fn idle_for(server: &Server, id: Uuid, days: i32) {
    sqlx::query("update conversations set created_at = now() - make_interval(days => $2), updated_at = now() - make_interval(days => $2), last_activity_at = now() - make_interval(days => $2) where id = $1").bind(id).bind(days).execute(&server.pool).await.unwrap();
    sqlx::query("update events set created_at = now() - make_interval(days => $2) where conversation_id = $1").bind(id).bind(days).execute(&server.pool).await.unwrap();
}

async fn archived(server: &Server, id: Uuid) -> bool {
    server.conversation(id).await["archived_at"].is_string()
}

async fn wait_until_archived(server: &Server, id: Uuid) {
    common::eventually("the managed conversation to be archived", || async { archived(server, id).await.then_some(()) }).await;
}

async fn stays_while_an_idle_one_is_archived(setup: &Setup, subject: Uuid) {
    let control = managed_chat(setup, 4).await;

    wait_until_archived(&setup.server, control).await;

    assert!(!archived(&setup.server, subject).await);
}

async fn tool(setup: &Setup, caller: Uuid, name: &str, arguments: Value) -> Value {
    let reply = setup.server.agent_mcp(&setup.link.token, caller, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}})).await;

    reply["result"].clone()
}

#[tokio::test]
async fn a_managed_conversation_idle_for_three_days_is_archived_and_one_idle_for_two_stays() {
    let setup = setup().await;
    let recent = managed_chat(&setup, 2).await;

    stays_while_an_idle_one_is_archived(&setup, recent).await;
}

#[tokio::test]
async fn a_conversation_the_person_started_is_never_archived_automatically() {
    let setup = setup().await;
    let started = setup.server.create_conversation(setup.link.id, None).await.id;
    idle_for(&setup.server, started, 30).await;

    stays_while_an_idle_one_is_archived(&setup, started).await;
}

#[tokio::test]
async fn a_managed_conversation_with_a_proposal_waiting_on_the_person_stays() {
    let setup = setup().await;
    let waiting = managed_chat(&setup, 0).await;
    sqlx::query("insert into message_proposals (owner, conversation_id, target_id, message, reason) select owner, id, $2, 'The schema is ready', 'You asked' from conversations where id = $1")
        .bind(waiting)
        .bind(setup.manager)
        .execute(&setup.server.pool)
        .await
        .unwrap();
    idle_for(&setup.server, waiting, 4).await;

    stays_while_an_idle_one_is_archived(&setup, waiting).await;
}

#[tokio::test]
async fn a_working_managed_conversation_stays() {
    let mut setup = setup().await;
    let working = prompted_chat(&mut setup, "acp-working").await;
    sqlx::query("update conversations set managed_by = $2 where id = $1").bind(working).bind(setup.manager).execute(&setup.server.pool).await.unwrap();
    idle_for(&setup.server, working, 4).await;

    stays_while_an_idle_one_is_archived(&setup, working).await;
}

#[tokio::test]
async fn a_managed_conversation_watched_by_a_working_conversation_stays() {
    let mut setup = setup().await;
    let watcher = prompted_chat(&mut setup, "acp-watcher").await;
    let watched = managed_chat(&setup, 0).await;
    connect(&setup.server, watcher, watched).await;

    let reply = tool(&setup, watcher, "watch_conversation", json!({"conversation": watched.to_string()})).await;
    assert_eq!(reply["isError"], json!(false), "{reply}");
    idle_for(&setup.server, watched, 4).await;

    stays_while_an_idle_one_is_archived(&setup, watched).await;
}

#[tokio::test]
async fn a_managed_conversation_watched_only_by_an_idle_manager_is_archived() {
    let setup = setup().await;
    let watched = managed_chat(&setup, 0).await;

    let reply = tool(&setup, setup.manager, "watch_conversation", json!({"conversation": watched.to_string()})).await;
    assert_eq!(reply["isError"], json!(false), "{reply}");
    idle_for(&setup.server, watched, 4).await;

    wait_until_archived(&setup.server, watched).await;
}

#[tokio::test]
async fn a_managed_conversation_with_an_unanswered_permission_request_stays() {
    let setup = setup().await;
    let asking = managed_chat(&setup, 0).await;
    sqlx::query("insert into events (conversation_id, seq, kind, payload) values ($1, 1, 'permission_request', '{\"request_id\": \"r1\"}')").bind(asking).execute(&setup.server.pool).await.unwrap();
    idle_for(&setup.server, asking, 4).await;

    stays_while_an_idle_one_is_archived(&setup, asking).await;
}

#[tokio::test]
async fn a_managed_conversation_with_a_paused_workflow_stays() {
    let setup = setup().await;
    let running = managed_chat(&setup, 0).await;
    sqlx::query("insert into workflows (id, owner, conversation_id, title, status, plan) select gen_random_uuid(), owner, id, 'Import', 'paused', '{}' from conversations where id = $1").bind(running).execute(&setup.server.pool).await.unwrap();
    idle_for(&setup.server, running, 4).await;

    stays_while_an_idle_one_is_archived(&setup, running).await;
}

#[tokio::test]
async fn a_managed_conversation_waiting_on_its_own_timer_stays() {
    let setup = setup().await;
    let waiting = managed_chat(&setup, 0).await;
    sqlx::query("insert into watches (owner, watcher_id, kind, due_at, note) select owner, id, 'timer', now() + interval '1 day', 'Check the deploy' from conversations where id = $1").bind(waiting).execute(&setup.server.pool).await.unwrap();
    idle_for(&setup.server, waiting, 4).await;

    stays_while_an_idle_one_is_archived(&setup, waiting).await;
}

#[tokio::test]
async fn a_message_to_an_archived_managed_conversation_brings_it_back() {
    let setup = setup().await;
    let idle = managed_chat(&setup, 4).await;
    wait_until_archived(&setup.server, idle).await;

    let (status, body) = setup.server.user.json(setup.server.user.post(&format!("/api/v1/conversations/{idle}/prompt")).json(&json!({"text": "One more thing"}))).await;

    assert!(status < 300, "{status} {body}");
    assert!(!archived(&setup.server, idle).await);
}

#[tokio::test]
async fn send_to_conversation_is_refused_and_leaves_an_archived_managed_conversation_archived() {
    let setup = setup().await;
    let idle = managed_chat(&setup, 4).await;
    wait_until_archived(&setup.server, idle).await;

    let reply = tool(&setup, setup.manager, "send_to_conversation", json!({"conversation": idle.to_string(), "message": "One more thing", "reason": "It was missed"})).await;

    assert_eq!(reply["isError"], json!(true), "{reply}");
    assert!(archived(&setup.server, idle).await);
}

#[tokio::test]
async fn the_manager_archives_a_conversation_it_manages_with_archive_conversation() {
    let setup = setup().await;
    let managed = managed_chat(&setup, 0).await;

    let reply = tool(&setup, setup.manager, "archive_conversation", json!({"conversation": managed.to_string()})).await;

    assert_eq!(reply["isError"], json!(false), "{reply}");
    assert!(archived(&setup.server, managed).await);
}

#[tokio::test]
async fn archive_conversation_refuses_a_conversation_this_one_does_not_manage() {
    let setup = setup().await;
    let unmanaged = setup.server.create_conversation(setup.link.id, None).await.id;
    let managed = managed_chat(&setup, 0).await;
    let bystander = setup.server.create_conversation(setup.link.id, None).await.id;

    let refused = tool(&setup, setup.manager, "archive_conversation", json!({"conversation": unmanaged.to_string()})).await;
    let not_its_own = tool(&setup, bystander, "archive_conversation", json!({"conversation": managed.to_string()})).await;

    assert_eq!((refused["isError"].clone(), not_its_own["isError"].clone()), (json!(true), json!(true)));
    assert!(!archived(&setup.server, unmanaged).await);
    assert!(!archived(&setup.server, managed).await);
}
