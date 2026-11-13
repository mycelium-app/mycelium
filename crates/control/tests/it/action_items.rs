use crate::common::{self, NodeLink, Server};
use crate::watches::{end_turn, prompt_for};
use base64::Engine;
use mycelium_proto::link::{ControlToNode, NodeToControl};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

const UPDATE: &str = "Update Mycelium on atlas: docker compose pull && docker compose up -d";

struct Setup {
    server: Server,
    link: NodeLink,
    chat: Uuid,
}

async fn setup() -> Setup {
    let server = Server::start_with(|config| config.scheduler_tick = Duration::from_millis(50)).await;
    let link = server.online_node("laptop").await;
    let chat = server.create_conversation(link.id, None).await.id;

    Setup { server, link, chat }
}

async fn tool(setup: &Setup, caller: Uuid, name: &str, arguments: Value) -> Value {
    let reply = setup.server.agent_mcp(&setup.link.token, caller, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}})).await;

    reply["result"].clone()
}

async fn add(setup: &Setup, caller: Uuid, text: &str) -> String {
    let reply = tool(setup, caller, "add_action_item", json!({"text": text})).await;
    assert_eq!(reply["isError"], json!(false), "{reply}");
    let message = reply["content"][0]["text"].as_str().unwrap();

    message.split_whitespace().nth(3).unwrap().to_string()
}

async fn items(setup: &Setup, id: Uuid) -> Vec<Value> {
    let conversation = setup.server.conversation(id).await;

    conversation.get("action_items").map(|items| items.as_array().unwrap().clone()).unwrap_or_default()
}

fn texts(items: &[Value]) -> Vec<&str> {
    items.iter().map(|item| item["text"].as_str().unwrap()).collect()
}

#[tokio::test]
async fn an_added_action_item_shows_on_the_conversation() {
    let setup = setup().await;

    let item = add(&setup, setup.chat, UPDATE).await;

    let shown = items(&setup, setup.chat).await;
    assert_eq!(texts(&shown), [UPDATE]);
    assert_eq!(shown[0]["id"], item);
    assert!(shown[0]["created_at"].is_string());
}

#[tokio::test]
async fn a_conversation_holds_several_action_items_oldest_first() {
    let setup = setup().await;

    add(&setup, setup.chat, UPDATE).await;
    add(&setup, setup.chat, "Send Alex the invoice").await;

    assert_eq!(texts(&items(&setup, setup.chat).await), [UPDATE, "Send Alex the invoice"]);
}

