use crate::common::{NodeLink, Server, eventually, hash};
use mycelium_proto::api::{ApiAccessRequest, ApiAccessState};
use mycelium_proto::link::{ControlToNode, NodeToControl};
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};
use uuid::Uuid;

const READ: &[&str] = &["conversations:read"];
const WRITE: &[&str] = &["conversations:write"];
const NODES: &[&str] = &["nodes:read"];
const SETTINGS: &[&str] = &["settings:read"];
const ALL: &[&str] = &["conversations:read", "nodes:read", "settings:read"];

async fn insert_token(server: &Server, scopes: Option<&[&str]>, expires_in: Option<&str>) -> (Uuid, String) {
    let secret = Uuid::new_v4().to_string();
    let scopes: Option<Vec<String>> = scopes.map(|scopes| scopes.iter().map(|scope| scope.to_string()).collect());
    let id = sqlx::query_scalar::<_, Uuid>("insert into client_tokens (user_id, label, token_hash, scopes, expires_at) values ((select id from users limit 1), 'Test', $1, $2, now() + $3::interval) returning id")
        .bind(hash(&secret))
        .bind(scopes)
        .bind(expires_in)
        .fetch_one(&server.pool)
        .await
        .unwrap();

    (id, format!("myc_client_{secret}"))
}

async fn status_with(server: &Server, token: &str, method: &str, path: &str) -> u16 {
    let anonymous = server.anonymous();
    let method = reqwest::Method::from_bytes(method.as_bytes()).unwrap();
    let response = anonymous.http.request(method, format!("{}{path}", server.base)).header("authorization", format!("Bearer {token}")).send().await.unwrap();

    response.status().as_u16()
}

#[tokio::test]
async fn an_embedded_view_signs_in_with_the_client_token_as_a_cookie() {
    let server = Server::start().await;
    let (_, token) = insert_token(&server, None, None).await;
    let anonymous = server.anonymous();

    let with_cookie = anonymous.http.get(format!("{}/api/v1/me", server.base)).header("cookie", format!("mycelium_client={token}")).send().await.unwrap();
    let with_a_wrong_cookie = anonymous.http.get(format!("{}/api/v1/me", server.base)).header("cookie", "mycelium_client=myc_client_nope").send().await.unwrap();

    assert_eq!((with_cookie.status().as_u16(), with_a_wrong_cookie.status().as_u16()), (200, 401));
}

#[tokio::test]
async fn a_scoped_token_sent_as_a_cookie_reaches_only_what_its_scopes_cover() {
    let server = Server::start().await;
    let (_, token) = insert_token(&server, Some(READ), None).await;
    let any = Uuid::new_v4();
    let anonymous = server.anonymous();
    let cookie = format!("mycelium_client={token}");
    let mut statuses = Vec::new();

    for path in [format!("/api/v1/api-access-requests/{any}/approve"), format!("/api/v1/nodes/{any}/ssh-tokens"), "/api/v1/device/ABCD-EFGH/approve".to_string()] {
        let response = anonymous.http.post(format!("{}{path}", server.base)).header("cookie", &cookie).json(&json!({})).send().await.unwrap();
        statuses.push(response.status().as_u16());
    }

    let reading = anonymous.http.get(format!("{}/api/v1/conversations", server.base)).header("cookie", &cookie).send().await.unwrap();

    assert_eq!((statuses, reading.status().as_u16()), (vec![403, 403, 403], 200));
}

#[tokio::test]
async fn a_planted_client_cookie_does_not_override_the_session_cookie() {
    let server = Server::start().await;
    let (_, token) = insert_token(&server, Some(READ), None).await;
    let cookie = format!("mycelium_client={token}; {}", server.user.cookie);

    let response = server.anonymous().http.post(format!("{}/api/v1/api-access-requests/{}/approve", server.base, Uuid::new_v4())).header("cookie", cookie).json(&json!({})).send().await.unwrap();

    assert_eq!(response.status().as_u16(), 404);
}

