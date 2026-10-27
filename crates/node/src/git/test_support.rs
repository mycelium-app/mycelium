use super::{git, run};
use mycelium_proto::link::{CommitOptions, GitCommand};
use serde_json::Value;
use std::path::Path;

pub(crate) fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());

    dir
}

pub(crate) fn init(path: &Path) {
    for args in [&["init", "-b", "main"][..], &["config", "user.name", "Tester"], &["config", "user.email", "tester@example.com"], &["config", "commit.gpgsign", "false"]] {
        git(path, args, &[]).unwrap();
    }
}

pub(crate) fn write(dir: &tempfile::TempDir, path: &str, text: &str) {
    let full = dir.path().join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, text).unwrap();
}

pub(crate) fn commit_all(dir: &tempfile::TempDir, message: &str) {
    run(dir.path(), GitCommand::StageAll).unwrap();
    run(dir.path(), GitCommand::Commit { message: message.into(), options: CommitOptions::default() }).unwrap();
}

pub(crate) fn file<'a>(status: &'a Value, path: &str) -> &'a Value {
    status["files"].as_array().unwrap().iter().find(|file| file["path"] == path).unwrap_or_else(|| panic!("{path} missing from {status}"))
}

pub(crate) fn clone_of(origin: &tempfile::TempDir) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["clone", "--", origin.path().to_str().unwrap(), "."], &[]).unwrap();
    for args in [&["config", "user.name", "Tester"][..], &["config", "user.email", "tester@example.com"], &["config", "commit.gpgsign", "false"]] {
        git(dir.path(), args, &[]).unwrap();
    }

    dir
}

pub(crate) fn bare_origin_with_a_commit() -> tempfile::TempDir {
    let work = repository();
    write(&work, "a.txt", "one\n");
    commit_all(&work, "first");
    let origin = tempfile::tempdir().unwrap();
    git(origin.path(), &["clone", "--bare", "--", work.path().to_str().unwrap(), "."], &[]).unwrap();

    origin
}
