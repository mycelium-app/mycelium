#[path = "common/fake_email.rs"]
mod fake_email;

use crate::common::Server;
use fake_email::{PASSWORD, USERNAME, fake_mail};
use serde_json::{Value, json};

async fn mcp(server: &Server, token: &str, body: Value) -> Value {
    let anonymous = server.anonymous();
    let (status, body) = anonymous.json(anonymous.post("/api/v1/mcp").header("authorization", format!("Bearer {token}")).json(&body)).await;
    assert_eq!(status, 200, "{body}");

    body
}

fn mail_settings(imap: u16, smtp: u16) -> Value {
    json!({"imap_host": "127.0.0.1", "imap_port": imap, "imap_tls": "tls", "smtp_host": "127.0.0.1", "smtp_port": smtp, "smtp_tls": "tls"})
}

async fn add_email_named(server: &Server, name: &str, imap: u16, smtp: u16, password: &str) -> (u16, Value) {
    server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": name, "kind": "email", "url": "", "account": USERNAME, "bearer_token": password, "settings": mail_settings(imap, smtp)}))).await
}

async fn agent_call(server: &Server, token: &str, conversation: uuid::Uuid, id: u64, tool: &str, arguments: Value) -> Value {
    let reply = server.agent_mcp(token, conversation, json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": format!("mail__{tool}"), "arguments": arguments}})).await;

    reply["result"].clone()
}

async fn call(server: &Server, token: &str, id: u64, tool: &str, arguments: Value) -> Value {
    if !matches!(tool, "send_message" | "delete_message" | "move_message") {
        let reply = mcp(server, token, json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": format!("mail__{tool}"), "arguments": arguments}})).await;

        return reply["result"].clone();
    }

    let reply = server.approved_agent_mcp(token, json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": format!("mail__{tool}"), "arguments": arguments}})).await;

    reply["result"].clone()
}

async fn add_email(server: &Server, imap: u16, smtp: u16, password: &str) -> (u16, Value) {
    add_email_named(server, "Mail", imap, smtp, password).await
}

fn payload(result: &Value) -> Value {
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[tokio::test]
async fn an_email_integration_is_verified_against_imap_and_smtp_and_stored_without_returning_the_password() {
    let server = Server::start().await;
    let (imap, smtp, _) = fake_mail().await;
    let post = |body: Value| server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&body));

    let (status, added) = add_email(&server, imap, smtp, PASSWORD).await;
    let stored = server.user.get("/api/v1/chat-tool-servers").send().await.unwrap().text().await.unwrap();
    let (_, wrong) = add_email_named(&server, "Other", imap, smtp, "wrong").await;
    let (_, no_password) = post(json!({"name": "Other", "kind": "email", "url": "", "account": USERNAME, "settings": mail_settings(imap, smtp)})).await;
    let (_, no_username) = post(json!({"name": "Other", "kind": "email", "url": "", "bearer_token": PASSWORD, "settings": mail_settings(imap, smtp)})).await;
    let (_, no_server) = post(json!({"name": "Other", "kind": "email", "url": "", "account": USERNAME, "bearer_token": PASSWORD, "settings": {"smtp_host": "smtp.example.com"}})).await;
    let (_, bad_port) = post(json!({"name": "Other", "kind": "email", "url": "", "account": USERNAME, "bearer_token": PASSWORD, "settings": {"imap_host": "a", "smtp_host": "b", "imap_port": 70000}})).await;

    assert_eq!(status, 201);
    assert_eq!((added["kind"].clone(), added["account"].clone(), added["token_hint"].clone()), (json!("email"), json!(USERNAME), json!("••••0123")));
    assert_eq!(added["settings"]["imap_port"], imap);
    let names: Vec<&str> = added["tools"].as_array().unwrap().iter().map(|tool| tool["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["list_accounts", "list_folders", "list_messages", "read_message", "search_messages", "delete_message", "move_message", "send_message"]);
    assert!(!stored.contains(PASSWORD));
    assert_eq!(wrong["error"], "tool_server_failed");
    assert!(wrong["message"].as_str().unwrap().contains("password"));
    assert_eq!((no_password["error"].clone(), no_username["error"].clone()), (json!("api_key_required"), json!("account_required")));
    assert_eq!((no_server["error"].clone(), bad_port["error"].clone()), (json!("settings_invalid"), json!("settings_invalid")));
    assert!(no_server["message"].as_str().unwrap().contains("IMAP server"));
}

#[tokio::test]
async fn an_email_integration_refuses_to_send_the_password_unencrypted() {
    let server = Server::start().await;
    let (imap, smtp, _) = fake_mail().await;
    let post = |settings: Value| server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Mail", "kind": "email", "url": "", "account": USERNAME, "bearer_token": PASSWORD, "settings": settings})));

    let (imap_status, imap_none) = post(json!({"imap_host": "127.0.0.1", "imap_port": imap, "imap_tls": "none", "smtp_host": "127.0.0.1", "smtp_port": smtp})).await;
    let (smtp_status, smtp_none) = post(json!({"imap_host": "127.0.0.1", "imap_port": imap, "smtp_host": "127.0.0.1", "smtp_port": smtp, "smtp_tls": "none"})).await;

    assert_eq!((imap_status, smtp_status), (400, 400));
    assert_eq!(imap_none["error"], "settings_invalid");
    assert_eq!(imap_none["message"], "Choose TLS or STARTTLS for IMAP. Mycelium does not send your password unencrypted.");
    assert_eq!(smtp_none["message"], "Choose TLS or STARTTLS for SMTP. Mycelium does not send your password unencrypted.");
}

#[tokio::test]
async fn email_tools_list_read_and_search_mail_and_never_fetch_attachments() {
    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, mail) = fake_mail().await;
    add_email(&server, imap, smtp, PASSWORD).await;

    let listed = mcp(&server, &token, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
    let send = listed["result"]["tools"].as_array().unwrap().iter().find(|tool| tool["name"] == "mail__send_message").unwrap();
    assert!(send["description"].as_str().unwrap().contains("needs the person's approval"));

    let folders = payload(&call(&server, &token, 2, "list_folders", json!({})).await);
    let messages = payload(&call(&server, &token, 3, "list_messages", json!({"folder": "INBOX", "limit": 10})).await);
    let read = payload(&call(&server, &token, 4, "read_message", json!({"id": "5"})).await);
    let html = payload(&call(&server, &token, 8, "read_message", json!({"id": "7"})).await);
    let found = payload(&call(&server, &token, 5, "search_messages", json!({"from": "grace", "subject": "lunch", "since": "2025-10-01"})).await);
    let missing = call(&server, &token, 6, "list_messages", json!({"folder": "Nowhere"})).await;
    let unfiltered = call(&server, &token, 7, "search_messages", json!({})).await;

    assert_eq!(folders, json!(["INBOX", "Sent"]));
    assert_eq!(messages[0]["id"], "7");
    assert_eq!((messages[0]["from"].clone(), messages[0]["subject"].clone()), (json!("Bob <bob@example.com>"), json!("Invoice")));
    assert_eq!((messages[1]["id"].clone(), messages[1]["subject"].clone(), messages[1]["date"].clone()), (json!("5"), json!("Lunch plans"), json!("2025-10-20T12:00:00Z")));
    assert_eq!(read["body"].as_str().unwrap().trim(), "Noon at the usual place?");
    assert_eq!(html["body"].as_str().unwrap().trim(), "Please pay.");
    assert_eq!((read["from"].clone(), read["to"].clone(), read["cc"].clone()), (json!("Grace Hopper <grace@example.com>"), json!(["Ada <ada@example.com>"]), json!(["Alan <alan@example.com>"])));
    assert_eq!(read["attachments"], json!([{"name": "menu.pdf", "size": 16}]));
    assert_eq!(found.as_array().unwrap().len(), 2);
    assert_eq!(*mail.attachment_bytes.lock().unwrap(), 0, "Attachment payload must stay on the IMAP server");
    assert_eq!((missing["isError"].clone(), unfiltered["isError"].clone()), (json!(true), json!(true)));

    let commands = mail.imap_commands.lock().unwrap().clone();
    assert!(commands.iter().any(|command| command.contains("UID SEARCH FROM \"grace\" SUBJECT \"lunch\" SINCE 1-Oct-2025")));
    assert!(commands.iter().any(|command| command.contains("1:2 (UID")));
    assert!(commands.iter().filter(|command| command.contains("EXAMINE")).count() >= 3);
    assert!(commands.iter().all(|command| !command.contains("BODY[") && !command.contains("STORE")));
}

#[tokio::test]
async fn sending_email_delivers_over_smtp_and_a_reply_threads_to_the_original() {
    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, mail) = fake_mail().await;
    add_email(&server, imap, smtp, PASSWORD).await;

    let sent = call(&server, &token, 1, "send_message", json!({"to": ["grace@example.com"], "cc": ["alan@example.com"], "subject": "Hello", "body": "Hi Grace."})).await;
    let replied = call(&server, &token, 2, "send_message", json!({"reply_to_id": "5", "body": "Noon works."})).await;
    let neither = call(&server, &token, 3, "send_message", json!({"body": "Hi"})).await;
    let unsubjected = call(&server, &token, 4, "send_message", json!({"to": ["grace@example.com"], "body": "Hi"})).await;

    assert_eq!((sent["isError"].clone(), replied["isError"].clone(), neither["isError"].clone(), unsubjected["isError"].clone()), (json!(false), json!(false), json!(true), json!(true)));

    let delivered = mail.sent.lock().unwrap().clone();
    assert_eq!(delivered.len(), 2);
    assert!(delivered[0].contains("To: grace@example.com") && delivered[0].contains("Cc: alan@example.com") && delivered[0].contains("Subject: Hello") && delivered[0].contains("Hi Grace."));
    assert!(delivered[0].contains("From: ada@example.com"));
    assert!(delivered[1].contains("Subject: Re: Lunch plans"));
    assert!(delivered[1].contains("In-Reply-To: <m5@example.com>"));
    assert!(delivered[1].contains("<m1@example.com> <m5@example.com>"));
    assert!(delivered[1].contains("grace@example.com"));
    assert_eq!(*mail.attachment_bytes.lock().unwrap(), 0, "Replying only needs the original message headers");
}

#[tokio::test]
async fn a_switched_off_email_integration_refuses_calls() {
    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, mail) = fake_mail().await;
    let (_, added) = add_email(&server, imap, smtp, PASSWORD).await;
    let logins = mail.imap_commands.lock().unwrap().len();
    server.user.json(server.user.patch(&format!("/api/v1/chat-tool-servers/{}", added["id"].as_str().unwrap())).json(&json!({"enabled": false}))).await;

    let called = call(&server, &token, 1, "list_folders", json!({})).await;

    assert_eq!(called["isError"], true);
    assert_eq!(mail.imap_commands.lock().unwrap().len(), logins);
}

#[tokio::test]
async fn deleting_an_email_removes_only_its_uid_and_leaves_other_deleted_messages_alone() {
    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, mail) = fake_mail().await;
    add_email(&server, imap, smtp, PASSWORD).await;

    let deleted = call(&server, &token, 1, "delete_message", json!({"id": "5"})).await;

    assert_eq!(deleted["isError"], false, "{deleted}");
    assert_eq!(*mail.removed.lock().unwrap(), [5]);
    assert_eq!(*mail.marked.lock().unwrap(), [7]);
    assert_eq!(*mail.attachment_bytes.lock().unwrap(), 0);
}

#[tokio::test]
async fn moving_within_an_account_uses_uid_move_without_fetching_the_message() {
    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, mail) = fake_mail().await;
    add_email(&server, imap, smtp, PASSWORD).await;

    let moved = call(&server, &token, 1, "move_message", json!({"id": "5", "destination_folder": "Sent"})).await;

    assert_eq!(moved["isError"], false, "{moved}");
    assert_eq!(*mail.moved.lock().unwrap(), [(5, "\"Sent\"".to_string())]);
    assert_eq!(*mail.marked.lock().unwrap(), [7]);
    assert_eq!(*mail.attachment_bytes.lock().unwrap(), 0);
}