#[tokio::test]
async fn the_person_marking_an_item_done_removes_it_from_the_conversation() {
    let setup = setup().await;
    let item = add(&setup, setup.chat, UPDATE).await;
    add(&setup, setup.chat, "Send Alex the invoice").await;

    let (status, conversation) = setup.server.user.json(setup.server.user.delete(&format!("/api/v1/conversations/{}/action-items/{item}", setup.chat))).await;

    assert_eq!(status, 200, "{conversation}");
    assert_eq!(texts(&items(&setup, setup.chat).await), ["Send Alex the invoice"]);
    assert_eq!(conversation["id"], setup.chat.to_string());
    assert_eq!(conversation["action_items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn marking_an_item_done_twice_is_not_found() {
    let setup = setup().await;
    let item = add(&setup, setup.chat, UPDATE).await;
    let path = format!("/api/v1/conversations/{}/action-items/{item}", setup.chat);
    setup.server.user.json(setup.server.user.delete(&path)).await;

    let (status, _) = setup.server.user.json(setup.server.user.delete(&path)).await;

    assert_eq!(status, 404);
}

#[tokio::test]
async fn writing_in_the_conversation_does_not_clear_an_action_item() {
    let setup = setup().await;
    add(&setup, setup.chat, UPDATE).await;

    let (status, body) = setup.server.user.json(setup.server.user.post(&format!("/api/v1/conversations/{}/prompt", setup.chat)).json(&json!({"text": "I will do it after lunch"}))).await;

    assert!(status < 300, "{body}");
    assert_eq!(texts(&items(&setup, setup.chat).await), [UPDATE]);
}

#[tokio::test]
async fn the_agent_resolving_its_own_item_removes_it() {
    let setup = setup().await;
    let item = add(&setup, setup.chat, UPDATE).await;

    let reply = tool(&setup, setup.chat, "resolve_action_item", json!({"item": item})).await;

    assert_eq!(reply["isError"], json!(false), "{reply}");
    assert!(items(&setup, setup.chat).await.is_empty());
}

#[tokio::test]
async fn resolving_an_unknown_item_is_refused() {
    let setup = setup().await;
    add(&setup, setup.chat, UPDATE).await;

    let unknown = tool(&setup, setup.chat, "resolve_action_item", json!({"item": Uuid::new_v4().to_string()})).await;
    let malformed = tool(&setup, setup.chat, "resolve_action_item", json!({"item": "the update"})).await;

    assert_eq!((unknown["isError"].clone(), malformed["isError"].clone()), (json!(true), json!(true)));
    assert_eq!(texts(&items(&setup, setup.chat).await), [UPDATE]);
}

#[tokio::test]
async fn resolving_another_conversations_item_is_refused() {
    let setup = setup().await;
    let other = setup.server.create_conversation(setup.link.id, None).await.id;
    let item = add(&setup, setup.chat, UPDATE).await;

    let reply = tool(&setup, other, "resolve_action_item", json!({"item": item})).await;

    assert_eq!(reply["isError"], json!(true));
    assert_eq!(texts(&items(&setup, setup.chat).await), [UPDATE]);
}

#[tokio::test]
async fn another_owner_cannot_see_or_finish_the_items() {
    let setup = setup().await;
    let item = add(&setup, setup.chat, UPDATE).await;
    let sam = setup.server.second_user("sam@example.com").await;

    let (shown, _) = sam.json(sam.get(&format!("/api/v1/conversations/{}", setup.chat))).await;
    let (finished, _) = sam.json(sam.delete(&format!("/api/v1/conversations/{}/action-items/{item}", setup.chat))).await;

    assert_eq!((shown, finished), (404, 404));
    assert_eq!(texts(&items(&setup, setup.chat).await), [UPDATE]);
}

#[tokio::test]
async fn a_managed_conversation_with_an_open_action_item_is_not_archived_when_idle() {
    let setup = setup().await;
    let managed = setup.server.create_conversation(setup.link.id, None).await.id;
    let quiet = setup.server.create_conversation(setup.link.id, None).await.id;
    add(&setup, managed, UPDATE).await;
    for id in [managed, quiet] {
        sqlx::query("update conversations set managed_by = $2 where id = $1").bind(id).bind(setup.chat).execute(&setup.server.pool).await.unwrap();
        sqlx::query("update conversations set created_at = now() - interval '4 days', updated_at = now() - interval '4 days', last_activity_at = now() - interval '4 days' where id = $1").bind(id).execute(&setup.server.pool).await.unwrap();
        sqlx::query("update events set created_at = now() - interval '4 days' where conversation_id = $1").bind(id).execute(&setup.server.pool).await.unwrap();
    }

    common::eventually("the quiet conversation to be archived", || async { setup.server.conversation(quiet).await["archived_at"].is_string().then_some(()) }).await;

    assert!(setup.server.conversation(managed).await["archived_at"].is_null());
}

const COMMAND: &str = "git push origin --delete feature/old-sync";
const FOLDER: &str = "~/projects/acme-backups";
const DELETE_NOTE: &str = "Delete the merged branch from the remote. Only Sam's key can push to acme-backups.";

struct Runner {
    server: Server,
    atlas: NodeLink,
    orion: NodeLink,
    chat: Uuid,
    item: String,
}

async fn runner_with(atlas_version: &str) -> Runner {
    let server = Server::start_with(|config| config.scheduler_tick = Duration::from_millis(50)).await;
    let mut atlas = server.online_node_as("atlas", atlas_version).await;
    let orion = server.online_node_as("orion", "0.34.30").await;
    let chat = common::caller_on_a_recent_node(&server, &mut atlas).await;
    end_turn(&mut atlas, chat, 1).await;
    let reply = call(&server, &atlas, chat, json!({"text": DELETE_NOTE, "command": COMMAND, "node": "atlas", "folder": FOLDER})).await;
    assert_eq!(reply["isError"], json!(false), "{reply}");
    let item = reply["content"][0]["text"].as_str().unwrap().split_whitespace().nth(3).unwrap().to_string();

    Runner { server, atlas, orion, chat, item }
}

async fn runner() -> Runner {
    runner_with("0.34.30").await
}

async fn call(server: &Server, link: &NodeLink, chat: Uuid, arguments: Value) -> Value {
    let reply = server.agent_mcp(&link.token, chat, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "add_action_item", "arguments": arguments}})).await;

    reply["result"].clone()
}

