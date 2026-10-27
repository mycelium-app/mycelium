mod common;

use base64::Engine;
use common::{Client, NodeLink, Server};
use mycelium_proto::link::{ControlToNode, GitCommand, NodeToControl};
use serde_json::{Value, json};
use uuid::Uuid;

const TOKEN: &str = "host-secret-0123456789abcdef";
const PDF: [u8; 8] = [37, 80, 68, 70, 45, 49, 46, 55];

async fn create(client: &Client, body: Value) -> (u16, Value) {
    client.json(client.post("/api/v1/artifacts").json(&body)).await
}

async fn created(client: &Client, kind: &str, title: &str, content: &str) -> Value {
    let (status, artifact) = create(client, json!({"kind": kind, "title": title, "content": content})).await;
    assert_eq!(status, 201, "{artifact}");

    artifact
}

async fn content_of(client: &Client, id: &str, version: i64) -> (reqwest::header::HeaderMap, Vec<u8>) {
    let response = client.get(&format!("/api/v1/artifacts/{id}/versions/{version}/content")).send().await.unwrap();
    let headers = response.headers().clone();

    (headers, response.bytes().await.unwrap().to_vec())
}

async fn started(server: &Server) -> (NodeLink, Uuid) {
    let mut link = server.online_node("laptop").await;
    let conversation = server.create_conversation(link.id, Some("hello")).await;
    link.expect_start_session().await;

    (link, conversation.id)
}

async fn tool(server: &Server, link: &NodeLink, conversation: Uuid, name: &str, arguments: Value) -> (bool, String) {
    let anonymous = server.anonymous();
    let request = anonymous
        .post("/api/v1/mcp")
        .header("authorization", format!("Bearer {}", link.token))
        .header("x-mycelium-conversation", conversation.to_string())
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}}));
    let (status, body) = anonymous.json(request).await;
    assert_eq!(status, 200, "{body}");

    (body["result"]["isError"].as_bool().unwrap(), body["result"]["content"][0]["text"].as_str().unwrap().to_string())
}

fn artifact_id(text: &str) -> String {
    text.split("(artifact ").nth(1).unwrap().split(',').next().unwrap().to_string()
}

async fn grant(client: &Client, artifact: &Value, conversation: Uuid) {
    let id = artifact["id"].as_str().unwrap();
    let (status, body) = client.json(client.post(&format!("/api/v1/artifacts/{id}/links")).json(&json!({"conversation_id": conversation, "permission": "edit"}))).await;
    assert_eq!(status, 200, "{body}");
}

