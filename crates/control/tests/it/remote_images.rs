#[path = "common/fake_image_origin.rs"]
mod fake_image_origin;

use axum::Json;
use axum::Router;
use axum::routing::get;
use crate::common::{Client, NodeLink, Server, serve};
use fake_image_origin::{Origin, Reply, certificate_path, png, start};
use mycelium_control::PublicFetchRouting;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

struct World {
    server: Server,
    node: NodeLink,
    conversation: Uuid,
    found: Arc<Mutex<Vec<String>>>,
    trusted: Origin,
    other: Origin,
    cdn: Origin,
}

struct Answer {
    status: u16,
    headers: reqwest::header::HeaderMap,
    body: Vec<u8>,
}

impl Answer {
    fn error(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    fn code(&self) -> String {
        self.error()["error"].as_str().unwrap_or_default().to_string()
    }
}

impl World {
    async fn start() -> World {
        let (trusted, other, cdn) = (start().await, start().await, start().await);
        let found: Arc<Mutex<Vec<String>>> = Arc::default();
        let listed = found.clone();
        let search = serve(Router::new().route("/search", get(move || {
            let results: Vec<Value> = listed.lock().unwrap().iter().map(|url| json!({"title": "Result", "img_src": url, "url": "https://shop.example.com/page"})).collect();

            async move { Json(json!({"results": results})) }
        })))
        .await;
        let loopback = |port: u16| std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let routing = PublicFetchRouting {
            hosts: vec![("trusted.test".into(), loopback(trusted.port)), ("other.test".into(), loopback(other.port)), ("cdn.test".into(), loopback(cdn.port)), ("private.test".into(), "10.0.0.1:0".parse().unwrap())],
            trusted_root_pem: Some(std::fs::read(certificate_path()).unwrap()),
            allow_loopback: true,
        };
        let server = Server::start_with(|config| config.public_fetch = routing).await;
        let node = server.online_node("laptop").await;

        server.user.json(server.user.post("/api/v1/chat-tool-servers").json(&json!({"name": "Search", "kind": "web_search", "url": search}))).await;

        let conversation = server.node_conversation(&node.token).await;

        World { server, node, conversation, found, trusted, other, cdn }
    }

    fn url(&self, origin: &Origin, path: &str) -> String {
        let name = if origin.port == self.trusted.port { "trusted" } else if origin.port == self.other.port { "other" } else { "cdn" };

        format!("https://{name}.test:{}{path}", origin.port)
    }

    async fn search_in(&self, conversation: Uuid, urls: &[String]) {
        *self.found.lock().unwrap() = urls.to_vec();

        let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "search__search_images", "arguments": {"query": "anything"}}});
        let reply = self.server.agent_mcp(&self.node.token, conversation, body).await;

        assert_ne!(reply["result"]["isError"], true, "{reply}");
    }

    async fn search(&self, urls: &[String]) {
        self.search_in(self.conversation, urls).await;
    }

    async fn ask(&self, client: &Client, conversation: Uuid, url: &str, click: bool) -> Answer {
        let address = format!("/api/v1/conversations/{conversation}/remote-images");
        let request = if click { client.post(&address).json(&json!({"url": url})) } else { client.get(&address).query(&[("url", url)]) };
        let response = request.send().await.unwrap();

        Answer { status: response.status().as_u16(), headers: response.headers().clone(), body: response.bytes().await.unwrap().to_vec() }
    }

    async fn get(&self, url: &str) -> Answer {
        self.ask(&self.server.user, self.conversation, url, false).await
    }

    async fn click(&self, url: &str) -> Answer {
        self.ask(&self.server.user, self.conversation, url, true).await
    }

    fn requests(&self) -> usize {
        self.trusted.count() + self.other.count() + self.cdn.count()
    }
}

fn sha256_of(body: &[u8]) -> String {
    Sha256::digest(body).iter().map(|byte| format!("{byte:02x}")).collect()
}

#[tokio::test]
async fn an_image_no_search_returned_is_never_fetched_before_a_click() {
    let world = World::start().await;
    world.trusted.route("/a.png", Reply::png(4, 4));
    world.other.route("/x.png", Reply::png(4, 4));
    world.search(&[world.url(&world.trusted, "/a.png")]).await;
    let unrecorded = [world.url(&world.other, "/x.png"), world.url(&world.other, "/x.png?size=large"), world.url(&world.trusted, "/a.png?"), world.url(&world.trusted, "/a.pnG"), world.url(&world.trusted, "/a.png#fragment")];

    for address in &unrecorded {
        let first = world.get(address).await;
        let second = world.get(address).await;

        assert_eq!((first.status, first.code()), (403, "image_needs_click".to_string()), "{address}");
        assert_eq!(second.code(), "image_needs_click", "{address}");
    }

    assert_eq!(world.get(&unrecorded[0]).await.error()["host"], "other.test");
    assert_eq!(world.requests(), 0);
}