async fn tool(server: &Server, link: &NodeLink, conversation: Uuid, name: &str, arguments: Value) -> Value {
    let anonymous = server.anonymous();
    let request = anonymous
        .post("/api/v1/mcp")
        .header("authorization", format!("Bearer {}", link.token))
        .header("x-mycelium-conversation", conversation.to_string())
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}}));
    let (status, body) = anonymous.json(request).await;
    assert_eq!(status, 200, "{body}");

    body["result"].clone()
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_string()
}

async fn working_conversation(server: &Server, link: &mut NodeLink) -> Uuid {
    let conversation = server.create_conversation(link.id, Some("Debug the sync")).await;
    link.expect_start_session().await;
    link.start_session(conversation.id, &format!("acp-{}", conversation.id)).await;
    link.expect_prompt().await;

    conversation.id
}

async fn requests(server: &Server) -> Vec<Value> {
    let (_, requests) = server.user.json(server.user.get("/api/v1/api-access-requests")).await;

    requests.as_array().unwrap().clone()
}

async fn queued_texts(server: &Server, conversation: Uuid) -> Vec<String> {
    let (_, queue) = server.user.json(server.user.get(&format!("/api/v1/conversations/{conversation}/queue"))).await;

    queue.as_array().unwrap().iter().map(|item| item["text"].as_str().unwrap().to_string()).collect()
}

async fn ask(server: &Server, link: &NodeLink, conversation: Uuid) -> Value {
    tool(server, link, conversation, "request_api_access", json!({"scopes": ["conversations:read", "nodes:read"], "reason": "Reading the events to find the stuck turn"})).await
}

async fn refusal_with(server: &Server, token: &str, method: &str, path: &str) -> (u16, Value) {
    let anonymous = server.anonymous();
    let method = reqwest::Method::from_bytes(method.as_bytes()).unwrap();
    let response = anonymous.http.request(method, format!("{}{path}", server.base)).header("authorization", format!("Bearer {token}")).send().await.unwrap();
    let status = response.status().as_u16();
    let body = response.json::<Value>().await.unwrap_or(Value::Null);

    (status, body)
}

async fn approve(server: &Server, id: &str, body: Value) -> (u16, Value) {
    server.user.json(server.user.post(&format!("/api/v1/api-access-requests/{id}/approve")).json(&body)).await
}

