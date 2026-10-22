use assert_cmd::Command;
use predicates::prelude::*;

fn mycelium() -> Command {
    Command::cargo_bin("mycelium").unwrap()
}

#[test]
fn version_prints_the_package_version() {
    mycelium()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::diff(format!("{}\n", env!("CARGO_PKG_VERSION"))));
}

#[test]
fn help_lists_the_stage_one_commands() {
    mycelium()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("control").and(predicate::str::contains("node")).and(predicate::str::contains("keeper")).and(predicate::str::contains("version")));
}

#[test]
fn node_join_takes_a_url_and_an_optional_join_token_file() {
    mycelium()
        .args(["node", "join", "https://mycelium.example", "--join-token-file", "/nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Join tokens arrive in a later release"));
}

#[test]
fn keeper_requires_a_command_after_the_separator() {
    mycelium().args(["keeper", "--dir", "/tmp/x"]).assert().failure();

    mycelium()
        .args(["keeper", "--dir", "/nonexistent/keeper", "--", "agent", "--flag"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("create /nonexistent/keeper"));
}

#[test]
fn control_serve_without_configuration_says_what_to_set() {
    mycelium()
        .args(["control", "serve"])
        .env_remove("MYCELIUM_PUBLIC_URL")
        .env_remove("MYCELIUM_DATABASE_URL")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Set MYCELIUM_PUBLIC_URL."));
}

#[test]
fn control_serve_refuses_dev_login_on_a_public_address() {
    mycelium()
        .args(["control", "serve"])
        .env("MYCELIUM_PUBLIC_URL", "https://mycelium.example.com")
        .env("MYCELIUM_DATABASE_URL", "postgres://x")
        .env("MYCELIUM_DEV_LOGIN", "me@example.com")
        .assert()
        .failure()
        .stderr(predicate::str::contains("MYCELIUM_DEV_LOGIN is refused"));
}

#[test]
fn control_serve_answers_healthz_and_reports_not_ready_while_the_database_is_down() {
    use std::io::{Read, Write};

    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = free.local_addr().unwrap().port();
    drop(free);

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_mycelium"))
        .args(["control", "serve"])
        .env("MYCELIUM_PUBLIC_URL", format!("http://127.0.0.1:{port}"))
        .env("MYCELIUM_LISTEN", format!("127.0.0.1:{port}"))
        .env("MYCELIUM_DATABASE_URL", "postgres://postgres:pw@127.0.0.1:1/none")
        .env("MYCELIUM_DEV_LOGIN", "me@example.com")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let get = |path: &str| -> String {
        for _ in 0..200 {
            if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                stream.write_all(format!("GET {path} HTTP/1.0\r\n\r\n").as_bytes()).unwrap();
                let mut response = String::new();
                stream.read_to_string(&mut response).unwrap();

                return response;
            }

            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        String::new()
    };

    let healthz = get("/healthz");
    let readyz = loop {
        let readyz = get("/readyz");

        if readyz.contains("migrations_failed") {
            break readyz;
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    };

    child.kill().unwrap();

    assert!(healthz.starts_with("HTTP/1.0 200"), "{healthz}");
    assert!(readyz.starts_with("HTTP/1.0 503"), "{readyz}");
}

fn node_config_for(control_plane: &str, dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("node.toml");
    let text = format!(
        "control_plane = \"{control_plane}\"\nnode_id = \"00000000-0000-0000-0000-000000000001\"\ntoken = \"myc_node_test\"\nstate_dir = \"{}\"\nprojects_root = \"{}\"\n",
        dir.join("state").display(),
        dir.join("projects").display()
    );
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn node_run_connects_over_tls_without_panicking() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            drop(stream);
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let config = node_config_for(&format!("https://127.0.0.1:{port}"), dir.path());

    let output = mycelium()
        .args(["node", "run"])
        .env("MYCELIUM_NODE_CONFIG", &config)
        .timeout(std::time::Duration::from_secs(4))
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("panicked"), "node run panicked: {stderr}");
    assert!(!stderr.contains("CryptoProvider"), "no TLS provider chosen: {stderr}");
}

#[test]
fn node_run_joins_first_when_given_a_control_plane_and_no_credential() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("node.toml");

    let output = mycelium()
        .args(["node", "run", "--control-plane", "http://127.0.0.1:9"])
        .env("MYCELIUM_NODE_CONFIG", &config)
        .timeout(std::time::Duration::from_secs(10))
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Could not reach http://127.0.0.1:9"), "node run did not try to join: {stderr}");
}