#[tokio::test]
async fn a_click_loads_that_one_image_and_later_requests_are_served_from_the_cache() {
    let world = World::start().await;
    world.other.route("/x.png", Reply::png(4, 4));
    world.other.route("/y.png", Reply::png(5, 5));
    let clicked = world.url(&world.other, "/x.png");
    let not_clicked = world.url(&world.other, "/y.png");

    let loaded = world.click(&clicked).await;
    let again = world.get(&clicked).await;
    let other_one = world.get(&not_clicked).await;

    assert_eq!(loaded.status, 200);
    assert_eq!(loaded.headers["content-type"], "image/png");
    assert_eq!((again.status, again.body.clone()), (200, loaded.body.clone()));
    assert_eq!(other_one.code(), "image_needs_click");
    assert_eq!(world.other.count(), 1);
}

#[tokio::test]
async fn a_url_from_search_images_loads_at_once_with_safe_headers_and_is_then_served_from_the_cache() {
    let world = World::start().await;
    world.trusted.route("/a.png", Reply::png(4, 4));
    let address = world.url(&world.trusted, "/a.png");
    world.search(std::slice::from_ref(&address)).await;

    let first = world.get(&address).await;
    let second = world.get(&address).await;

    assert_eq!(first.status, 200);
    assert_eq!(first.headers["x-content-type-options"], "nosniff");
    assert_eq!(first.headers["content-security-policy"], "sandbox");
    assert_eq!(first.headers["etag"].to_str().unwrap(), format!("\"{}\"", sha256_of(&first.body)));
    assert_eq!(second.body, first.body);
    assert_eq!(world.trusted.count(), 1);
}

#[tokio::test]
async fn the_fetch_sends_no_referer_and_no_cookies_and_names_itself_mycelium() {
    let world = World::start().await;
    world.trusted.route("/a.png", Reply::png(4, 4));
    let address = world.url(&world.trusted, "/a.png");
    world.search(std::slice::from_ref(&address)).await;

    world.get(&address).await;

    let request = world.trusted.requests().remove(0);

    assert_eq!(request.header("user-agent"), Some("mycelium"));
    assert!(request.header("accept").unwrap().contains("image/avif"));
    assert_eq!((request.header("referer"), request.header("cookie")), (None, None));
}

#[tokio::test]
async fn trust_belongs_to_one_conversation_and_one_owner() {
    let world = World::start().await;
    world.trusted.route("/a.png", Reply::png(4, 4));
    let address = world.url(&world.trusted, "/a.png");
    world.search(std::slice::from_ref(&address)).await;
    let sibling = world.server.node_conversation(&world.node.token).await;
    let stranger = world.server.second_user("other@example.com").await;

    let elsewhere = world.ask(&world.server.user, sibling, &address, false).await;
    let theirs = world.ask(&stranger, world.conversation, &address, false).await;
    let clicked_theirs = world.ask(&stranger, world.conversation, &address, true).await;

    assert_eq!(elsewhere.code(), "image_needs_click");
    assert_eq!((theirs.status, clicked_theirs.status), (404, 404));
    assert_eq!(world.trusted.count(), 0);
}

#[tokio::test]
async fn a_redirect_from_a_recorded_url_to_an_unrecorded_host_does_not_autoload() {
    let world = World::start().await;
    world.trusted.route("/a.png", Reply::redirect(&world.url(&world.other, "/x.png")));
    world.other.route("/x.png", Reply::png(4, 4));
    let address = world.url(&world.trusted, "/a.png");
    world.search(std::slice::from_ref(&address)).await;

    let answer = world.get(&address).await;

    assert_eq!((answer.status, answer.code()), (403, "image_needs_click".to_string()));
    assert_eq!(world.other.count(), 0);
}

#[tokio::test]
async fn a_redirect_within_the_same_host_autoloads_and_one_to_another_recorded_url_autoloads() {
    let world = World::start().await;
    world.trusted.route("/a.png", Reply::redirect("/b.png"));
    world.trusted.route("/b.png", Reply::png(4, 4));
    world.trusted.route("/via-cdn.png", Reply::redirect(&world.url(&world.cdn, "/c.png")));
    world.cdn.route("/c.png", Reply::png(4, 4));
    let (same_host, to_cdn) = (world.url(&world.trusted, "/a.png"), world.url(&world.trusted, "/via-cdn.png"));
    world.search(&[same_host.clone(), to_cdn.clone()]).await;

    let first = world.get(&same_host).await;
    let before_recording = world.get(&to_cdn).await;

    assert_eq!(first.status, 200);
    assert_eq!((before_recording.status, before_recording.code()), (403, "image_needs_click".to_string()));
    assert_eq!(world.cdn.count(), 0);

    world.search(&[same_host, to_cdn.clone(), world.url(&world.cdn, "/c.png")]).await;

    assert_eq!(world.get(&to_cdn).await.status, 200);
}