#[tokio::test]
async fn a_token_reaches_only_the_routes_its_scopes_cover() {
    let server = Server::start().await;
    let any = Uuid::new_v4();
    let table: Vec<(Option<&[&str]>, &str, String, bool)> = vec![
        (Some(READ), "GET", "/api/v1/conversations".into(), true),
        (Some(READ), "GET", format!("/api/v1/conversations/{any}/events"), true),
        (Some(READ), "GET", "/api/v1/me".into(), true),
        (Some(READ), "POST", "/api/v1/conversations".into(), false),
        (Some(READ), "POST", format!("/api/v1/conversations/{any}/prompt"), false),
        (Some(READ), "GET", "/api/v1/nodes".into(), false),
        (Some(READ), "GET", "/api/v1/memories".into(), false),
        (Some(READ), "GET", "/api/v1/sessions".into(), false),
        (Some(READ), "GET", "/api/v1/stream".into(), false),
        (Some(READ), "DELETE", format!("/api/v1/sessions/{any}"), false),
        (Some(READ), "POST", "/api/v1/device/ABCD-EFGH/approve".into(), false),
        (Some(WRITE), "POST", format!("/api/v1/conversations/{any}/prompt"), true),
        (Some(WRITE), "PATCH", format!("/api/v1/conversations/{any}"), true),
        (Some(WRITE), "GET", "/api/v1/conversations".into(), false),
        (Some(WRITE), "POST", format!("/api/v1/conversations/{any}/permission"), false),
        (Some(WRITE), "POST", format!("/api/v1/conversations/{any}/terminal"), false),
        (Some(WRITE), "POST", format!("/api/v1/conversations/{any}/config"), false),
        (Some(WRITE), "POST", format!("/api/v1/conversations/{any}/responses/{any}"), false),
        (Some(WRITE), "POST", format!("/api/v1/conversations/{any}/environment"), false),
        (Some(WRITE), "POST", format!("/api/v1/conversations/{any}/move"), false),
        (Some(WRITE), "POST", format!("/api/v1/conversations/{any}/git/push"), false),
        (Some(WRITE), "POST", "/api/v1/conversations".into(), false),
        (Some(NODES), "GET", "/api/v1/nodes".into(), true),
        (Some(NODES), "POST", format!("/api/v1/nodes/{any}/drain"), false),
        (Some(NODES), "GET", "/api/v1/conversations".into(), false),
        (Some(SETTINGS), "GET", "/api/v1/instructions".into(), true),
        (Some(SETTINGS), "PUT", "/api/v1/instructions/global".into(), false),
        (Some(SETTINGS), "GET", "/api/v1/api-access-requests".into(), false),
        (Some(SETTINGS), "POST", format!("/api/v1/api-access-requests/{any}/approve"), false),
        (Some(SETTINGS), "GET", "/api/v1/unlisted".into(), false),
        (Some(ALL), "GET", "/api/v1/nodes".into(), true),
        (None, "GET", "/api/v1/sessions".into(), true),
        (None, "GET", "/api/v1/api-access-requests".into(), true),
    ];

    for (scopes, method, path, allowed) in table {
        let (_, token) = insert_token(&server, scopes, None).await;
        let status = status_with(&server, &token, method, &path).await;

        if allowed {
            assert!(status != 403 && status != 401, "{scopes:?} should reach {method} {path}, got {status}");
        } else {
            assert_eq!(status, 403, "{scopes:?} should not reach {method} {path}");
        }
    }
}

#[tokio::test]
async fn a_token_granted_for_one_conversation_is_refused_on_another() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let own = working_conversation(&server, &mut link).await;
    let other = Uuid::new_v4();
    let (token_id, token) = insert_token(&server, Some(READ), None).await;
    sqlx::query("update client_tokens set conversation_id = $2 where id = $1").bind(token_id).bind(own).execute(&server.pool).await.unwrap();

    let on_its_own = status_with(&server, &token, "GET", &format!("/api/v1/conversations/{own}/events")).await;
    let on_another = status_with(&server, &token, "GET", &format!("/api/v1/conversations/{other}/events")).await;

    assert_eq!((on_its_own, on_another), (200, 403));
}

async fn grouped(server: &Server, link: &mut NodeLink, name: &str, folder: &str) -> Uuid {
    let conversation = working_conversation(server, link).await;
    sqlx::query("update conversations set cwd = $2 where id = $1").bind(conversation).bind(folder).execute(&server.pool).await.unwrap();
    let (status, created) = server.user.json(server.user.post("/api/v1/groups").json(&json!({"name": name}))).await;
    assert_eq!(status, 201, "{created}");
    let group = created["id"].as_str().unwrap();
    let (status, claimed) = server.user.json(server.user.post(&format!("/api/v1/groups/{group}/folders")).json(&json!({"prefix": folder}))).await;
    assert_eq!(status, 201, "{claimed}");

    conversation
}

