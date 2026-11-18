use std::path::Path;
use std::process::Command;

fn fake_vibe() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../node/tests/fake-vibe-acp").canonicalize().unwrap().display().to_string()
}

#[test]
fn signing_in_to_mistral_vibe_shows_the_sign_in_page_then_saves_the_key_where_the_node_sees_it() {
    let home = tempfile::tempdir().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_mycelium")).args(["node", "vibe-sign-in"]).env("MYCELIUM_VIBE_ACP", fake_vibe()).env("VIBE_HOME", home.path()).env_remove("PYTHON_KEYRING_BACKEND").output().unwrap();
    let printed = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "{printed}{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(printed, "Open this page to sign in to Mistral Vibe with your Mistral account: https://console.example.test/vibe/sign-in?attempt=a1\nSigned in to Mistral Vibe.\n");
    assert_eq!(std::fs::read_to_string(home.path().join(".env")).unwrap(), "MISTRAL_API_KEY=signed-in-placeholder\n");
}