#[tokio::test]
async fn an_unrecorded_url_cannot_autoload_by_redirecting_to_a_recorded_one() {
    let world = World::start().await;
    world.other.route("/x.png", Reply::redirect(&world.url(&world.trusted, "/a.png")));
    world.trusted.route("/a.png", Reply::png(4, 4));
    world.search(&[world.url(&world.trusted, "/a.png")]).await;

    let answer = world.get(&world.url(&world.other, "/x.png")).await;

    assert_eq!(answer.code(), "image_needs_click");
    assert_eq!(world.requests(), 0);
}

#[tokio::test]
async fn a_trusted_chain_is_pinned_to_the_hops_of_its_first_fetch() {
    let world = World::start().await;
    world.trusted.route("/a.png", Reply::redirect("/b.png"));
    world.trusted.route("/b.png", Reply::png(4, 4));
    let address = world.url(&world.trusted, "/a.png");
    world.search(std::slice::from_ref(&address)).await;
    world.get(&address).await;

    for blob in std::fs::read_dir(world.server.data_dir.join("attachments")).unwrap() {
        std::fs::remove_file(blob.unwrap().path()).unwrap();
    }

    world.trusted.route("/a.png", Reply::redirect("/c.png"));
    world.trusted.route("/c.png", Reply::png(5, 5));

    let answer = world.get(&address).await;

    assert_eq!(answer.code(), "image_needs_click");
    assert_eq!(world.trusted.count_of("/c.png"), 0);
}

#[tokio::test]
async fn a_blob_that_is_lost_is_fetched_again_along_the_same_hops() {
    let world = World::start().await;
    world.trusted.route("/a.png", Reply::redirect("/b.png"));
    world.trusted.route("/b.png", Reply::png(4, 4));
    let address = world.url(&world.trusted, "/a.png");
    world.search(std::slice::from_ref(&address)).await;
    world.get(&address).await;

    for blob in std::fs::read_dir(world.server.data_dir.join("attachments")).unwrap() {
        std::fs::remove_file(blob.unwrap().path()).unwrap();
    }

    let answer = world.get(&address).await;

    assert_eq!(answer.status, 200);
    assert_eq!(world.trusted.count_of("/b.png"), 2);
}

#[tokio::test]
async fn every_hop_is_checked_for_private_addresses_and_plain_http_before_it_is_requested() {
    let world = World::start().await;
    let private = world.url(&world.trusted, "/private.png").replace("trusted.test", "private.test");
    world.trusted.route("/to-private-name.png", Reply::redirect(&private));
    world.trusted.route("/to-private-ip.png", Reply::redirect("http://10.0.0.1/x"));
    world.trusted.route("/to-http.png", Reply::redirect("http://example.com/x.png"));
    let addresses: Vec<String> = ["/to-private-name.png", "/to-private-ip.png", "/to-http.png"].iter().map(|path| world.url(&world.trusted, path)).collect();
    world.search(&[addresses.clone(), vec![private]].concat()).await;

    for (address, expected) in addresses.iter().zip(["image_private_address", "image_private_address", "image_not_https"]) {
        let trusted_get = world.get(address).await;
        let clicked = world.click(address).await;

        assert_eq!((trusted_get.status, trusted_get.code()), (422, expected.to_string()), "{address}");
        assert_eq!((clicked.status, clicked.code()), (422, expected.to_string()), "{address}");
    }
}

#[tokio::test]
async fn a_redirect_to_a_private_name_nobody_recorded_stops_a_trusted_fetch_without_resolving_it() {
    let world = World::start().await;
    let private = world.url(&world.trusted, "/x.png").replace("trusted.test", "private.test");
    world.trusted.route("/a.png", Reply::redirect(&private));
    let address = world.url(&world.trusted, "/a.png");
    world.search(std::slice::from_ref(&address)).await;

    assert_eq!(world.get(&address).await.code(), "image_needs_click");
}