#[tokio::test]
async fn a_token_granted_for_one_conversation_reads_and_writes_others_in_its_group_and_nothing_outside_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let own = grouped(&server, &mut link, "Mycelium", "/srv/atlas").await;
    let sibling = working_conversation(&server, &mut link).await;
    sqlx::query("update conversations set cwd = '/srv/atlas/docs' where id = $1").bind(sibling).execute(&server.pool).await.unwrap();
    let outside = grouped(&server, &mut link, "Orion", "/srv/orion").await;
    let (token_id, token) = insert_token(&server, Some(&["conversations:read", "conversations:write"]), None).await;
    sqlx::query("update client_tokens set conversation_id = $2 where id = $1").bind(token_id).bind(own).execute(&server.pool).await.unwrap();

    let mut statuses = Vec::new();

    for conversation in [sibling, outside] {
        statuses.push(status_with(&server, &token, "GET", &format!("/api/v1/conversations/{conversation}")).await);
        statuses.push(status_with(&server, &token, "GET", &format!("/api/v1/conversations/{conversation}/events")).await);
        statuses.push(u16::from(status_with(&server, &token, "POST", &format!("/api/v1/conversations/{conversation}/prompt")).await == 403));
    }

    assert_eq!(statuses, vec![200, 200, 0, 403, 403, 1]);
}

#[tokio::test]
async fn a_refusal_names_whether_the_route_needs_full_access_or_a_scope_the_token_lacks() {
    let server = Server::start().await;
    let any = Uuid::new_v4();
    let table: Vec<(&[&str], &str, String, &str, &str)> = vec![
        (WRITE, "POST", "/api/v1/conversations".into(), "full_access_required", "Starting a conversation needs full access, which this token does not include. Use the start_conversation tool, which asks the person."),
        (WRITE, "POST", format!("/api/v1/conversations/{any}/move"), "full_access_required", "Moving a conversation needs full access, which this token does not include."),
        (WRITE, "POST", format!("/api/v1/conversations/{any}/git/push"), "full_access_required", "Pushing from a conversation needs full access, which this token does not include."),
        (WRITE, "POST", format!("/api/v1/conversations/{any}/environment"), "full_access_required", "Driving a conversation's environment needs full access, which this token does not include."),
        (READ, "GET", "/api/v1/sessions".into(), "full_access_required", "That needs full access, which this token does not include."),
        (READ, "POST", format!("/api/v1/conversations/{any}/prompt"), "scope_not_granted", "This token does not include conversations:write. Ask the person for access that includes it."),
        (WRITE, "GET", "/api/v1/conversations".into(), "scope_not_granted", "This token does not include conversations:read. Ask the person for access that includes it."),
        (READ, "GET", "/api/v1/nodes".into(), "scope_not_granted", "This token does not include nodes:read. Ask the person for access that includes it."),
    ];

    for (scopes, method, path, code, message) in table {
        let (_, token) = insert_token(&server, Some(scopes), None).await;
        let (status, body) = refusal_with(&server, &token, method, &path).await;

        assert_eq!((status, body["error"].as_str(), body["message"].as_str()), (403, Some(code), Some(message)), "{scopes:?} {method} {path}");
    }
}

#[tokio::test]
async fn creating_a_conversation_with_a_write_token_is_refused_and_creates_nothing() {
    let server = Server::start().await;
    let (_, token) = insert_token(&server, Some(WRITE), None).await;
    let before = sqlx::query_scalar::<_, i64>("select count(*) from conversations").fetch_one(&server.pool).await.unwrap();

    let (status, body) = refusal_with(&server, &token, "POST", "/api/v1/conversations").await;
    let after = sqlx::query_scalar::<_, i64>("select count(*) from conversations").fetch_one(&server.pool).await.unwrap();

    assert_eq!((status, body["error"].as_str(), after), (403, Some("full_access_required"), before));
}

#[tokio::test]
async fn a_conversation_outside_the_connected_groups_is_refused_naming_both_groups() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let own = grouped(&server, &mut link, "Mycelium", "/srv/atlas").await;
    let outside = grouped(&server, &mut link, "Orion", "/srv/orion").await;
    let ungrouped = working_conversation(&server, &mut link).await;
    let (token_id, token) = insert_token(&server, Some(READ), None).await;
    sqlx::query("update client_tokens set conversation_id = $2 where id = $1").bind(token_id).bind(own).execute(&server.pool).await.unwrap();

    let (outside_status, outside_body) = refusal_with(&server, &token, "GET", &format!("/api/v1/conversations/{outside}/events")).await;
    let (ungrouped_status, ungrouped_body) = refusal_with(&server, &token, "GET", &format!("/api/v1/conversations/{ungrouped}/events")).await;

    assert_eq!(outside_status, 403);
    assert_eq!(outside_body["error"], "group_not_connected");
    assert_eq!(outside_body["message"], "This token can reach conversations in Mycelium and groups connected to it. Orion is not connected.");
    assert_eq!(ungrouped_status, 403);
    assert_eq!(ungrouped_body["message"], "This token can reach conversations in Mycelium and groups connected to it. no group is not connected.");
}