async fn design_of(client: &Client, id: &str) -> Value {
    let (_, artifact) = client.json(client.get(&format!("/api/v1/artifacts/{id}"))).await;
    let (_, bytes) = content_of(client, id, artifact["latest_version"].as_i64().unwrap()).await;

    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn each_kind_is_created_versioned_listed_and_served() {
    let server = Server::start().await;
    let user = &server.user;
    let design = r##"{"pages":[{"id":"p1","name":"Page 1","items":[{"id":"s1","type":"rect","x":0,"y":0,"w":10,"h":10,"fill":"#ffffff","stroke":null,"radius":0}]}]}"##;
    let edited_design = design.replace("#ffffff", "#000000");
    let cases = [("design", "Landing sketch", design.to_string(), edited_design), ("page", "Planning board", "<h1>Plan</h1>".to_string(), "<h1>Plan v2</h1>".to_string()), ("markdown", "Notes", "# Notes".to_string(), "# Notes\n\nMore".to_string())];

    for (kind, title, first, second) in cases {
        let artifact = created(user, kind, title, &first).await;
        let id = artifact["id"].as_str().unwrap();
        let (status, versioned) = user.json(user.post(&format!("/api/v1/artifacts/{id}/versions")).json(&json!({"content": second, "note": "Second pass"}))).await;
        let (_, first_content) = content_of(user, id, 1).await;
        let (_, second_content) = content_of(user, id, 2).await;

        assert_eq!((artifact["kind"].as_str(), artifact["latest_version"].as_i64(), artifact["public"].as_bool()), (Some(kind), Some(1), Some(false)), "{artifact}");
        assert_eq!((status, versioned["latest_version"].as_i64(), versioned["versions"][0]["author"].as_str(), versioned["versions"][0]["note"].as_str()), (201, Some(2), Some("person"), Some("Second pass")), "{versioned}");
        assert_eq!((String::from_utf8(first_content).unwrap(), String::from_utf8(second_content).unwrap()), (first, second));
    }

    let (_, everything) = user.json(user.get("/api/v1/artifacts")).await;
    let (_, pages) = user.json(user.get("/api/v1/artifacts?kind=page")).await;
    let (_, found) = user.json(user.get("/api/v1/artifacts?q=notes")).await;
    let (invalid, _) = user.json(user.get("/api/v1/artifacts?kind=deck")).await;

    assert_eq!(everything.as_array().unwrap().iter().map(|artifact| artifact["title"].as_str().unwrap()).collect::<Vec<_>>(), ["Notes", "Planning board", "Landing sketch"]);
    assert_eq!(pages.as_array().unwrap().len(), 1);
    assert_eq!(found[0]["slug"], "notes");
    assert_eq!(invalid, 400);
}

#[tokio::test]
async fn a_file_is_stored_from_base64_and_served_with_its_media_type() {
    let server = Server::start().await;
    let user = &server.user;
    let encoded = base64::engine::general_purpose::STANDARD.encode(PDF);

    let (status, artifact) = create(user, json!({"kind": "file", "title": "Quarterly report", "content_base64": encoded, "media_type": "application/pdf"})).await;
    let (headers, bytes) = content_of(user, artifact["id"].as_str().unwrap(), 1).await;
    let (_, plain) = create(user, json!({"kind": "file", "title": "Blob", "content_base64": encoded})).await;
    let (plain_headers, _) = content_of(user, plain["id"].as_str().unwrap(), 1).await;

    assert_eq!(status, 201, "{artifact}");
    assert_eq!((headers["content-type"].to_str().unwrap(), bytes), ("application/pdf", PDF.to_vec()));
    assert_eq!(plain_headers["content-type"], "application/octet-stream");
}

#[tokio::test]
async fn page_content_is_served_sandboxed_so_its_scripts_cannot_reach_the_session() {
    let server = Server::start().await;
    let page = created(&server.user, "page", "Board", "<script>fetch('/api/v1/me')</script>").await;
    let markdown = created(&server.user, "markdown", "Notes", "# Notes").await;

    let (page_headers, _) = content_of(&server.user, page["id"].as_str().unwrap(), 1).await;
    let (markdown_headers, _) = content_of(&server.user, markdown["id"].as_str().unwrap(), 1).await;

    assert_eq!(page_headers["content-security-policy"], "sandbox allow-scripts");
    assert_eq!(page_headers["x-content-type-options"], "nosniff");
    assert_eq!(page_headers["content-type"], "text/html; charset=utf-8");
    assert_eq!(markdown_headers["content-security-policy"], "sandbox");
}

#[tokio::test]
async fn a_design_made_without_content_starts_with_one_empty_page() {
    let server = Server::start().await;
    let (status, artifact) = create(&server.user, json!({"kind": "design", "title": "Blank"})).await;

    assert_eq!(status, 201, "{artifact}");
    assert_eq!(design_of(&server.user, artifact["id"].as_str().unwrap()).await, json!({"pages": [{"id": "p1", "name": "Page 1", "items": []}]}));
}

#[tokio::test]
async fn content_that_cannot_be_kept_is_refused_with_what_to_do() {
    let server = Server::start().await;
    let user = &server.user;

    let (no_kind, kind_error) = create(user, json!({"kind": "deck", "title": "x", "content": "x"})).await;
    let (no_title, title_error) = create(user, json!({"kind": "page", "title": "  ", "content": "x"})).await;
    let (no_content, _) = create(user, json!({"kind": "page", "title": "x"})).await;
    let (empty, _) = create(user, json!({"kind": "markdown", "title": "x", "content": ""})).await;
    let (both, _) = create(user, json!({"kind": "page", "title": "x", "content": "x", "content_base64": "eA=="})).await;
    let (bad_base64, _) = create(user, json!({"kind": "file", "title": "x", "content_base64": "not base64!"})).await;
    let (bad_design, design_error) = create(user, json!({"kind": "design", "title": "x", "content": "{\"pages\": 3}"})).await;
    let (bad_type, _) = create(user, json!({"kind": "file", "title": "x", "content": "x", "media_type": "no spaces/here please"})).await;
    let (huge, huge_error) = create(user, json!({"kind": "file", "title": "x", "content": "a".repeat(20 * 1024 * 1024 + 1)})).await;
    let (stranger, _) = create(user, json!({"kind": "page", "title": "x", "content": "x", "conversation_id": Uuid::new_v4()})).await;

    assert_eq!((no_kind, kind_error["message"].as_str()), (400, Some("Kind must be design, page, markdown or file.")));
    assert_eq!((no_title, title_error["error"].as_str()), (400, Some("title_required")));
    assert_eq!((no_content, empty, both, bad_base64, bad_type, stranger), (400, 400, 400, 400, 400, 404));
    assert_eq!((bad_design, design_error["error"].as_str()), (400, Some("design_invalid")));
    assert_eq!((huge, huge_error["error"].as_str()), (413, Some("artifact_too_large")));
}

#[tokio::test]
async fn two_artifacts_with_one_title_get_distinct_slugs() {
    let server = Server::start_with(|config| config.artifact_domain = Some("artifacts.example.com".into())).await;

    let first = created(&server.user, "page", "Planning Board!", "a").await;
    let second = created(&server.user, "page", "Planning Board!", "b").await;
    let untitled = created(&server.user, "page", "???", "c").await;

    assert_eq!(first["slug"], "planning-board");
    assert_eq!(first["url"], "https://planning-board.artifacts.example.com");
    assert!(second["slug"].as_str().unwrap().starts_with("planning-board-"), "{second}");
    assert_eq!(untitled["slug"], "artifact");
}

#[tokio::test]
async fn an_artifact_is_renamed_made_public_and_deleted() {
    let server = Server::start().await;
    let user = &server.user;
    let artifact = created(user, "page", "Board", "x").await;
    let id = artifact["id"].as_str().unwrap();

    let (status, renamed) = user.json(user.patch(&format!("/api/v1/artifacts/{id}")).json(&json!({"title": "Roadmap"}))).await;
    let (_, public) = user.json(user.patch(&format!("/api/v1/artifacts/{id}")).json(&json!({"public": true}))).await;
    let (_, private) = user.json(user.patch(&format!("/api/v1/artifacts/{id}")).json(&json!({"public": false}))).await;
    let (blank, _) = user.json(user.patch(&format!("/api/v1/artifacts/{id}")).json(&json!({"title": " "}))).await;
    let deleted = user.delete(&format!("/api/v1/artifacts/{id}")).send().await.unwrap().status().as_u16();
    let (gone, _) = user.json(user.get(&format!("/api/v1/artifacts/{id}"))).await;

    assert_eq!((status, renamed["title"].as_str(), renamed["slug"].as_str(), renamed["public"].as_bool()), (200, Some("Roadmap"), Some("board"), Some(false)));
    assert_eq!((public["public"].as_bool(), public["title"].as_str(), private["public"].as_bool()), (Some(true), Some("Roadmap"), Some(false)));
    assert_eq!((blank, deleted, gone), (400, 204, 404));
}

#[tokio::test]
async fn comments_are_added_listed_and_resolved_and_the_data_store_keeps_json() {
    let server = Server::start().await;
    let user = &server.user;
    let artifact = created(user, "design", "Landing", r#"{"pages":[{"id":"p1","items":[]}]}"#).await;
    let id = artifact["id"].as_str().unwrap();

    let (status, comment) = user.json(user.post(&format!("/api/v1/artifacts/{id}/comments")).json(&json!({"body": "Make the logo bigger", "anchor": {"page": "p1", "item": "a1", "x": 10, "y": 20}}))).await;
    let (_, signed) = user.json(user.post(&format!("/api/v1/artifacts/{id}/comments")).json(&json!({"body": "Fine", "author_name": "Reviewer"}))).await;
    let (empty, empty_error) = user.json(user.post(&format!("/api/v1/artifacts/{id}/comments")).json(&json!({"body": "  "}))).await;
    let comment_id = comment["id"].as_str().unwrap();
    let (resolved_status, resolved) = user.json(user.patch(&format!("/api/v1/artifacts/{id}/comments/{comment_id}")).json(&json!({"resolved": true}))).await;
    let (missing, _) = user.json(user.patch(&format!("/api/v1/artifacts/{id}/comments/{}", Uuid::new_v4())).json(&json!({"resolved": true}))).await;
    let (_, listed) = user.json(user.get(&format!("/api/v1/artifacts/{id}/comments"))).await;

    assert_eq!((status, comment["anchor"]["item"].as_str(), comment["resolved"].as_bool()), (201, Some("a1"), Some(false)));
    assert_eq!(signed["author_name"], "Reviewer");
    assert_eq!((empty, empty_error["message"].as_str()), (400, Some("Write the comment before sending it.")));
    assert_eq!((resolved_status, resolved["resolved"].as_bool(), missing), (200, Some(true), 404));
    assert_eq!(listed.as_array().unwrap().iter().map(|comment| comment["resolved"].as_bool().unwrap()).collect::<Vec<_>>(), [true, false]);

    let (put_status, entry) = user.json(user.put(&format!("/api/v1/artifacts/{id}/data/row-1")).json(&json!({"call": "yours", "note": ["a"]}))).await;
    user.json(user.put(&format!("/api/v1/artifacts/{id}/data/row-1")).json(&json!({"call": "mine"}))).await;
    user.json(user.put(&format!("/api/v1/artifacts/{id}/data/row-0")).json(&json!(7))).await;
    let (_, entries) = user.json(user.get(&format!("/api/v1/artifacts/{id}/data"))).await;
    let (too_big, _) = user.json(user.put(&format!("/api/v1/artifacts/{id}/data/big")).json(&json!("a".repeat(1024 * 1024 + 1)))).await;

    assert_eq!((put_status, entry["key"].as_str(), entry["value"]["call"].as_str()), (200, Some("row-1"), Some("yours")));
    assert_eq!(entries.as_array().unwrap().iter().map(|entry| (entry["key"].as_str().unwrap(), entry["value"].clone())).collect::<Vec<_>>(), [("row-0", json!(7)), ("row-1", json!({"call": "mine"}))]);
    assert_eq!(too_big, 413);
}

#[tokio::test]
async fn another_person_sees_none_of_an_artifact() {
    let server = Server::start().await;
    let artifact = created(&server.user, "page", "Private", "x").await;
    let id = artifact["id"].as_str().unwrap();
    let other = server.second_user("other@example.com").await;

    let (listed_status, listed) = other.json(other.get("/api/v1/artifacts")).await;
    let attempts = [
        other.get(&format!("/api/v1/artifacts/{id}")),
        other.get(&format!("/api/v1/artifacts/{id}/versions/1/content")),
        other.patch(&format!("/api/v1/artifacts/{id}")).json(&json!({"public": true})),
        other.delete(&format!("/api/v1/artifacts/{id}")),
        other.post(&format!("/api/v1/artifacts/{id}/versions")).json(&json!({"content": "y"})),
        other.get(&format!("/api/v1/artifacts/{id}/comments")),
        other.post(&format!("/api/v1/artifacts/{id}/comments")).json(&json!({"body": "hi"})),
        other.get(&format!("/api/v1/artifacts/{id}/data")),
        other.put(&format!("/api/v1/artifacts/{id}/data/k")).json(&json!(1)),
    ];
    let mut statuses = Vec::new();

    for attempt in attempts {
        statuses.push(attempt.send().await.unwrap().status().as_u16());
    }

    let anonymous = server.anonymous().get(&format!("/api/v1/artifacts/{id}")).send().await.unwrap().status().as_u16();
    let (_, still_there) = server.user.json(server.user.get(&format!("/api/v1/artifacts/{id}"))).await;

    assert_eq!((listed_status, listed), (200, json!([])));
    assert_eq!(statuses, [404; 9]);
    assert_eq!((anonymous, still_there["title"].as_str(), still_there["latest_version"].as_i64()), (401, Some("Private"), Some(1)));
}

#[tokio::test]
async fn the_internal_api_refuses_without_or_with_a_wrong_token_and_works_with_the_right_one() {
    let server = Server::start_with(|config| config.artifact_host_token = Some(TOKEN.into())).await;
    let artifact = created(&server.user, "page", "Board", "<p>hi</p>").await;
    let other = server.second_user("other@example.com").await;
    let slug = artifact["slug"].as_str().unwrap();
    let anonymous = server.anonymous();
    let get = |path: String, token: Option<&str>| {
        let request = anonymous.get(&path);

        match token {
            Some(token) => request.header("authorization", format!("Bearer {token}")),
            None => request,
        }
    };
    let by_slug = format!("/internal/artifacts/by-slug/{slug}");

    let missing = get(by_slug.clone(), None).send().await.unwrap().status().as_u16();
    let wrong = get(by_slug.clone(), Some("nope")).send().await.unwrap().status().as_u16();
    let person = other.get(&by_slug).send().await.unwrap().status().as_u16();
    let (status, hosted) = anonymous.json(get(by_slug.clone(), Some(TOKEN))).await;
    let served = get(format!("{by_slug}/content"), Some(TOKEN)).send().await.unwrap();
    let headers = served.headers().clone();
    let body = served.text().await.unwrap();
    let (unknown, _) = anonymous.json(get("/internal/artifacts/by-slug/nothing".into(), Some(TOKEN))).await;

    assert_eq!((missing, wrong, person), (401, 401, 401));
    assert_eq!((status, hosted["title"].as_str(), hosted["media_type"].as_str(), hosted["public"].as_bool()), (200, Some("Board"), Some("text/html"), Some(false)), "{hosted}");
    assert_eq!(hosted["sha256"].as_str().unwrap().len(), 64);
    assert_eq!((body.as_str(), headers["content-security-policy"].to_str().unwrap()), ("<p>hi</p>", "sandbox allow-scripts"));
    assert_eq!(unknown, 404);

    let put = anonymous.put(&format!("{by_slug}/data/choice")).header("authorization", format!("Bearer {TOKEN}")).json(&json!({"pick": 2}));
    let (put_status, _) = anonymous.json(put).await;
    let (_, data) = anonymous.json(get(format!("{by_slug}/data"), Some(TOKEN))).await;
    let denied = anonymous.put(&format!("{by_slug}/data/choice")).json(&json!({"pick": 3})).send().await.unwrap().status().as_u16();
    let (_, own_data) = server.user.json(server.user.get(&format!("/api/v1/artifacts/{}/data", artifact["id"].as_str().unwrap()))).await;

    assert_eq!((put_status, denied), (200, 401));
    assert_eq!(data, json!([{"key": "choice", "value": {"pick": 2}, "updated_at": data[0]["updated_at"]}]));
    assert_eq!(own_data[0]["value"], json!({"pick": 2}));
}

#[tokio::test]
async fn the_host_saves_a_new_version_only_for_the_signed_in_owner_of_a_file() {
    let server = Server::start_with(|config| config.artifact_host_token = Some(TOKEN.into())).await;
    let (status, file) = create(&server.user, json!({"kind": "file", "title": "Report", "content_base64": "JVBERi0xLjQ=", "media_type": "application/pdf"})).await;
    let page = created(&server.user, "page", "Board", "<p>hi</p>").await;
    let owner: uuid::Uuid = file["owner"].as_str().unwrap().parse().unwrap();
    sqlx::query("update users set subject = 'oidc:owner-sub' where id = $1").bind(owner).execute(&server.pool).await.unwrap();
    sqlx::query("insert into users (subject, email, name) values ('oidc:other-sub', 'other@example.com', 'Other')").execute(&server.pool).await.unwrap();
    let anonymous = server.anonymous();
    let slug = file["slug"].as_str().unwrap();
    let save = |slug: &str, subject: &str, token: &str| anonymous.post(&format!("/internal/artifacts/by-slug/{slug}/versions")).query(&[("subject", subject)]).header("authorization", format!("Bearer {token}")).body(b"%PDF-1.4 annotated".to_vec());

    let (wrong_token, _) = anonymous.json(save(slug, "owner-sub", "nope")).await;
    let (stranger, refusal) = anonymous.json(save(slug, "other-sub", TOKEN)).await;
    let (unknown_person, _) = anonymous.json(save(slug, "nobody", TOKEN)).await;
    let (not_a_file, _) = anonymous.json(save(page["slug"].as_str().unwrap(), "owner-sub", TOKEN)).await;
    let (still_one, unchanged) = server.user.json(server.user.get(&format!("/api/v1/artifacts/{}", file["id"].as_str().unwrap()))).await;
    let (saved_status, saved) = anonymous.json(save(slug, "owner-sub", TOKEN)).await;
    let (_, bytes) = content_of(&server.user, file["id"].as_str().unwrap(), 2).await;

    assert_eq!(status, 201, "{file}");
    assert_eq!((wrong_token, stranger, unknown_person, not_a_file), (401, 403, 403, 400));
    assert_eq!(refusal["error"], "edit_forbidden");
    assert_eq!((still_one, unchanged["latest_version"].as_i64()), (200, Some(1)));
    assert_eq!((saved_status, saved["latest_version"].as_i64()), (201, Some(2)), "{saved}");
    assert_eq!(bytes, b"%PDF-1.4 annotated");
}

#[tokio::test]
async fn the_internal_api_is_off_while_no_token_is_configured() {
    let server = Server::start().await;
    let artifact = created(&server.user, "page", "Board", "x").await;
    let slug = artifact["slug"].as_str().unwrap();
    let anonymous = server.anonymous();

    let request = anonymous.get(&format!("/internal/artifacts/by-slug/{slug}")).header("authorization", "Bearer ");
    let (status, _) = anonymous.json(request).await;

    assert_eq!(status, 404);
}

#[tokio::test]
async fn the_design_editor_is_served_to_signed_in_people_only() {
    let server = Server::start().await;
    let editor = server.web_dir.parent().unwrap().join("editor");
    std::fs::create_dir_all(editor.join("assets")).unwrap();
    std::fs::write(editor.join("index.html"), "<title>Design editor</title>").unwrap();
    std::fs::write(editor.join("assets/app.js"), "console.log('editor')").unwrap();
    let id = Uuid::new_v4();

    let index = server.user.get("/design/").send().await.unwrap().text().await.unwrap();
    let deep = server.user.get(&format!("/design/{id}")).send().await.unwrap().text().await.unwrap();
    let asset = server.user.get("/design/assets/app.js").send().await.unwrap().text().await.unwrap();
    let anonymous = server.anonymous().get(&format!("/design/{id}")).send().await.unwrap();

    assert_eq!((index.as_str(), deep.as_str(), asset.as_str()), ("<title>Design editor</title>", "<title>Design editor</title>", "console.log('editor')"));
    assert_eq!((anonymous.status().as_u16(), anonymous.headers()["location"].to_str().unwrap()), (303, "/auth/login"));
}

#[tokio::test]
async fn an_agent_creates_an_artifact_from_text_and_updates_it() {
    let server = Server::start().await;
    let (link, conversation) = started(&server).await;

    let (failed, created_text) = tool(&server, &link, conversation, "create_artifact", json!({"kind": "page", "title": "Planning board", "content": "<h1>Plan</h1>"})).await;
    let id = artifact_id(&created_text);
    let (_, artifact) = server.user.json(server.user.get(&format!("/api/v1/artifacts/{id}"))).await;
    let (updated_failed, updated_text) = tool(&server, &link, conversation, "update_artifact", json!({"artifact": id, "content": "<h1>Plan v2</h1>", "note": "Added the second column"})).await;
    let (_, by_slug) = tool(&server, &link, conversation, "update_artifact", json!({"artifact": "planning-board", "content": "<h1>Plan v3</h1>"})).await;
    let (_, after) = server.user.json(server.user.get(&format!("/api/v1/artifacts/{id}"))).await;
    let (empty_failed, empty_text) = tool(&server, &link, conversation, "update_artifact", json!({"artifact": id})).await;

    assert_eq!((failed, created_text.as_str()), (false, format!("Created page \"Planning board\" (artifact {id}, version 1).").as_str()));
    assert_eq!((artifact["conversation_id"].as_str(), artifact["versions"][0]["author"].as_str()), (Some(conversation.to_string().as_str()), Some("agent")));
    assert_eq!((updated_failed, updated_text.as_str()), (false, "Updated \"Planning board\" to version 2."));
    assert_eq!(by_slug, "Updated \"Planning board\" to version 3.");
    assert_eq!((after["latest_version"].as_i64(), after["versions"][1]["note"].as_str()), (Some(3), Some("Added the second column")));
    assert_eq!((empty_failed, empty_text.as_str()), (true, "Give content or path."));
}

#[tokio::test]
async fn an_agent_creates_a_file_artifact_from_a_path_on_its_node() {
    let server = Server::start().await;
    let (mut link, conversation) = started(&server).await;
    let calling = {
        let token = link.token.clone();
        let anonymous = server.anonymous();

        tokio::spawn(async move {
            let request = anonymous
                .post("/api/v1/mcp")
                .header("authorization", format!("Bearer {token}"))
                .header("x-mycelium-conversation", conversation.to_string())
                .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "create_artifact", "arguments": {"kind": "file", "title": "Report", "path": "out/report.pdf"}}}));

            anonymous.json(request).await
        })
    };

    let (request_id, cwd, command) = match link.next().await {
        ControlToNode::Git { request_id, cwd, command } => (request_id, cwd, command),
        other => panic!("expected git, got {other:?}"),
    };
    let data = base64::engine::general_purpose::STANDARD.encode(PDF);
    link.send(NodeToControl::GitResult { request_id, ok: true, output: json!({"data": data, "size": PDF.len()}), error: None }).await;

    let (_, answer) = calling.await.unwrap();
    let text = answer["result"]["content"][0]["text"].as_str().unwrap();
    let (headers, bytes) = content_of(&server.user, &artifact_id(text), 1).await;

    assert_eq!((cwd.as_str(), command), ("/work/app", GitCommand::ReadFile { path: "out/report.pdf".into() }));
    assert!(text.starts_with("Created file \"Report\""), "{text}");
    assert_eq!((headers["content-type"].to_str().unwrap(), bytes), ("application/pdf", PDF.to_vec()));
}