#[tokio::test]
async fn a_server_without_safe_uid_operations_is_refused_before_mutating_mail() {
    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, mail) = fake_mail().await;
    *mail.capabilities.lock().unwrap() = Some("IMAP4rev1".into());
    add_email(&server, imap, smtp, PASSWORD).await;

    for (id, tool, arguments) in [(1, "delete_message", json!({"id": "5"})), (2, "move_message", json!({"id": "5", "destination_folder": "Sent"}))] {
        let refused = call(&server, &token, id, tool, arguments).await;

        assert_eq!(refused["isError"], true);
        assert!(refused["content"][0]["text"].as_str().unwrap().contains("support"));
    }

    assert!(mail.removed.lock().unwrap().is_empty());
    assert_eq!(*mail.marked.lock().unwrap(), [7]);
}

#[tokio::test]
async fn moving_between_accounts_preserves_mime_flags_and_date_before_deleting_the_source() {
    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, source) = fake_mail().await;
    add_email(&server, imap, smtp, PASSWORD).await;
    let (destination_imap, destination_smtp, destination) = fake_mail().await;
    let (_, account) = add_email_named(&server, "Archive", destination_imap, destination_smtp, PASSWORD).await;

    let accounts = payload(&call(&server, &token, 1, "list_accounts", json!({})).await);
    let moved = call(&server, &token, 2, "move_message", json!({"id": "5", "destination_account": account["id"], "destination_folder": "Sent"})).await;

    assert_eq!(moved["isError"], false, "{moved}");
    assert!(accounts.as_array().unwrap().iter().any(|item| item["id"] == account["id"] && item["name"] == "Archive"));
    assert!(!accounts.to_string().contains(PASSWORD));
    let appended = destination.appended.lock().unwrap();
    assert_eq!(appended.len(), 1);
    assert_eq!(appended[0].1, fake_email::first_message().as_bytes());
    assert!(appended[0].0.contains("\\Seen") && appended[0].0.contains("\\Flagged") && appended[0].0.contains("custom"));
    assert!(!appended[0].0.contains("\\Recent") && !appended[0].0.contains("\\Deleted"));
    assert!(appended[0].0.contains("20-Oct-2025 12:00:00 +0000"));
    assert_eq!(*source.removed.lock().unwrap(), [5]);
    assert_eq!(*source.marked.lock().unwrap(), [7]);
    assert!(!moved.to_string().contains("aGVsbG8gd29ybGQh"));
}

