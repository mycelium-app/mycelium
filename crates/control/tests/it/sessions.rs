use crate::common::{Client, Server, session_cookie};
use serde_json::{Value, json};

async fn another_browser(server: &Server, user_agent: &str) -> Client {
    let response = server.anonymous().get("/auth/login").header("user-agent", user_agent).send().await.unwrap();
    let cookie = session_cookie(&response);
    let browser = Client { base: server.base.clone(), cookie, http: server.user.http.clone() };

    let (status, _) = browser.json(browser.get("/api/v1/me").header("user-agent", user_agent)).await;
    assert_eq!(status, 200);

    browser
}

async fn sessions(client: &Client) -> Vec<Value> {
    let (status, body) = client.json(client.get("/api/v1/sessions")).await;
    assert_eq!(status, 200, "{body}");

    body.as_array().unwrap().clone()
}

#[tokio::test]
async fn the_current_browser_is_listed_first_and_marked_with_where_it_signed_in_from() {
    let server = Server::start().await;
    another_browser(&server, "Mozilla/5.0 (Linux; Android 14) Chrome/130.0 Mobile Safari/537.36").await;

    let (status, _) = server.user.json(server.user.get("/api/v1/me").header("user-agent", "Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0")).await;
    assert_eq!(status, 200);

    let listed = sessions(&server.user).await;

    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0]["current"], true);
    assert_eq!(listed[0]["label"], "Firefox on Linux");
    assert_eq!(listed[0]["address"], "127.0.0.1");
    assert_eq!(listed[1]["current"], false);
    assert_eq!(listed[1]["label"], "Chrome on Android");
}

#[tokio::test]
async fn ending_another_browser_signs_it_out_at_once() {
    let server = Server::start().await;
    let other = another_browser(&server, "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15) Safari/605.1.15").await;
    let listed = sessions(&server.user).await;
    let id = listed.iter().find(|session| session["current"] == false).unwrap()["id"].as_str().unwrap().to_string();

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/sessions/{id}"))).await;
    assert_eq!(status, 204);

    let (status, _) = other.json(other.get("/api/v1/me")).await;
    assert_eq!(status, 401);
    assert_eq!(sessions(&server.user).await.len(), 1);
}

#[tokio::test]
async fn an_app_is_listed_with_its_label_and_hostname_and_ending_it_revokes_its_token() {
    let server = Server::start().await;
    let (id, token) = server.join("client", "Desktop app").await;
    let app = Client { base: server.base.clone(), cookie: String::new(), http: server.user.http.clone() };
    let bearer = format!("Bearer {token}");
    let (status, _) = app.json(app.get("/api/v1/me").header("authorization", &bearer)).await;
    assert_eq!(status, 200);

    let listed = sessions(&server.user).await;
    let entry = listed.iter().find(|session| session["id"] == json!(id)).unwrap();
    assert_eq!(entry["kind"], "client");
    assert_eq!(entry["label"], "Desktop app");
    assert_eq!(entry["hostname"], "Desktop app");
    assert_eq!(entry["address"], "127.0.0.1");
    assert_eq!(entry["current"], false);

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/sessions/{id}"))).await;
    assert_eq!(status, 204);

    let (status, _) = app.json(app.get("/api/v1/me").header("authorization", &bearer)).await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn ending_the_session_in_use_needs_confirmation() {
    let server = Server::start().await;
    let current = sessions(&server.user).await[0]["id"].as_str().unwrap().to_string();

    let (status, body) = server.user.json(server.user.delete(&format!("/api/v1/sessions/{current}"))).await;
    assert_eq!(status, 409);
    assert_eq!(body["error"], "current_session");
    assert_eq!(body["message"], "This is the session you are using. Confirm to sign out of it.");

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/sessions/{current}?confirm_current=true"))).await;
    assert_eq!(status, 204);

    let (status, _) = server.user.json(server.user.get("/api/v1/me")).await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn nobody_can_see_or_end_another_persons_session() {
    let server = Server::start().await;
    let other = server.second_user("grace@example.com").await;
    let theirs = sessions(&other).await[0]["id"].as_str().unwrap().to_string();

    assert!(sessions(&server.user).await.iter().all(|session| session["id"] != json!(theirs)));

    let (status, _) = server.user.json(server.user.delete(&format!("/api/v1/sessions/{theirs}"))).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn a_node_lists_the_address_it_connected_from() {
    let server = Server::start().await;
    server.online_node("laptop").await;

    let (_, nodes) = server.user.json(server.user.get("/api/v1/nodes")).await;

    assert_eq!(nodes[0]["address"], "127.0.0.1");
}