#[tokio::test]
async fn design_set_artboard_adds_an_artboard_then_replaces_it_and_leaves_everything_else() {
    let server = Server::start().await;
    let (link, conversation) = started(&server).await;
    let drawn = r##"{"pages":[{"id":"p1","name":"Page 1","items":[{"id":"n1","type":"note","x":5,"y":6,"w":240,"h":120,"text":"Keep me"}]}]}"##;
    let design = created(&server.user, "design", "Landing", drawn).await;
    grant(&server.user, &design, conversation).await;
    let id = design["id"].as_str().unwrap();

    let (_, first) = tool(&server, &link, conversation, "design_set_artboard", json!({"artifact": id, "name": "Home", "html": "<h1>Home</h1>"})).await;
    let (_, second) = tool(&server, &link, conversation, "design_set_artboard", json!({"artifact": id, "name": "Pricing", "w": 800, "h": 600, "html": "<h1>Pricing</h1>"})).await;
    let added = design_of(&server.user, id).await;
    let (failed, replaced_text) = tool(&server, &link, conversation, "design_set_artboard", json!({"artifact": id, "artboard": "a1", "html": "<h1>Home v2</h1>", "x": 10})).await;
    let replaced = design_of(&server.user, id).await;
    let (missing_failed, missing_text) = tool(&server, &link, conversation, "design_set_artboard", json!({"artifact": id, "artboard": "zzz", "html": "<p></p>"})).await;
    let (_, artifact) = server.user.json(server.user.get(&format!("/api/v1/artifacts/{id}"))).await;

    assert_eq!(first, "Saved artboard \"Home\" (a1) in \"Landing\" as version 2.");
    assert_eq!(second, "Saved artboard \"Pricing\" (a2) in \"Landing\" as version 3.");
    assert_eq!(added["pages"][0]["items"][0], json!({"id": "n1", "type": "note", "x": 5, "y": 6, "w": 240, "h": 120, "text": "Keep me"}));
    assert_eq!(added["pages"][0]["items"][1], json!({"id": "a1", "type": "artboard", "name": "Home", "x": 0, "y": 0, "w": 1440, "h": 900, "html": "<h1>Home</h1>"}));
    assert_eq!(added["pages"][0]["items"][2], json!({"id": "a2", "type": "artboard", "name": "Pricing", "x": 1520, "y": 0, "w": 800, "h": 600, "html": "<h1>Pricing</h1>"}));
    assert_eq!((failed, replaced_text.as_str()), (false, "Saved artboard \"Home\" (a1) in \"Landing\" as version 4."));
    assert_eq!(replaced["pages"][0]["items"][1], json!({"id": "a1", "type": "artboard", "name": "Home", "x": 10, "y": 0, "w": 1440, "h": 900, "html": "<h1>Home v2</h1>"}));
    assert_eq!(replaced["pages"][0]["items"][2], added["pages"][0]["items"][2]);
    assert_eq!((missing_failed, missing_text.as_str()), (true, "The page has no artboard zzz."));
    assert_eq!((artifact["latest_version"].as_i64(), artifact["versions"][0]["author"].as_str()), (Some(4), Some("agent")));
}