#[tokio::test]
async fn an_expired_token_is_refused_and_a_running_one_is_not() {
    let server = Server::start().await;
    let (_, expired) = insert_token(&server, Some(READ), Some("-1 minute")).await;
    let (_, running) = insert_token(&server, Some(READ), Some("1 hour")).await;
    let (_, expired_full) = insert_token(&server, None, Some("-1 minute")).await;

    assert_eq!(status_with(&server, &expired, "GET", "/api/v1/me").await, 401);
    assert_eq!(status_with(&server, &expired, "GET", "/api/v1/conversations").await, 401);
    assert_eq!(status_with(&server, &expired_full, "GET", "/api/v1/me").await, 401);
    assert_eq!(status_with(&server, &running, "GET", "/api/v1/me").await, 200);
}

#[tokio::test]
async fn every_call_a_scoped_token_makes_is_logged_and_calls_older_than_thirty_days_are_dropped() {
    let server = Server::start().await;
    let (id, token) = insert_token(&server, Some(READ), Some("1 hour")).await;
    let (_, full) = insert_token(&server, None, None).await;
    sqlx::query("insert into api_token_calls (token_id, method, route, status, called_at) values ($1, 'GET', '/api/v1/me', 200, now() - interval '31 days')").bind(id).execute(&server.pool).await.unwrap();

    status_with(&server, &token, "GET", &format!("/api/v1/conversations/{}/events", Uuid::new_v4())).await;
    status_with(&server, &token, "GET", "/api/v1/nodes").await;
    status_with(&server, &full, "GET", "/api/v1/nodes").await;

    let calls = sqlx::query_as::<_, (String, String, i32)>("select method, route, status from api_token_calls where token_id = $1 order by id").bind(id).fetch_all(&server.pool).await.unwrap();
    assert_eq!(calls, vec![("GET".to_string(), "/api/v1/conversations/{id}/events".to_string(), 404), ("GET".to_string(), "/api/v1/nodes".to_string(), 403)]);
    assert_eq!(sqlx::query_scalar::<_, i64>("select count(*) from api_token_calls").fetch_one(&server.pool).await.unwrap(), 2);
}

#[tokio::test]
async fn requesting_access_waits_for_the_person_and_hands_out_nothing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;
    let mut stream = server.user.stream().await;

    let result = ask(&server, &link, conversation).await;

    let listed = requests(&server).await;
    assert_eq!(listed.len(), 1);
    assert_eq!((result["isError"].clone(), text(&result)), (json!(false), format!("Waiting for the person to approve API access (request {}).", listed[0]["id"].as_str().unwrap())));
    assert_eq!(
        (listed[0]["state"].clone(), listed[0]["conversation_id"].clone(), listed[0]["scopes"].clone(), listed[0]["hours"].clone(), listed[0]["reason"].clone()),
        (json!("requested"), json!(conversation), json!(["conversations:read", "nodes:read"]), json!(4), json!("Reading the events to find the stuck turn")),
    );
    assert_eq!(sqlx::query_scalar::<_, i64>("select count(*) from client_tokens where scopes is not null").fetch_one(&server.pool).await.unwrap(), 0);

    let streamed: ApiAccessRequest = stream
        .until(|message| match message {
            StreamMessage::ApiAccessRequest(request) => Some(request),
            _ => None,
        })
        .await;
    assert_eq!(streamed.state, ApiAccessState::Requested);
}

