mod common;

use common::{Server, create_database, eventually};
use serde_json::json;
use sqlx::Connection;
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn readyz_is_200_once_migrations_have_run() {
    let server = Server::start().await;
    let anonymous = server.anonymous();

    let (health, _) = anonymous.json(anonymous.get("/healthz")).await;
    let (ready, body) = anonymous.json(anonymous.get("/readyz")).await;

    assert_eq!(health, 200);
    assert_eq!(ready, 200);
    assert_eq!(body["status"], "ready");
}

#[tokio::test]
async fn readyz_is_503_and_healthz_stays_200_when_the_database_is_unreachable_at_boot() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed_port = listener.local_addr().unwrap().port();
    drop(listener);

    let directory = tempfile::tempdir().unwrap();
    let address = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let base = format!("http://127.0.0.1:{}", address.port());
        let config = mycelium_control::Config {
            database_url: format!("postgres://postgres:pw@127.0.0.1:{closed_port}/mycelium"),
            listen: address,
            public_url: base,
            data_dir: directory.path().to_path_buf(),
            oidc: None,
            dev_login: Some("dev@example.com".into()),
            allowed_emails: Vec::new(),
            web_dir: directory.path().to_path_buf(),
            skip_migrations: false,
            trusted_proxies: Vec::new(),
        device_code_lifetime: mycelium_control::DEVICE_CODE_LIFETIME,
        release_url: "http://127.0.0.1:1/latest".into(),
        npm_registry_url: "http://127.0.0.1:1".into(),
        version_check_interval: mycelium_control::VERSION_CHECK_INTERVAL,
        scheduler_tick: mycelium_control::SCHEDULER_TICK,
        };

        tokio::spawn(mycelium_control::run(config, listener));

        address
    };

    let http = reqwest::Client::new();
    let base = format!("http://{address}");

    let health = http.get(format!("{base}/healthz")).send().await.unwrap();
    assert_eq!(health.status(), 200);

    let unready = eventually("readyz to report the failure", || async {
        let response = http.get(format!("{base}/readyz")).send().await.unwrap();
        let status = response.status();
        let body: serde_json::Value = response.json().await.unwrap();

        (body["error"] == "migrations_failed").then_some((status, body))
    })
    .await;

    assert_eq!(unready.0, 503);
    assert_eq!(unready.1["message"], "Migrations have not been applied.");
    assert_eq!(http.get(format!("{base}/healthz")).send().await.unwrap().status(), 200);
}

#[tokio::test]
async fn readyz_turns_503_when_the_database_connection_is_cut() {
    let real = create_database().await;
    let real_address = real.split('@').nth(1).unwrap().split('/').next().unwrap().to_string();
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_port = proxy.local_addr().unwrap().port();
    let through_proxy = real.replacen(&real_address, &format!("127.0.0.1:{proxy_port}"), 1);
    let (cut, mut cut_signal) = tokio::sync::watch::channel(false);

    tokio::spawn(async move {
        loop {
            tokio::select! {
                accepted = proxy.accept() => {
                    let Ok((mut inbound, _)) = accepted else {
                        break;
                    };
                    let upstream = real_address.clone();
                    let mut signal = cut_signal.clone();

                    tokio::spawn(async move {
                        let Ok(mut outbound) = TcpStream::connect(upstream).await else {
                            return;
                        };

                        tokio::select! {
                            _ = copy_bidirectional(&mut inbound, &mut outbound) => {}
                            _ = signal.changed() => {}
                        }
                    });
                }
                _ = cut_signal.changed() => break,
            }
        }
    });

    let server = Server::start_on(through_proxy, |_| {}).await;
    let anonymous = server.anonymous();

    let (before, _) = anonymous.json(anonymous.get("/readyz")).await;
    assert_eq!(before, 200);

    cut.send(true).unwrap();

    let (status, body) = eventually("readyz to notice the cut", || async {
        let (status, body) = anonymous.json(anonymous.get("/readyz")).await;

        (status == 503).then_some((status, body))
    })
    .await;

    assert_eq!(status, 503);
    assert_eq!(body["error"], "database_unreachable");
    assert_eq!(body["message"], "The database is not reachable.");
    assert_eq!(anonymous.json(anonymous.get("/healthz")).await.0, 200);
}

#[tokio::test]
async fn a_failed_migration_keeps_the_process_up_and_reports_the_error() {
    let database_url = create_database().await;
    let mut connection = sqlx::PgConnection::connect(&database_url).await.unwrap();
    sqlx::query("create table users (already_here int)").execute(&mut connection).await.unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let config = mycelium_control::Config {
        database_url,
        listen: address,
        public_url: format!("http://{address}"),
        data_dir: directory.path().to_path_buf(),
        oidc: None,
        dev_login: Some("dev@example.com".into()),
        allowed_emails: Vec::new(),
        web_dir: directory.path().to_path_buf(),
        skip_migrations: false,
        trusted_proxies: Vec::new(),
        device_code_lifetime: mycelium_control::DEVICE_CODE_LIFETIME,
        release_url: "http://127.0.0.1:1/latest".into(),
        npm_registry_url: "http://127.0.0.1:1".into(),
        version_check_interval: mycelium_control::VERSION_CHECK_INTERVAL,
        scheduler_tick: mycelium_control::SCHEDULER_TICK,
    };

    tokio::spawn(mycelium_control::run(config, listener));

    let http = reqwest::Client::new();
    let body = eventually("the migration failure", || async {
        let response = http.get(format!("http://{address}/readyz")).send().await.unwrap();
        let body: serde_json::Value = response.json().await.unwrap();

        (body["error"] == "migrations_failed").then_some(body)
    })
    .await;

    assert_eq!(body["message"], "Migrations have not been applied.");
    assert!(!body.to_string().contains("users"), "{body}");
    assert_eq!(http.get(format!("http://{address}/healthz")).send().await.unwrap().status(), 200);
}

#[tokio::test]
async fn two_control_planes_booting_on_one_database_both_become_ready() {
    let first = Server::start().await;
    let second = Server::start_on(first.database_url.clone(), |_| {}).await;

    assert_eq!(second.anonymous().json(second.anonymous().get("/readyz")).await.0, 200);

    let applied = sqlx::query_scalar::<_, i64>("select count(*) from _sqlx_migrations").fetch_one(&first.pool).await.unwrap();
    let shipped = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations")).unwrap().count();
    assert_eq!(applied, shipped as i64);
}

#[tokio::test]
async fn the_web_client_is_served_with_single_page_fallback_but_unknown_api_paths_are_json_404s() {
    let server = Server::start().await;
    std::fs::write(server.web_dir.join("index.html"), "<html>mycelium</html>").unwrap();
    std::fs::write(server.web_dir.join("app.js"), "console.log(1)").unwrap();
    let anonymous = server.anonymous();

    let route = anonymous.get("/approve?code=WDJB-MJHT").send().await.unwrap();
    assert_eq!(route.status(), 200);
    assert_eq!(route.text().await.unwrap(), "<html>mycelium</html>");

    let script = anonymous.get("/app.js").send().await.unwrap();
    assert_eq!(script.text().await.unwrap(), "console.log(1)");

    let (status, body) = anonymous.json(anonymous.get("/api/v1/nothing-here")).await;
    assert_eq!(status, 404);
    assert_eq!(body["error"], json!("not_found"));
}