#[tokio::test]
async fn design_tools_refuse_other_kinds_and_remove_an_item_by_id() {
    let server = Server::start().await;
    let (link, conversation) = started(&server).await;
    let page = created(&server.user, "page", "Board", "x").await;
    let design = created(&server.user, "design", "Landing", r##"{"pages":[{"id":"p1","items":[{"id":"s1","type":"rect","x":0,"y":0,"w":10,"h":10,"fill":"#fff","stroke":null,"radius":0},{"id":"t1","type":"text","x":0,"y":0,"w":10,"text":"Hi","size":16,"color":"#000"}]}]}"##).await;
    grant(&server.user, &design, conversation).await;
    let id = design["id"].as_str().unwrap();

    grant(&server.user, &page, conversation).await;
    let (page_failed, page_text) = tool(&server, &link, conversation, "design_set_artboard", json!({"artifact": page["id"], "html": "<p></p>"})).await;
    let (no_html_failed, _) = tool(&server, &link, conversation, "design_set_artboard", json!({"artifact": id})).await;
    let (removed_failed, removed_text) = tool(&server, &link, conversation, "design_remove_item", json!({"artifact": id, "item": "s1"})).await;
    let (unknown_failed, unknown_text) = tool(&server, &link, conversation, "design_remove_item", json!({"artifact": id, "item": "s1"})).await;
    let remaining = design_of(&server.user, id).await;

    assert_eq!((page_failed, page_text.as_str()), (true, "That artefact is not a design."));
    assert!(no_html_failed);
    assert_eq!((removed_failed, removed_text.as_str()), (false, "Saved the removal of item s1 in \"Landing\" as version 2."));
    assert_eq!((unknown_failed, unknown_text.as_str()), (true, "The design has no item s1."));
    assert_eq!(remaining["pages"][0]["items"].as_array().unwrap().iter().map(|item| item["id"].as_str().unwrap()).collect::<Vec<_>>(), ["t1"]);
}

#[tokio::test]
async fn read_artifact_gives_text_a_file_summary_or_the_outline_of_a_design() {
    let server = Server::start().await;
    let (link, conversation) = started(&server).await;
    let design = created(&server.user, "design", "Landing", r#"{"pages":[{"id":"p1","name":"Page 1","items":[{"id":"a1","type":"artboard","name":"Home","x":0,"y":0,"w":1440,"h":900,"html":"<h1>Home</h1>"},{"id":"n1","type":"note","x":0,"y":0,"w":240,"h":120,"text":"Ask about pricing"}]}]}"#).await;
    let page = created(&server.user, "page", "Board", "<h1>Board</h1>").await;
    let (_, file) = create(&server.user, json!({"kind": "file", "title": "Report", "content_base64": base64::engine::general_purpose::STANDARD.encode(PDF), "media_type": "application/pdf"})).await;

    grant(&server.user, &design, conversation).await;
    grant(&server.user, &page, conversation).await;
    grant(&server.user, &file, conversation).await;
    let (_, outline) = tool(&server, &link, conversation, "read_artifact", json!({"artifact": design["id"]})).await;
    let (_, html) = tool(&server, &link, conversation, "read_artifact", json!({"artifact": design["id"], "artboard": "home"})).await;
    let (_, text) = tool(&server, &link, conversation, "read_artifact", json!({"artifact": page["id"]})).await;
    let (_, summary) = tool(&server, &link, conversation, "read_artifact", json!({"artifact": file["id"]})).await;
    let (stranger_failed, stranger_text) = tool(&server, &link, conversation, "read_artifact", json!({"artifact": Uuid::new_v4().to_string()})).await;

    assert!(outline.contains("Design \"Landing\""), "{outline}");
    assert!(outline.contains("artboard a1 \"Home\" 1440x900"), "{outline}");
    assert!(outline.contains("note n1 \"Ask about pricing\" 240x120"), "{outline}");
    assert!(!outline.contains("<h1>"), "{outline}");
    assert_eq!((html.as_str(), text.as_str()), ("<h1>Home</h1>", "<h1>Board</h1>"));
    assert!(summary.starts_with("File \"Report\" (application/pdf, 8 bytes, version 1)."), "{summary}");
    assert_eq!((stranger_failed, stranger_text.as_str()), (true, "That artefact was not found. Check the address and try again."));
}

#[tokio::test]
async fn read_artifact_feedback_gives_unresolved_comments_and_the_data_store() {
    let server = Server::start().await;
    let (link, conversation) = started(&server).await;
    let page = created(&server.user, "page", "Board", "x").await;
    let id = page["id"].as_str().unwrap();

    grant(&server.user, &page, conversation).await;
    let (_, quiet) = tool(&server, &link, conversation, "read_artifact_feedback", json!({"artifact": id})).await;
    let (_, open) = server.user.json(server.user.post(&format!("/api/v1/artifacts/{id}/comments")).json(&json!({"body": "Row 3 is wrong", "anchor": {"row": 3}}))).await;
    let (_, done) = server.user.json(server.user.post(&format!("/api/v1/artifacts/{id}/comments")).json(&json!({"body": "Typo"}))).await;
    server.user.json(server.user.patch(&format!("/api/v1/artifacts/{id}/comments/{}", done["id"].as_str().unwrap())).json(&json!({"resolved": true}))).await;
    server.user.json(server.user.put(&format!("/api/v1/artifacts/{id}/data/row-3")).json(&json!({"call": "yours"}))).await;
    let (failed, feedback) = tool(&server, &link, conversation, "read_artifact_feedback", json!({"artifact": id})).await;

    assert_eq!(quiet, "No unresolved comments.\nThe data store is empty.");
    assert!(!failed);
    assert!(feedback.contains(&format!("- {} by", open["id"].as_str().unwrap())), "{feedback}");
    assert!(feedback.contains("Row 3 is wrong (anchor {\"row\":3})"), "{feedback}");
    assert!(!feedback.contains("Typo"), "{feedback}");
    assert!(feedback.contains("- row-3 = {\"call\":\"yours\"}"), "{feedback}");
}

#[tokio::test]
async fn an_agent_cannot_reach_another_persons_artifact() {
    let server = Server::start().await;
    let (link, conversation) = started(&server).await;
    let other = server.second_user("other@example.com").await;
    let theirs = created(&other, "page", "Theirs", "secret").await;

    let (read_failed, _) = tool(&server, &link, conversation, "read_artifact", json!({"artifact": theirs["id"]})).await;
    let (by_slug_failed, _) = tool(&server, &link, conversation, "read_artifact", json!({"artifact": theirs["slug"]})).await;
    let (update_failed, _) = tool(&server, &link, conversation, "update_artifact", json!({"artifact": theirs["id"], "content": "mine now"})).await;

    assert_eq!((read_failed, by_slug_failed, update_failed), (true, true, true));
}

#[tokio::test]
async fn artefact_access_is_scoped_approved_upgraded_and_revoked() {
    let server = Server::start().await;
    let (link, conversation) = started(&server).await;
    let artefact = created(&server.user, "markdown", "Private roadmap", "# Plan").await;
    let id = artefact["id"].as_str().unwrap();
    let (denied, _) = tool(&server, &link, conversation, "read_artifact", json!({"artifact": id})).await;
    assert!(denied);
    let (_, found) = tool(&server, &link, conversation, "find_artifacts", json!({"query": "roadmap"})).await;
    assert_eq!(serde_json::from_str::<Value>(&found).unwrap()["artifacts"], json!([]));
    let (failed, _) = tool(&server, &link, conversation, "request_artifact_access", json!({"artifact": id, "permission": "view", "reason": "Read the roadmap"})).await;
    assert!(!failed);
    let _ = tool(&server, &link, conversation, "request_artifact_access", json!({"artifact": id, "permission": "view", "reason": "Read the roadmap again"})).await;
    let (_, requests) = server.user.json(server.user.get("/api/v1/artifact-access-requests")).await;
    assert_eq!(requests.as_array().unwrap().len(), 1);
    let request = requests[0]["id"].as_str().unwrap();
    let (status, approved) = server.user.json(server.user.post(&format!("/api/v1/artifact-access-requests/{request}/approve")).json(&json!({}))).await;
    assert_eq!(status, 200, "{approved}");
    assert_eq!(approved["state"], "approved");
    let (denied, text) = tool(&server, &link, conversation, "read_artifact", json!({"artifact": id})).await;
    assert!(!denied, "{text}");
    assert_eq!(text, "# Plan");
    let (denied, _) = tool(&server, &link, conversation, "update_artifact", json!({"artifact": id, "content": "Changed"})).await;
    assert!(denied);
    let (status, linked) = server.user.json(server.user.post(&format!("/api/v1/artifacts/{id}/links")).json(&json!({"conversation_id": conversation, "permission": "edit"}))).await;
    assert_eq!(status, 200, "{linked}");
    let (denied, text) = tool(&server, &link, conversation, "update_artifact", json!({"artifact": id, "content": "Changed"})).await;
    assert!(!denied, "{text}");
    let (_, found) = tool(&server, &link, conversation, "find_artifacts", json!({"query": "ROADMAP"})).await;
    let found: Value = serde_json::from_str(&found).unwrap();
    assert_eq!(found["artifacts"][0]["permission"], "edit");
    let link_id = linked["id"].as_str().unwrap();
    let response = server.user.delete(&format!("/api/v1/artifacts/{id}/links/{link_id}")).send().await.unwrap();
    assert_eq!(response.status(), 204);
    let (denied, _) = tool(&server, &link, conversation, "read_artifact", json!({"artifact": id})).await;
    assert!(denied);
    let (status, _) = server.user.json(server.user.get(&format!("/api/v1/artifacts/{id}"))).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn artefact_creator_chat_keeps_access_and_strangers_cannot_grant_it() {
    let server = Server::start().await;
    let (link, conversation) = started(&server).await;
    let (_, created) = tool(&server, &link, conversation, "create_artifact", json!({"kind": "markdown", "title": "Chat notes", "content": "Notes"})).await;
    let id = artifact_id(&created);
    let (_, found) = tool(&server, &link, conversation, "find_artifacts", json!({})).await;
    assert!(found.contains(&id));
    let other = server.second_user("stranger@example.test").await;
    let (status, _) = other.json(other.post(&format!("/api/v1/artifacts/{id}/links")).json(&json!({"conversation_id": conversation, "permission": "edit"}))).await;
    assert_eq!(status, 404);
    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/artifacts/{id}/links")).json(&json!({"conversation_id": Uuid::new_v4(), "permission": "edit"}))).await;
    assert_eq!(status, 404);
    let (status, _) = server.user.json(server.user.post(&format!("/api/v1/artifacts/{id}/links")).json(&json!({"conversation_id": conversation, "permission": "owner"}))).await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn artefacts_belong_to_the_creator_project_and_can_be_shared_with_more_projects() {
    let server = Server::start().await;
    let (mut link, creator) = started(&server).await;
    let second = server.create_conversation(link.id, Some("second")).await;
    link.expect_start_session().await;
    let third = server.create_conversation(link.id, Some("third")).await;
    link.expect_start_session().await;
    let first_project: Uuid = sqlx::query_scalar("insert into projects (owner, node_id, name, path) select owner, node_id, 'First', '/first' from conversations where id = $1 returning id").bind(creator).fetch_one(&server.pool).await.unwrap();
    let second_project: Uuid = sqlx::query_scalar("insert into projects (owner, node_id, name, path) select owner, node_id, 'Second', '/second' from conversations where id = $1 returning id").bind(creator).fetch_one(&server.pool).await.unwrap();
    sqlx::query("update conversations set project_id = $1 where id = any($2)").bind(first_project).bind(vec![creator, second.id]).execute(&server.pool).await.unwrap();
    sqlx::query("update conversations set project_id = $1 where id = $2").bind(second_project).bind(third.id).execute(&server.pool).await.unwrap();
    let (_, text) = tool(&server, &link, creator, "create_artifact", json!({"kind": "markdown", "title": "Project plan", "content": "Shared"})).await;
    let id = artifact_id(&text);
    let (denied, text) = tool(&server, &link, second.id, "read_artifact", json!({"artifact": id})).await;
    assert!(!denied, "{text}");
    let (denied, _) = tool(&server, &link, third.id, "read_artifact", json!({"artifact": id})).await;
    assert!(denied);
    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/artifacts/{id}/links")).json(&json!({"project_id": second_project, "permission": "view"}))).await;
    assert_eq!(status, 200, "{body}");
    let (denied, text) = tool(&server, &link, third.id, "read_artifact", json!({"artifact": id})).await;
    assert!(!denied, "{text}");
    let (denied, _) = tool(&server, &link, third.id, "update_artifact", json!({"artifact": id, "content": "Changed"})).await;
    assert!(denied);
    let (_, _) = tool(&server, &link, third.id, "request_artifact_access", json!({"artifact": id, "permission": "edit", "reason": "Edit the project plan"})).await;
    let (_, requests) = server.user.json(server.user.get("/api/v1/artifact-access-requests")).await;
    let request = requests[0]["id"].as_str().unwrap();
    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/artifact-access-requests/{request}/approve")).json(&json!({"project": true}))).await;
    assert_eq!(status, 200, "{body}");
    let (denied, text) = tool(&server, &link, third.id, "update_artifact", json!({"artifact": id, "content": "Changed"})).await;
    assert!(!denied, "{text}");
    let (_, artifact) = server.user.json(server.user.get(&format!("/api/v1/artifacts/{id}"))).await;
    assert_eq!(artifact["project_ids"].as_array().unwrap().len(), 2);
    assert!(artifact["conversation_ids"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn an_artefact_lists_with_the_project_and_conversation_it_came_from() {
    let server = Server::start().await;
    let (link, conversation) = started(&server).await;
    let project: Uuid = sqlx::query_scalar("insert into projects (owner, node_id, name, path, icon) select owner, node_id, 'Okapi', '/okapi', 'rocket' from conversations where id = $1 returning id").bind(conversation).fetch_one(&server.pool).await.unwrap();
    sqlx::query("update conversations set project_id = $1, title = 'Plan the rollout' where id = $2").bind(project).bind(conversation).execute(&server.pool).await.unwrap();
    tool(&server, &link, conversation, "create_artifact", json!({"kind": "markdown", "title": "Rollout notes", "content": "Notes"})).await;
    let (_, listed) = server.user.json(server.user.get("/api/v1/artifacts")).await;
    let origin = &listed[0]["origin"];

    assert_eq!(
        (origin["project_name"].as_str(), origin["project_icon"].as_str(), origin["conversation_title"].as_str(), origin["project_id"].as_str(), origin["conversation_id"].as_str()),
        (Some("Okapi"), Some("rocket"), Some("Plan the rollout"), Some(project.to_string().as_str()), Some(conversation.to_string().as_str()))
    );

    let (_, in_project) = server.user.json(server.user.get(&format!("/api/v1/artifacts?project={project}"))).await;
    let (_, elsewhere) = server.user.json(server.user.get(&format!("/api/v1/artifacts?project={}", Uuid::new_v4()))).await;

    assert_eq!((in_project.as_array().unwrap().len(), elsewhere.as_array().unwrap().len()), (1, 0));
}

#[tokio::test]
async fn an_artefact_whose_conversation_was_deleted_has_no_origin() {
    let server = Server::start().await;
    let (link, conversation) = started(&server).await;
    let (_, text) = tool(&server, &link, conversation, "create_artifact", json!({"kind": "markdown", "title": "Orphan", "content": "Notes"})).await;
    let id = artifact_id(&text);
    sqlx::query("delete from conversations where id = $1").bind(conversation).execute(&server.pool).await.unwrap();
    let (status, artifact) = server.user.json(server.user.get(&format!("/api/v1/artifacts/{id}"))).await;

    assert_eq!(status, 200, "{artifact}");
    assert!(artifact.get("origin").is_none());
}