#[tokio::test]
async fn a_request_with_unknown_scopes_no_scopes_a_bad_duration_or_no_reason_is_refused_and_stores_nothing() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;

    for arguments in [
        json!({"scopes": ["nodes:write"], "reason": "r"}),
        json!({"scopes": [], "reason": "r"}),
        json!({"scopes": ["nodes:read"], "hours": 0, "reason": "r"}),
        json!({"scopes": ["nodes:read"], "hours": 25, "reason": "r"}),
        json!({"scopes": ["nodes:read"], "reason": " "}),
    ] {
        let result = tool(&server, &link, conversation, "request_api_access", arguments.clone()).await;

        assert_eq!(result["isError"], true, "{arguments}: {result}");
        assert!(text(&result).starts_with("Nothing was requested."), "{arguments}: {result}");
    }

    assert_eq!(requests(&server).await.len(), 0);
}

#[tokio::test]
async fn approving_mints_a_narrowed_expiring_token_for_the_node_and_tells_the_agent_where_it_is_never_the_token() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;
    let result = ask(&server, &link, conversation).await;
    let id = requests(&server).await[0]["id"].as_str().unwrap().to_string();

    let (status, body) = approve(&server, &id, json!({"scopes": ["conversations:read"]})).await;

    assert_eq!((status, body["state"].clone(), body["scopes"].clone()), (200, json!("approved"), json!(["conversations:read"])), "{body}");
    let token = match link.next().await {
        ControlToNode::ApiToken { conversation_id, token } => {
            assert_eq!(conversation_id, conversation);
            token
        }
        other => panic!("expected api_token, got {other:?}"),
    };
    assert!(token.starts_with("myc_client_"));
    assert_eq!(status_with(&server, &token, "GET", "/api/v1/conversations").await, 200);
    assert_eq!(status_with(&server, &token, "GET", "/api/v1/nodes").await, 403);
    assert_eq!(status_with(&server, &token, "POST", &format!("/api/v1/conversations/{conversation}/prompt")).await, 403);

    let hours = sqlx::query_scalar::<_, f64>("select (extract(epoch from expires_at - now()) / 3600)::float8 from client_tokens where scopes is not null").fetch_one(&server.pool).await.unwrap();
    assert!((3.9..=4.0).contains(&hours), "{hours}");

    let path = format!("/state/sessions/{conversation}/api-token");
    link.send(NodeToControl::ApiTokenSaved { conversation_id: conversation, path: Some(path.clone()) }).await;
    let told = eventually("the agent being told", || async { queued_texts(&server, conversation).await.into_iter().find(|text| text.contains(&path)) }).await;
    assert!(told.contains(&format!("{}/api/v1/", server.base)), "{told}");

    let secret = token.trim_start_matches("myc_client_");
    let everywhere = [
        serde_json::to_string(&server.events(conversation).await).unwrap(),
        serde_json::to_string(&requests(&server).await).unwrap(),
        serde_json::to_string(&queued_texts(&server, conversation).await).unwrap(),
        text(&result),
        told,
    ];
    assert!(everywhere.iter().all(|place| !place.contains(secret)), "the token leaked");
}

#[tokio::test]
async fn declining_delivers_nothing_to_the_node_and_tells_the_agent() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;
    ask(&server, &link, conversation).await;
    let id = requests(&server).await[0]["id"].as_str().unwrap().to_string();

    let (status, body) = server.user.json(server.user.post(&format!("/api/v1/api-access-requests/{id}/decline"))).await;

    assert_eq!((status, body["state"].clone()), (200, json!("declined")), "{body}");
    assert_eq!(sqlx::query_scalar::<_, i64>("select count(*) from client_tokens where scopes is not null").fetch_one(&server.pool).await.unwrap(), 0);
    assert!(queued_texts(&server, conversation).await.iter().any(|text| text.contains("declined") && text.contains("API access")));

    let (status, body) = approve(&server, &id, json!({})).await;
    assert_eq!((status, body["error"].clone()), (409, json!("request_not_requested")));
    assert_eq!(sqlx::query_scalar::<_, i64>("select count(*) from client_tokens where scopes is not null").fetch_one(&server.pool).await.unwrap(), 0);
}