async fn press_run(runner: &Runner) -> (u16, Value) {
    runner.server.user.json(runner.server.user.post(&format!("/api/v1/conversations/{}/action-items/{}/run", runner.chat, runner.item)).json(&json!({"cols": 80, "rows": 24}))).await
}

async fn started(runner: &mut Runner) -> Uuid {
    let (status, conversation) = press_run(runner).await;
    assert_eq!(status, 200, "{conversation}");

    let ControlToNode::StartTerminal { terminal_id, .. } = runner.atlas.next().await else {
        panic!("expected the node to be told to start the command");
    };

    terminal_id
}

async fn printed(runner: &mut Runner, terminal_id: Uuid, text: &str) {
    runner.atlas.send(NodeToControl::TerminalOutput { terminal_id, data: base64::engine::general_purpose::STANDARD.encode(text) }).await;
}

#[tokio::test]
async fn an_action_item_with_a_command_is_stored_and_listed_with_its_node_and_folder() {
    let runner = runner().await;

    let conversation = runner.server.conversation(runner.chat).await;
    let shown = conversation["action_items"].as_array().unwrap();

    assert_eq!(shown[0]["command"], COMMAND);
    assert_eq!(shown[0]["node_name"], "atlas");
    assert_eq!(shown[0]["folder"], FOLDER);
    assert!(shown[0].get("run").is_none());
}

#[tokio::test]
async fn an_action_item_names_another_node_and_defaults_to_the_conversations_own() {
    let runner = runner().await;
    let other = call(&runner.server, &runner.atlas, runner.chat, json!({"text": "Restart the proxy", "command": "sudo systemctl restart proxy", "node": "orion"})).await;
    assert_eq!(other["isError"], json!(false), "{other}");
    let default = call(&runner.server, &runner.atlas, runner.chat, json!({"text": "List the backups", "command": "ls"})).await;
    assert_eq!(default["isError"], json!(false), "{default}");

    let conversation = runner.server.conversation(runner.chat).await;
    let listed = conversation["action_items"].as_array().unwrap();

    assert_eq!((listed[1]["node_name"].as_str(), listed[1]["folder"].as_str()), (Some("orion"), Some("/work/app")));
    assert_eq!((listed[2]["node_name"].as_str(), listed[2]["folder"].as_str()), (Some("atlas"), Some("/work/app")));
}

#[tokio::test]
async fn a_node_or_folder_without_a_command_is_refused() {
    let runner = runner().await;

    let reply = call(&runner.server, &runner.atlas, runner.chat, json!({"text": "Send Sam the invoice", "node": "orion"})).await;

    assert_eq!(reply["isError"], json!(true), "{reply}");
}

#[tokio::test]
async fn running_an_item_tells_its_node_to_start_the_command_in_its_folder() {
    let mut runner = runner().await;

    let (status, conversation) = press_run(&runner).await;
    assert_eq!(status, 200, "{conversation}");
    let ControlToNode::StartTerminal { terminal_id, conversation_id, cwd, cols, rows, command } = runner.atlas.next().await else {
        panic!("expected the node to be told to start the command");
    };

    assert_eq!((conversation_id, cwd.as_str(), command.as_deref(), cols, rows), (runner.chat, FOLDER, Some(COMMAND), 80, 24));
    let run = &conversation["action_items"][0]["run"];
    assert_eq!((run["status"].as_str(), run["terminal_id"].as_str()), (Some("running"), Some(terminal_id.to_string().as_str())));
}

#[tokio::test]
async fn the_command_runs_on_the_items_node_not_the_conversations() {
    let mut runner = runner().await;
    let reply = call(&runner.server, &runner.atlas, runner.chat, json!({"text": "Restart the proxy", "command": "sudo systemctl restart proxy", "node": "orion", "folder": "/srv/proxy"})).await;
    let item = reply["content"][0]["text"].as_str().unwrap().split_whitespace().nth(3).unwrap().to_string();

    let (status, body) = runner.server.user.json(runner.server.user.post(&format!("/api/v1/conversations/{}/action-items/{item}/run", runner.chat)).json(&json!({"cols": 80, "rows": 24}))).await;
    assert_eq!(status, 200, "{body}");

    let ControlToNode::StartTerminal { cwd, command, .. } = runner.orion.next().await else {
        panic!("expected orion to be told to start the command");
    };
    assert_eq!((cwd.as_str(), command.as_deref()), ("/srv/proxy", Some("sudo systemctl restart proxy")));
}

