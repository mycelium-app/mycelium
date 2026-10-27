use super::repository::credential_env;
use super::{git, git_with_env};
use base64::Engine;
use mycelium_proto::link::{CommitOptions, GitCredentials};
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn changed(cwd: &Path, args: &[&str], paths: &[String]) -> Result<Value, String> {
    let mut all = args.to_vec();
    all.extend(paths.iter().map(String::as_str));
    git(cwd, &all, &[])?;

    Ok(json!({}))
}

pub(super) fn commit(cwd: &Path, message: &str, options: &CommitOptions) -> Result<Value, String> {
    let mut args = vec!["commit"];

    if options.amend {
        args.push("--amend");
    }

    if options.signoff {
        args.push("--signoff");
    }

    if options.no_verify {
        args.push("--no-verify");
    }

    if options.amend && message.is_empty() {
        args.push("--no-edit");
    } else {
        args.extend(["-m", message]);
    }

    changed(cwd, &args, &[])
}

pub(super) fn push(cwd: &Path, force: bool) -> Result<Value, String> {
    let tracking = git(cwd, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"], &[]).is_ok();
    let remotes = String::from_utf8_lossy(&git(cwd, &["remote"], &[])?).lines().map(str::to_string).collect::<Vec<_>>();
    let remote = remotes.iter().find(|name| *name == "origin").or(remotes.first()).cloned();
    let mut args = vec!["push"];

    if force {
        args.push("--force-with-lease");
    }

    match (tracking, remote.as_deref()) {
        (true, _) => {}
        (false, Some(remote)) => args.extend(["--set-upstream", remote, "HEAD"]),
        (false, None) => return Err("This repository has no remote to push to. Add one with git remote add, then push again.".to_string()),
    }

    changed(cwd, &args, &[])
}

pub(super) fn undo_last_commit(cwd: &Path) -> Result<Value, String> {
    let message = String::from_utf8_lossy(&git(cwd, &["log", "-1", "--format=%B"], &[])?).trim_end().to_string();
    git(cwd, &["reset", "--soft", "HEAD~1"], &[])?;

    Ok(json!({"message": message}))
}

pub(super) fn remote(cwd: &Path, action: &str, force: bool, url: &str, allowed_urls: &[String], credentials: &GitCredentials) -> Result<Value, String> {
    if !matches!(action, "fetch" | "pull" | "push") {
        return Err("Choose fetch, pull or push.".into());
    }

    let configured = git(cwd, &["remote", "get-url", "origin"], &[])?;
    let configured = String::from_utf8_lossy(&configured);

    let configured = configured.trim();
    let push_url = git(cwd, &["remote", "get-url", "--push", "origin"], &[])?;
    let push_url = String::from_utf8_lossy(&push_url);
    let approved = |candidate: &str| candidate == url || allowed_urls.iter().any(|allowed| allowed == candidate);

    if !approved(configured) || (action == "push" && !approved(push_url.trim())) {
        return Err("The project's remote has changed. Register its current forge repository before using the forge token.".into());
    }

    let mut env = credential_env(url, credentials)?;
    env[0].1 = "2".into();
    env.push(("GIT_CONFIG_KEY_1".into(), format!("url.{url}.insteadOf")));
    env.push(("GIT_CONFIG_VALUE_1".into(), configured.into()));

    if action == "push" && push_url.trim() != configured {
        env[0].1 = "3".into();
        env.push(("GIT_CONFIG_KEY_2".into(), format!("url.{url}.insteadOf")));
        env.push(("GIT_CONFIG_VALUE_2".into(), push_url.trim().into()));
    }

    let mut args = vec![action];

    if action == "push" {
        if force {
            args.push("--force-with-lease");
        }

        args.extend(["--set-upstream", "origin", "HEAD"]);
    } else {
        args.push("origin");
    }

    let output = git_with_env(cwd, &args, &[], &env);

    match output {
        Ok(_) => Ok(json!({})),
        Err(problem) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", credentials.username, credentials.password));

            Err(problem.replace(&encoded, "[redacted]").replace(&credentials.password, "[redacted]"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::run;
    use crate::git::test_support::*;
    use mycelium_proto::link::GitCommand;

    #[test]
    fn forge_credentials_are_refused_when_origin_is_a_different_repository() {
        let dir = repository();
        git(dir.path(), &["remote", "add", "origin", "https://other.example/repo.git"], &[]).unwrap();
        let credentials = GitCredentials { username: "git".into(), password: "test-private-token".into() };
        let error = remote(dir.path(), "push", false, "https://forge.example/repo.git", &[], &credentials).unwrap_err();
        assert!(error.contains("remote has changed"));
        assert!(!error.contains(&credentials.password));
    }

    #[test]
    fn a_forges_approved_ssh_remote_uses_https_credentials_without_changing_its_configuration() {
        use std::io::{Read, Write};

        let dir = repository();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/repository.git", listener.local_addr().unwrap());
        let credentials = GitCredentials { username: "git".into(), password: "test-private-token".into() };
        let expected = base64::engine::general_purpose::STANDARD.encode("git:test-private-token");
        let serving = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut bytes = [0; 8192];
            let read = stream.read(&mut bytes).unwrap();
            let received = String::from_utf8_lossy(&bytes[..read]).contains(&format!("Authorization: Basic {expected}"));
            stream.write_all(b"HTTP/1.1 500 Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();

            received
        });
        let ssh = "ssh://git@forge.example/repository.git";
        git(dir.path(), &["remote", "add", "origin", ssh], &[]).unwrap();
        assert!(remote(dir.path(), "fetch", false, &url, &[ssh.into()], &credentials).is_err());
        assert!(serving.join().unwrap());
        assert_eq!(String::from_utf8(git(dir.path(), &["remote", "get-url", "origin"], &[]).unwrap()).unwrap().trim(), ssh);
    }

    #[test]
    fn pushing_a_new_branch_publishes_it_and_a_clone_pulls_it_back() {
        let origin = tempfile::tempdir().unwrap();
        git(origin.path(), &["init", "--bare", "-b", "main"], &[]).unwrap();
        let dir = repository();
        git(dir.path(), &["remote", "add", "origin", origin.path().to_str().unwrap()], &[]).unwrap();
        write(&dir, "a.txt", "one\n");
        commit_all(&dir, "first");

        run(dir.path(), GitCommand::Push { force: false }).unwrap();

        assert_eq!(run(dir.path(), GitCommand::Status).unwrap()["upstream"], "origin/main");

        let other = tempfile::tempdir().unwrap();
        git(other.path(), &["clone", "--quiet", origin.path().to_str().unwrap(), "."], &[]).unwrap();
        init(other.path());
        write(&dir, "a.txt", "two\n");
        commit_all(&dir, "second");
        run(dir.path(), GitCommand::Push { force: false }).unwrap();

        run(other.path(), GitCommand::Pull { rebase: true }).unwrap();

        assert_eq!(run(other.path(), GitCommand::Log { limit: 1 }).unwrap()[0]["subject"], "second");
    }

    #[test]
    fn force_pushing_replaces_a_rewritten_commit_and_a_plain_push_refuses_it() {
        let origin = tempfile::tempdir().unwrap();
        git(origin.path(), &["init", "--bare", "-b", "main"], &[]).unwrap();
        let dir = repository();
        git(dir.path(), &["remote", "add", "origin", origin.path().to_str().unwrap()], &[]).unwrap();
        write(&dir, "a.txt", "one\n");
        commit_all(&dir, "first");
        write(&dir, "a.txt", "two\n");
        commit_all(&dir, "second");
        run(dir.path(), GitCommand::Push { force: false }).unwrap();
        write(&dir, "a.txt", "three\n");
        run(dir.path(), GitCommand::StageAll).unwrap();
        run(dir.path(), GitCommand::Commit { message: "second, rewritten".into(), options: CommitOptions { amend: true, ..Default::default() } }).unwrap();

        assert!(run(dir.path(), GitCommand::Push { force: false }).is_err());

        run(dir.path(), GitCommand::Push { force: true }).unwrap();

        let pushed = String::from_utf8(git(origin.path(), &["log", "-1", "--format=%s", "main"], &[]).unwrap()).unwrap();
        assert_eq!(pushed.trim(), "second, rewritten");
    }

    #[test]
    fn pushing_without_a_remote_says_how_to_add_one() {
        let dir = repository();
        write(&dir, "a.txt", "one\n");
        commit_all(&dir, "first");

        let error = run(dir.path(), GitCommand::Push { force: false }).unwrap_err();

        assert!(error.contains("git remote add"), "{error}");
    }

    #[test]
    fn staging_and_unstaging_move_a_file_between_the_two_states() {
        let dir = repository();
        write(&dir, "a.txt", "one\n");
        commit_all(&dir, "first");
        write(&dir, "a.txt", "two\n");

        run(dir.path(), GitCommand::Stage { paths: vec!["a.txt".into()] }).unwrap();
        assert_eq!(file(&run(dir.path(), GitCommand::Status).unwrap(), "a.txt")["staged"], true);

        run(dir.path(), GitCommand::Unstage { paths: vec!["a.txt".into()] }).unwrap();
        let status = run(dir.path(), GitCommand::Status).unwrap();
        assert_eq!((file(&status, "a.txt")["staged"].clone(), file(&status, "a.txt")["unstaged"].clone()), (json!(false), json!(true)));
    }

    #[test]
    fn stage_all_stages_untracked_files_too() {
        let dir = repository();
        write(&dir, "a.txt", "one\n");

        run(dir.path(), GitCommand::StageAll).unwrap();

        let status = run(dir.path(), GitCommand::Status).unwrap();
        assert_eq!((file(&status, "a.txt")["staged"].clone(), file(&status, "a.txt")["untracked"].clone()), (json!(true), json!(false)));
    }

    #[test]
    fn a_commit_appears_in_the_log_and_undoing_it_leaves_its_changes_staged() {
        let dir = repository();
        write(&dir, "a.txt", "one\n");
        commit_all(&dir, "first");
        write(&dir, "a.txt", "two\n");
        commit_all(&dir, "Second change");

        let log = run(dir.path(), GitCommand::Log { limit: 10 }).unwrap();
        let commits = log.as_array().unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!((commits[0]["subject"].clone(), commits[0]["author"].clone()), (json!("Second change"), json!("Tester")));
        assert_eq!(commits[0]["hash"].as_str().unwrap().len(), 40);
        assert!(commits[0]["date"].as_str().unwrap().contains('T'));
        assert_eq!(run(dir.path(), GitCommand::Log { limit: 1 }).unwrap().as_array().unwrap().len(), 1);

        let undone = run(dir.path(), GitCommand::UndoLastCommit).unwrap();

        assert_eq!(undone["message"], "Second change");
        assert_eq!(run(dir.path(), GitCommand::Log { limit: 10 }).unwrap().as_array().unwrap().len(), 1);
        assert_eq!(file(&run(dir.path(), GitCommand::Status).unwrap(), "a.txt")["staged"], true);
    }

    #[test]
    fn committing_nothing_returns_gits_own_message() {
        let dir = repository();

        let error = run(dir.path(), GitCommand::Commit { message: "empty".into(), options: CommitOptions::default() }).unwrap_err();

        assert!(error.contains("nothing to commit"), "{error}");
    }

    #[test]
    fn amending_with_no_message_keeps_the_last_one_and_signoff_adds_the_trailer() {
        let dir = repository();
        write(&dir, "a.txt", "one\n");
        commit_all(&dir, "First change");
        write(&dir, "a.txt", "two\n");
        run(dir.path(), GitCommand::StageAll).unwrap();

        run(dir.path(), GitCommand::Commit { message: String::new(), options: CommitOptions { amend: true, signoff: true, no_verify: false } }).unwrap();

        let body = String::from_utf8(git(dir.path(), &["log", "--format=%B"], &[]).unwrap()).unwrap();
        assert_eq!(run(dir.path(), GitCommand::Log { limit: 10 }).unwrap().as_array().unwrap().len(), 1);
        assert!(body.starts_with("First change") && body.contains("Signed-off-by: Tester <tester@example.com>"), "{body}");
    }

    #[test]
    fn skipping_hooks_commits_past_a_failing_pre_commit_hook() {
        let dir = repository();
        let hook = dir.path().join(".git/hooks/pre-commit");
        std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        write(&dir, "a.txt", "one\n");
        run(dir.path(), GitCommand::StageAll).unwrap();

        assert!(run(dir.path(), GitCommand::Commit { message: "blocked".into(), options: CommitOptions::default() }).is_err());
        run(dir.path(), GitCommand::Commit { message: "past the hook".into(), options: CommitOptions { no_verify: true, ..Default::default() } }).unwrap();

        assert_eq!(run(dir.path(), GitCommand::Log { limit: 1 }).unwrap()[0]["subject"], "past the hook");
    }

    #[test]
    fn a_commit_message_is_never_read_as_an_option() {
        let dir = repository();
        write(&dir, "a.txt", "one\n");
        run(dir.path(), GitCommand::StageAll).unwrap();

        run(dir.path(), GitCommand::Commit { message: "--amend; touch pwned".into(), options: CommitOptions::default() }).unwrap();

        let log = run(dir.path(), GitCommand::Log { limit: 1 }).unwrap();
        assert_eq!(log[0]["subject"], "--amend; touch pwned");
        assert!(!dir.path().join("pwned").exists());
    }
}