#[tokio::test]
async fn approving_with_every_scope_unticked_is_refused_and_the_request_stays_open() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;
    ask(&server, &link, conversation).await;
    let id = requests(&server).await[0]["id"].as_str().unwrap().to_string();

    let (status, body) = approve(&server, &id, json!({"scopes": []})).await;

    assert_eq!((status, body["error"].clone()), (400, json!("scopes_required")));
    assert_eq!(requests(&server).await[0]["state"], "requested");
}

#[tokio::test]
async fn another_person_cannot_decide_a_request() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;
    ask(&server, &link, conversation).await;
    let id = requests(&server).await[0]["id"].as_str().unwrap().to_string();
    let other = server.second_user("other@example.com").await;

    assert_eq!(other.json(other.post(&format!("/api/v1/api-access-requests/{id}/approve")).json(&json!({}))).await.0, 404);
    assert_eq!(other.json(other.post(&format!("/api/v1/api-access-requests/{id}/decline"))).await.0, 404);
    assert_eq!(other.json(other.get("/api/v1/api-access-requests")).await.1, json!([]));
}

#[tokio::test]
async fn a_granted_token_is_listed_with_its_scopes_expiry_and_conversation_and_revoking_it_stops_it() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;
    sqlx::query("update conversations set title = 'Debug the sync' where id = $1").bind(conversation).execute(&server.pool).await.unwrap();
    ask(&server, &link, conversation).await;
    let id = requests(&server).await[0]["id"].as_str().unwrap().to_string();
    approve(&server, &id, json!({})).await;
    let ControlToNode::ApiToken { token, .. } = link.next().await else {
        panic!("expected api_token");
    };

    let (_, sessions) = server.user.json(server.user.get("/api/v1/sessions")).await;
    let agent = sessions.as_array().unwrap().iter().find(|session| session["kind"] == "agent").unwrap().clone();

    assert_eq!((agent["scopes"].clone(), agent["conversation_id"].clone(), agent["conversation_title"].clone()), (json!(["conversations:read", "nodes:read"]), json!(conversation), json!("Debug the sync")));
    assert!(agent["expires_at"].as_str().is_some());
    assert_eq!(status_with(&server, &token, "GET", "/api/v1/nodes").await, 200);

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/sessions/{}", agent["id"].as_str().unwrap()))).await;

    assert_eq!(status, 204);
    assert_eq!(status_with(&server, &token, "GET", "/api/v1/nodes").await, 401);
}

#[tokio::test]
async fn when_the_node_cannot_save_the_token_it_is_revoked_and_the_agent_is_told() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;
    ask(&server, &link, conversation).await;
    let id = requests(&server).await[0]["id"].as_str().unwrap().to_string();
    approve(&server, &id, json!({})).await;
    let ControlToNode::ApiToken { token, .. } = link.next().await else {
        panic!("expected api_token");
    };

    link.send(NodeToControl::ApiTokenSaved { conversation_id: conversation, path: None }).await;

    eventually("the agent being told", || async { queued_texts(&server, conversation).await.into_iter().find(|text| text.contains("could not save")) }).await;
    assert_eq!(status_with(&server, &token, "GET", "/api/v1/nodes").await, 401);
}

const FULL: &[&str] = &["full"];

