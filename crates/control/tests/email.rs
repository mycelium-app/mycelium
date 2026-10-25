mod common;

#[path = "common/fake_email.rs"]
mod fake_email;

use common::Server;
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

async fn call(server: &Server, token: &str, id: u64, tool: &str, arguments: Value) -> Value {
    let reply = mcp(server, token, json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": format!("mail__{tool}"), "arguments": arguments}})).await;

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
    assert_eq!(names, ["list_folders", "list_messages", "read_message", "search_messages", "send_message"]);
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
    let (_, token) = server.join("node", "laptop").await;
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
    let (_, token) = server.join("node", "laptop").await;
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
    let (_, token) = server.join("node", "laptop").await;
    let (imap, smtp, mail) = fake_mail().await;
    let (_, added) = add_email(&server, imap, smtp, PASSWORD).await;
    let logins = mail.imap_commands.lock().unwrap().len();
    server.user.json(server.user.patch(&format!("/api/v1/chat-tool-servers/{}", added["id"].as_str().unwrap())).json(&json!({"enabled": false}))).await;

    let called = call(&server, &token, 1, "list_folders", json!({})).await;

    assert_eq!(called["isError"], true);
    assert_eq!(mail.imap_commands.lock().unwrap().len(), logins);
}
