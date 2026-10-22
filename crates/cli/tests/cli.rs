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
        .stderr(predicate::str::contains("not built yet"));
}

#[test]
fn keeper_requires_a_command_after_the_separator() {
    mycelium().args(["keeper", "--dir", "/tmp/x"]).assert().failure();

    mycelium()
        .args(["keeper", "--dir", "/tmp/x", "--", "agent", "--flag"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not built yet"));
}

#[test]
fn unbuilt_commands_exit_non_zero_with_a_reason() {
    for args in [["control", "serve"].as_slice(), ["node", "run"].as_slice(), ["node", "status"].as_slice()] {
        mycelium().args(args).assert().failure().stderr(predicate::str::contains("not built yet"));
    }
}