async fn proposed_message(server: &Server, link: &NodeLink, from: Uuid, to: Uuid) -> String {
    tool(server, link, from, "send_to_conversation", json!({"conversation": to.to_string(), "message": "The schema is ready", "reason": "You asked"})).await;
    let (_, proposals) = server.user.json(server.user.get("/api/v1/message-proposals")).await;

    proposals[0]["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn a_full_token_lists_and_declines_message_proposals_that_a_narrow_token_cannot_reach() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let from = working_conversation(&server, &mut link).await;
    let to = working_conversation(&server, &mut link).await;
    let id = proposed_message(&server, &link, from, to).await;
    let (_, full) = insert_token(&server, Some(FULL), None).await;
    let (_, narrow) = insert_token(&server, Some(&["conversations:read", "conversations:write", "nodes:read", "settings:read", "assistants"]), None).await;

    assert_eq!(status_with(&server, &narrow, "GET", "/api/v1/message-proposals").await, 403);
    assert_eq!(status_with(&server, &narrow, "POST", &format!("/api/v1/message-proposals/{id}/decline")).await, 403);
    assert_eq!(status_with(&server, &full, "GET", "/api/v1/message-proposals").await, 200);
    assert_eq!(status_with(&server, &full, "POST", &format!("/api/v1/message-proposals/{id}/decline")).await, 200);
    let (_, proposals) = server.user.json(server.user.get("/api/v1/message-proposals")).await;
    assert_eq!(proposals[0]["state"], "declined");
}

#[tokio::test]
async fn an_expired_full_token_is_refused() {
    let server = Server::start().await;
    let (_, expired) = insert_token(&server, Some(FULL), Some("-1 minute")).await;
    let (_, running) = insert_token(&server, Some(FULL), Some("1 hour")).await;

    assert_eq!(status_with(&server, &expired, "GET", "/api/v1/message-proposals").await, 401);
    assert_eq!(status_with(&server, &running, "GET", "/api/v1/message-proposals").await, 200);
}

#[tokio::test]
async fn a_full_token_cannot_reach_another_persons_data_or_a_conversation_outside_its_reach() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let own = working_conversation(&server, &mut link).await;
    let to = working_conversation(&server, &mut link).await;
    let id = proposed_message(&server, &link, own, to).await;
    server.second_user("other@example.com").await;
    sqlx::query("update message_proposals set owner = (select id from users where email = 'other@example.com') where id = $1::uuid").bind(&id).execute(&server.pool).await.unwrap();
    let (token_id, token) = insert_token(&server, Some(FULL), None).await;
    sqlx::query("update client_tokens set conversation_id = $2 where id = $1").bind(token_id).bind(own).execute(&server.pool).await.unwrap();

    let anonymous = server.anonymous();
    let listed = anonymous.http.get(format!("{}/api/v1/message-proposals", server.base)).header("authorization", format!("Bearer {token}")).send().await.unwrap();
    let listed: Value = listed.json().await.unwrap();

    assert_eq!(listed.as_array().unwrap().len(), 0);
    assert_eq!(status_with(&server, &token, "POST", &format!("/api/v1/message-proposals/{id}/decline")).await, 404);
    assert_eq!(status_with(&server, &token, "GET", &format!("/api/v1/conversations/{}/events", Uuid::new_v4())).await, 403);
}

#[tokio::test]
async fn full_access_is_requested_approved_and_shown_as_one_more_scope() {
    let server = Server::start().await;
    let mut link = server.online_node("laptop").await;
    let conversation = working_conversation(&server, &mut link).await;

    let result = tool(&server, &link, conversation, "request_api_access", json!({"scopes": ["full"], "reason": "Deciding message proposals"})).await;

    assert_eq!(result["isError"], false, "{result}");
    let listed = requests(&server).await;
    assert_eq!(listed[0]["scopes"], json!(["full"]));

    let (status, body) = approve(&server, listed[0]["id"].as_str().unwrap(), json!({})).await;
    assert_eq!((status, body["scopes"].clone()), (200, json!(["full"])), "{body}");
    let ControlToNode::ApiToken { token, .. } = link.next().await else { panic!("expected api_token") };
    assert_eq!(status_with(&server, &token, "GET", "/api/v1/message-proposals").await, 200);
}
