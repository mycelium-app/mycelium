use axum::Router;
use axum::routing::get;
use mycelium_node::update::install;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tokio::net::TcpListener;

const ASSET: &str = "mycelium-linux-x86_64";

fn sha256_of(bytes: &[u8]) -> String {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("file");
    std::fs::write(&file, bytes).unwrap();
    let output = std::process::Command::new("sha256sum").arg(&file).output().unwrap();

    String::from_utf8(output.stdout).unwrap().split_whitespace().next().unwrap().to_string()
}

async fn serve_release(binary: &'static [u8], checksum: String) -> String {
    let path = format!("/v9.9.9/{ASSET}");
    let router = Router::new().route(&path, get(move || async move { binary })).route(&format!("{path}.sha256"), get(move || async move { format!("{checksum}  {ASSET}\n") }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move { axum::serve(listener, router).await });

    format!("http://127.0.0.1:{}", address.port())
}

fn installed_copy(directory: &Path) -> std::path::PathBuf {
    let target = directory.join("mycelium");
    std::fs::write(&target, b"old build").unwrap();

    target
}

#[tokio::test]
async fn a_verified_download_replaces_the_binary_and_leaves_it_executable() {
    let directory = tempfile::tempdir().unwrap();
    let target = installed_copy(directory.path());
    let url = serve_release(b"new build", sha256_of(b"new build")).await;

    let installed = install(&reqwest::Client::new(), &url, "v9.9.9", ASSET, &target).await;

    assert!(installed.is_ok());
    assert_eq!(std::fs::read(&target).unwrap(), b"new build");
    assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o755);
    assert!(!directory.path().join("mycelium.new").exists());
}

#[tokio::test]
async fn a_download_that_does_not_match_its_checksum_is_refused_and_the_binary_is_kept() {
    let directory = tempfile::tempdir().unwrap();
    let target = installed_copy(directory.path());
    let url = serve_release(b"tampered build", sha256_of(b"new build")).await;

    let installed = install(&reqwest::Client::new(), &url, "v9.9.9", ASSET, &target).await;

    assert!(installed.unwrap_err().to_string().contains("checksum"));
    assert_eq!(std::fs::read(&target).unwrap(), b"old build");
    assert!(!directory.path().join("mycelium.new").exists());
}

#[tokio::test]
async fn a_release_that_is_not_published_is_reported_and_the_binary_is_kept() {
    let directory = tempfile::tempdir().unwrap();
    let target = installed_copy(directory.path());
    let url = serve_release(b"new build", sha256_of(b"new build")).await;

    let installed = install(&reqwest::Client::new(), &url, "v1.0.0", ASSET, &target).await;

    assert!(installed.unwrap_err().to_string().contains("404"));
    assert_eq!(std::fs::read(&target).unwrap(), b"old build");
}

#[tokio::test]
async fn a_directory_that_cannot_be_written_is_reported_as_a_nix_install() {
    let directory = tempfile::tempdir().unwrap();
    let target = installed_copy(directory.path());
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let url = serve_release(b"new build", sha256_of(b"new build")).await;

    let installed = install(&reqwest::Client::new(), &url, "v9.9.9", ASSET, &target).await;

    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(installed.unwrap_err().to_string(), "This node was installed by Nix; update it through the flake");
}