#[tokio::test]
async fn a_failed_destination_append_leaves_the_source_intact() {
    use std::sync::atomic::Ordering;

    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, source) = fake_mail().await;
    add_email(&server, imap, smtp, PASSWORD).await;
    let (destination_imap, destination_smtp, destination) = fake_mail().await;
    let (_, account) = add_email_named(&server, "Archive", destination_imap, destination_smtp, PASSWORD).await;
    destination.reject_append.store(true, Ordering::SeqCst);

    let moved = call(&server, &token, 1, "move_message", json!({"id": "5", "destination_account": account["id"], "destination_folder": "Sent"})).await;

    assert_eq!(moved["isError"], true);
    assert!(moved["content"][0]["text"].as_str().unwrap().contains("source"));
    assert!(destination.appended.lock().unwrap().is_empty());
    assert!(source.removed.lock().unwrap().is_empty());
    assert_eq!(*source.marked.lock().unwrap(), [7]);
}

#[tokio::test]
async fn a_failed_source_delete_reports_the_retained_destination_copy_and_restores_source_flags() {
    use std::sync::atomic::Ordering;

    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, source) = fake_mail().await;
    add_email(&server, imap, smtp, PASSWORD).await;
    source.reject_delete.store(true, Ordering::SeqCst);
    let (destination_imap, destination_smtp, destination) = fake_mail().await;
    let (_, account) = add_email_named(&server, "Archive", destination_imap, destination_smtp, PASSWORD).await;

    let moved = call(&server, &token, 1, "move_message", json!({"id": "5", "destination_account": account["id"], "destination_folder": "Sent"})).await;

    assert_eq!(moved["isError"], true);
    assert!(moved["content"][0]["text"].as_str().unwrap().contains("copy"));
    assert_eq!(destination.appended.lock().unwrap().len(), 1);
    assert!(source.removed.lock().unwrap().is_empty());
    assert_eq!(*source.marked.lock().unwrap(), [7]);
}