#[tokio::test]
async fn exit_code_zero_resolves_the_item_and_tells_the_adding_conversation() {
    let mut runner = runner().await;
    let terminal_id = started(&mut runner).await;
    printed(&mut runner, terminal_id, " - [deleted] feature/old-sync\r\n").await;

    runner.atlas.send(NodeToControl::TerminalExited { terminal_id, exit_code: Some(0) }).await;

    let told = prompt_for(&mut runner.atlas, runner.chat).await;
    assert!(told.contains(COMMAND) && told.contains("exit code 0") && told.contains("[deleted] feature/old-sync") && told.contains("atlas") && told.contains(FOLDER), "{told}");
    let conversation = runner.server.conversation(runner.chat).await;
    assert!(conversation.get("action_items").is_none_or(|items| items.as_array().unwrap().is_empty()), "{conversation}");
}

#[tokio::test]
async fn exit_code_one_leaves_the_item_open_as_failed_with_its_output() {
    let mut runner = runner().await;
    let terminal_id = started(&mut runner).await;
    printed(&mut runner, terminal_id, "\u{1b}[31merror: remote ref does not exist\u{1b}[0m\r\n").await;

    runner.atlas.send(NodeToControl::TerminalExited { terminal_id, exit_code: Some(1) }).await;

    let told = prompt_for(&mut runner.atlas, runner.chat).await;
    assert!(told.contains("exit code 1") && told.contains("error: remote ref does not exist") && !told.contains('\u{1b}'), "{told}");
    let conversation = runner.server.conversation(runner.chat).await;
    let run = &conversation["action_items"][0]["run"];
    assert_eq!((run["status"].as_str(), run["exit_code"].as_i64()), (Some("failed"), Some(1)));
    assert_eq!(run["output"], "error: remote ref does not exist\n");
}

#[tokio::test]
async fn a_failed_item_can_be_run_again() {
    let mut runner = runner().await;
    let terminal_id = started(&mut runner).await;
    runner.atlas.send(NodeToControl::TerminalExited { terminal_id, exit_code: Some(1) }).await;
    prompt_for(&mut runner.atlas, runner.chat).await;

    let again = started(&mut runner).await;

    assert_ne!(again, terminal_id);
}

#[tokio::test]
async fn stopping_the_terminal_marks_the_run_stopped() {
    let mut runner = runner().await;
    let terminal_id = started(&mut runner).await;

    let (status, _) = runner.server.user.json(runner.server.user.delete(&format!("/api/v1/terminals/{terminal_id}"))).await;
    assert_eq!(status, 204);

    let told = prompt_for(&mut runner.atlas, runner.chat).await;
    assert!(told.contains("Stopped"), "{told}");
    let conversation = runner.server.conversation(runner.chat).await;
    assert_eq!(conversation["action_items"][0]["run"]["status"], "stopped");
}

#[tokio::test]
async fn an_older_node_is_refused_with_the_version_it_needs() {
    let runner = runner_with("0.34.28").await;

    let (status, body) = press_run(&runner).await;

    assert_eq!(status, 409, "{body}");
    let message = body["message"].as_str().unwrap();
    assert!(message.contains("atlas") && message.contains("v0.34.30"), "{message}");
}

#[tokio::test]
async fn an_item_without_a_command_cannot_be_run() {
    let setup = setup().await;
    let item = add(&setup, setup.chat, "Send Sam the invoice").await;

    let (status, body) = setup.server.user.json(setup.server.user.post(&format!("/api/v1/conversations/{}/action-items/{item}/run", setup.chat)).json(&json!({"cols": 80, "rows": 24}))).await;

    assert_eq!(status, 400, "{body}");
}

#[tokio::test]
async fn an_offline_node_refuses_the_run_and_leaves_the_item_waiting() {
    let runner = runner().await;
    drop(runner.atlas);
    tokio::time::sleep(Duration::from_millis(300)).await;

    let (status, body) = runner.server.user.json(runner.server.user.post(&format!("/api/v1/conversations/{}/action-items/{}/run", runner.chat, runner.item)).json(&json!({"cols": 80, "rows": 24}))).await;

    assert_eq!(status, 409, "{body}");
    let conversation = runner.server.conversation(runner.chat).await;
    assert!(conversation["action_items"][0].get("run").is_none());
}