#[tokio::test]
async fn a_sixth_redirect_is_refused() {
    let world = World::start().await;

    for step in 0..7 {
        world.other.route(&format!("/hop{step}.png"), Reply::redirect(&format!("/hop{}.png", step + 1)));
    }

    world.other.route("/hop7.png", Reply::png(4, 4));

    let answer = world.click(&world.url(&world.other, "/hop0.png")).await;

    assert_eq!(answer.status, 422);
    assert_eq!(world.other.count_of("/hop6.png"), 0);
}

#[tokio::test]
async fn an_image_over_ten_mebibytes_is_refused_whether_announced_or_streamed() {
    let world = World::start().await;
    world.other.route("/streamed.png", Reply::bytes("image/png", png(4, 4, 10 * 1024 * 1024)));
    world.other.route("/announced.png", Reply::bytes("image/png", png(4, 4, 0)).announcing(20 * 1024 * 1024));

    let streamed = world.click(&world.url(&world.other, "/streamed.png")).await;
    let announced = world.click(&world.url(&world.other, "/announced.png")).await;

    assert_eq!((streamed.status, streamed.code()), (422, "image_too_large".to_string()));
    assert_eq!((announced.status, announced.code()), (422, "image_too_large".to_string()));
    assert_eq!(announced.error()["message"], "The image is 20 MB, over the 10 MB limit.");
}

#[tokio::test]
async fn an_image_over_forty_megapixels_is_refused_from_its_header() {
    let world = World::start().await;
    world.other.route("/big.png", Reply::bytes("image/png", png(8000, 5001, 0)));
    world.other.route("/fits.png", Reply::bytes("image/png", png(8000, 5000, 0)));

    let big = world.click(&world.url(&world.other, "/big.png")).await;
    let fits = world.click(&world.url(&world.other, "/fits.png")).await;

    assert_eq!((big.status, big.code()), (422, "image_too_large".to_string()));
    assert_eq!(fits.status, 200);
}

#[tokio::test]
async fn the_type_comes_from_the_bytes_so_html_and_svg_are_refused_whatever_they_are_labelled() {
    let world = World::start().await;
    world.other.route("/page.png", Reply::bytes("image/png", b"<!doctype html><title>Sign in</title>".to_vec()));
    world.other.route("/drawing.png", Reply::bytes("image/png", b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec()));
    world.other.route("/mislabelled.png", Reply::bytes("text/html", png(4, 4, 0)));

    let page = world.click(&world.url(&world.other, "/page.png")).await;
    let drawing = world.click(&world.url(&world.other, "/drawing.png")).await;
    let mislabelled = world.click(&world.url(&world.other, "/mislabelled.png")).await;

    assert_eq!((page.status, page.code()), (422, "image_not_image".to_string()));
    assert_eq!(drawing.code(), "image_not_image");
    assert_eq!((mislabelled.status, mislabelled.headers["content-type"].to_str().unwrap()), (200, "image/png"));
}

#[tokio::test]
async fn an_address_with_userinfo_a_literal_ip_or_http_is_refused_before_any_connection() {
    let world = World::start().await;
    let port = world.other.port;
    let refused = [
        (format!("https://user:word@other.test:{port}/x.png"), "image_address_refused"),
        ("https://93.184.215.14/x.png".to_string(), "image_address_refused"),
        ("https://127.0.0.1/x.png".to_string(), "image_private_address"),
        (format!("http://other.test:{port}/x.png"), "image_not_https"),
    ];

    for (address, code) in refused {
        let from_get = world.get(&address).await;
        let from_click = world.click(&address).await;

        assert_eq!((from_get.status, from_get.code()), (422, code.to_string()), "{address}");
        assert_eq!((from_click.status, from_click.code()), (422, code.to_string()), "{address}");
    }

    assert_eq!(world.requests(), 0);
}

#[tokio::test]
async fn a_refused_load_is_remembered_so_scrolling_does_not_fetch_it_again() {
    let world = World::start().await;
    world.trusted.route("/gone.png", Reply::status(404));
    let address = world.url(&world.trusted, "/gone.png");
    world.search(std::slice::from_ref(&address)).await;

    let first = world.get(&address).await;
    let second = world.get(&address).await;

    assert_eq!((first.status, first.code()), (422, "image_unavailable".to_string()));
    assert_eq!(second.code(), "image_unavailable");
    assert_eq!(world.trusted.count(), 1);
}

#[tokio::test]
async fn a_clicked_image_that_failed_shows_its_reason_when_asked_again_without_fetching() {
    let world = World::start().await;
    world.other.route("/gone.png", Reply::status(404));
    let address = world.url(&world.other, "/gone.png");

    let clicked = world.click(&address).await;
    let later = world.get(&address).await;

    assert_eq!(clicked.code(), "image_unavailable");
    assert_eq!(later.code(), "image_unavailable");
    assert_eq!(world.other.count(), 1);
}