#[tokio::test]
async fn a_timeout_after_the_destination_copy_reports_the_retained_copy() {
    use std::sync::atomic::Ordering;

    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, source) = fake_mail().await;
    add_email(&server, imap, smtp, PASSWORD).await;
    source.hang_delete.store(true, Ordering::SeqCst);
    let (destination_imap, destination_smtp, destination) = fake_mail().await;
    let (_, account) = add_email_named(&server, "Archive", destination_imap, destination_smtp, PASSWORD).await;

    let (moved, ()) = tokio::join!(
        call(&server, &token, 1, "move_message", json!({"id": "5", "destination_account": account["id"], "destination_folder": "Sent"})),
        async {
            source.delete_reached.notified().await;
            tokio::time::pause();
            tokio::time::advance(std::time::Duration::from_secs(46)).await;
        }
    );

    assert_eq!(moved["isError"], true);
    assert!(moved["content"][0]["text"].as_str().unwrap().contains("complete copy is retained"));
    assert!(moved["content"][0]["text"].as_str().unwrap().contains("timed out"));
    assert_eq!(destination.appended.lock().unwrap().len(), 1);
    assert!(source.removed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_agent_deleting_mail_waits_for_the_person_and_runs_once_allowed() {
    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, mail) = fake_mail().await;
    add_email(&server, imap, smtp, PASSWORD).await;
    let conversation = server.node_conversation(&token).await;

    let (reply, ()) = tokio::join!(agent_call(&server, &token, conversation, 1, "delete_message", json!({"id": "5"})), async {
        let request = server.open_request(conversation).await;

        assert_eq!(request["toolCall"]["title"], "mail__delete_message");
        assert!(mail.removed.lock().unwrap().is_empty());
        assert_eq!(server.conversation(conversation).await["status"], "input");

        server.answer(conversation, &request, "approve").await;
    });

    assert_eq!(reply["isError"], false);
    assert_eq!(*mail.removed.lock().unwrap(), [5]);
}

#[tokio::test]
async fn an_agent_deleting_mail_is_refused_when_the_person_declines() {
    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, mail) = fake_mail().await;
    add_email(&server, imap, smtp, PASSWORD).await;
    let conversation = server.node_conversation(&token).await;

    let (reply, ()) = tokio::join!(agent_call(&server, &token, conversation, 1, "delete_message", json!({"id": "5"})), async {
        let request = server.open_request(conversation).await;

        server.answer(conversation, &request, "deny").await;
    });

    assert_eq!(reply["isError"], true);
    assert!(reply["content"][0]["text"].as_str().unwrap().contains("declined"));
    assert!(mail.removed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_agent_reading_mail_needs_no_approval() {
    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, _) = fake_mail().await;
    add_email(&server, imap, smtp, PASSWORD).await;
    let conversation = server.node_conversation(&token).await;

    let reply = agent_call(&server, &token, conversation, 1, "list_folders", json!({})).await;

    assert_eq!(reply["isError"], false);
}

#[tokio::test]
async fn cross_account_destinations_must_be_owned_enabled_email_integrations() {
    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, source) = fake_mail().await;
    add_email(&server, imap, smtp, PASSWORD).await;
    let (destination_imap, destination_smtp, destination) = fake_mail().await;
    let (_, account) = add_email_named(&server, "Archive", destination_imap, destination_smtp, PASSWORD).await;
    let other = server.second_user("other@example.com").await;
    let (_, foreign) = other.json(other.post("/api/v1/chat-tool-servers").json(&json!({"name": "Other", "kind": "email", "url": "", "account": USERNAME, "bearer_token": PASSWORD, "settings": mail_settings(destination_imap, destination_smtp)}))).await;
    let foreign_attempt = call(&server, &token, 1, "move_message", json!({"id": "5", "destination_account": foreign["id"], "destination_folder": "Sent"})).await;
    server.user.json(server.user.patch(&format!("/api/v1/chat-tool-servers/{}", account["id"].as_str().unwrap())).json(&json!({"enabled": false}))).await;
    let disabled_attempt = call(&server, &token, 2, "move_message", json!({"id": "5", "destination_account": account["id"], "destination_folder": "Sent"})).await;
    sqlx::query("update chat_tool_servers set enabled = true, kind = 'mcp' where id = $1").bind(account["id"].as_str().unwrap().parse::<uuid::Uuid>().unwrap()).execute(&server.pool).await.unwrap();
    let wrong_kind = call(&server, &token, 3, "move_message", json!({"id": "5", "destination_account": account["id"], "destination_folder": "Sent"})).await;

    for refused in [foreign_attempt, disabled_attempt, wrong_kind] {
        assert_eq!(refused["isError"], true);
        assert!(refused["content"][0]["text"].as_str().unwrap().contains("destination"));
    }

    assert!(destination.appended.lock().unwrap().is_empty());
    assert!(source.removed.lock().unwrap().is_empty());
    let accounts = payload(&call(&server, &token, 4, "list_accounts", json!({})).await);
    assert_eq!(accounts.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn oversized_messages_are_refused_before_their_attachments_are_downloaded() {
    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, source) = fake_mail().await;
    add_email(&server, imap, smtp, PASSWORD).await;
    *source.reported_size.lock().unwrap() = Some(20 * 1024 * 1024 + 1);
    let (destination_imap, destination_smtp, destination) = fake_mail().await;
    let (_, account) = add_email_named(&server, "Archive", destination_imap, destination_smtp, PASSWORD).await;

    let moved = call(&server, &token, 1, "move_message", json!({"id": "5", "destination_account": account["id"], "destination_folder": "Sent"})).await;

    assert_eq!(moved["isError"], true);
    assert!(moved["content"][0]["text"].as_str().unwrap().contains("20 MB"));
    assert_eq!(*source.attachment_bytes.lock().unwrap(), 0);
    assert!(source.removed.lock().unwrap().is_empty());
    assert!(destination.appended.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_size_mismatch_is_refused_without_copying_or_deleting_partial_mail() {
    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, source) = fake_mail().await;
    add_email(&server, imap, smtp, PASSWORD).await;
    *source.reported_size.lock().unwrap() = Some(1);
    let (destination_imap, destination_smtp, destination) = fake_mail().await;
    let (_, account) = add_email_named(&server, "Archive", destination_imap, destination_smtp, PASSWORD).await;

    let moved = call(&server, &token, 1, "move_message", json!({"id": "5", "destination_account": account["id"], "destination_folder": "Sent"})).await;

    assert_eq!(moved["isError"], true);
    assert!(moved["content"][0]["text"].as_str().unwrap().contains("incomplete or oversized"));
    assert!(source.removed.lock().unwrap().is_empty());
    assert!(destination.appended.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_over_budget_imap_response_is_aborted_without_copying_or_deleting() {
    use std::sync::atomic::Ordering;

    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, source) = fake_mail().await;
    add_email(&server, imap, smtp, PASSWORD).await;
    source.transfer_overrun.store(true, Ordering::SeqCst);
    let (destination_imap, destination_smtp, destination) = fake_mail().await;
    let (_, account) = add_email_named(&server, "Archive", destination_imap, destination_smtp, PASSWORD).await;

    let moved = call(&server, &token, 1, "move_message", json!({"id": "5", "destination_account": account["id"], "destination_folder": "Sent"})).await;

    assert_eq!(moved["isError"], true);
    assert!(moved["content"][0]["text"].as_str().unwrap().contains("source was not changed"));
    assert!(source.removed.lock().unwrap().is_empty());
    assert!(destination.appended.lock().unwrap().is_empty());
}

#[tokio::test]
async fn malformed_mutation_locations_are_refused_without_defaulting_to_another_account_or_folder() {
    let server = Server::start().await;
    let _node = server.online_node("laptop").await;
    let token = _node.token.clone();
    let (imap, smtp, source) = fake_mail().await;
    add_email(&server, imap, smtp, PASSWORD).await;

    for (id, arguments) in [
        (1, json!({"id": "5", "destination_folder": "Sent", "destination_account": 7})),
        (2, json!({"id": "5", "destination_folder": "Sent", "destination_account": ""})),
        (3, json!({"id": "5", "folder": 7, "destination_folder": "Sent"})),
    ] {
        let moved = call(&server, &token, id, "move_message", arguments).await;

        assert_eq!(moved["isError"], true, "A malformed location must not default to a destructive operation");
    }

    assert!(source.removed.lock().unwrap().is_empty());
    assert!(source.moved.lock().unwrap().is_empty());
}
