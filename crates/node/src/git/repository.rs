use super::git;
use super::git_with_env;
use base64::Engine;
use mycelium_proto::link::GitCredentials;
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn clone(target: &Path, url: &str, credentials: Option<&GitCredentials>) -> Result<Value, String> {
    let occupied = std::fs::read_dir(target).is_ok_and(|mut entries| entries.next().is_some()) || target.is_file();

    if occupied {
        return Err(format!("{} already exists and is not empty. Choose another folder.", target.display()));
    }

    let parent = target.parent().ok_or_else(|| format!("{} has no parent folder.", target.display()))?;
    std::fs::create_dir_all(parent).map_err(|cause| format!("Could not create {}: {cause}.", parent.display()))?;

    let env = credentials.map(|credentials| credential_env(url, credentials)).transpose()?.unwrap_or_default();
    let target_text = target.to_string_lossy();
    git_with_env(parent, &["clone", "--", url, &target_text], &[], &env)?;

    Ok(json!({"path": target.to_string_lossy()}))
}

pub(super) fn credential_env(url: &str, credentials: &GitCredentials) -> Result<Vec<(String, String)>, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| format!("{url} is not a valid address."))?;
    let origin = parsed.origin().ascii_serialization();
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", credentials.username, credentials.password));

    Ok(vec![
        ("GIT_CONFIG_COUNT".to_string(), "1".to_string()),
        ("GIT_CONFIG_KEY_0".to_string(), format!("http.{origin}/.extraheader")),
        ("GIT_CONFIG_VALUE_0".to_string(), format!("Authorization: Basic {encoded}")),
    ])
}

pub(super) fn register(folder: &Path) -> Result<Value, String> {
    if !folder.is_dir() {
        return Err(format!("The folder {} does not exist on this node.", folder.display()));
    }

    let canonical = folder.canonicalize().map_err(|cause| format!("Could not read {}: {cause}.", folder.display()))?;
    let repository = git(&canonical, &["rev-parse", "--git-dir"], &[]).is_ok();
    let configured = if repository { git(&canonical, &["config", "--get-regexp", r"^remote\..*\.url$"], &[1]).unwrap_or_default() } else { Vec::new() };
    let remotes: Vec<&str> = std::str::from_utf8(&configured).unwrap_or_default().lines().filter_map(|line| line.split_once(' ').map(|(_, url)| url.trim())).collect();

    Ok(json!({"path": canonical.to_string_lossy(), "repository": repository, "remotes": remotes}))
}

pub(super) fn add_worktree(project: &Path, branch: &str) -> Result<Value, String> {
    if branch.starts_with('-') || git(project, &["check-ref-format", "--branch", branch], &[]).is_err() {
        return Err(format!("{branch} is not a valid branch name. Use letters, numbers, dashes and slashes."));
    }

    let name = project.file_name().ok_or_else(|| format!("{} has no folder name.", project.display()))?.to_string_lossy();
    let folder = project.with_file_name(format!("{name}-worktrees")).join(branch.replace('/', "-"));

    if folder.exists() {
        return Err(format!("{} already exists. Choose another branch name.", folder.display()));
    }

    let folder_text = folder.to_string_lossy();
    git(project, &["worktree", "add", "-b", branch, &folder_text], &[])?;

    Ok(json!({"path": folder_text, "branch": branch}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{run, run_in};
    use crate::git::test_support::*;
    use mycelium_proto::link::{CommitOptions, GitCommand};

    fn one_request_server() -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 4096];
            let read = stream.read(&mut buffer).unwrap();
            stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();

            String::from_utf8_lossy(&buffer[..read]).into_owned()
        });

        (format!("http://{address}"), handle)
    }

    #[test]
    fn cloning_puts_the_repository_in_the_target_and_reports_its_path() {
        let origin = bare_origin_with_a_commit();
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("projects/org/app");

        let cloned = clone(&target, origin.path().to_str().unwrap(), None).unwrap();

        assert_eq!(cloned["path"], target.to_str().unwrap());
        assert_eq!(std::fs::read_to_string(target.join("a.txt")).unwrap(), "one\n");
    }

    #[test]
    fn cloning_into_a_folder_that_has_files_is_refused_and_leaves_them_alone() {
        let origin = bare_origin_with_a_commit();
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("mine.txt"), "keep").unwrap();

        let error = clone(root.path(), origin.path().to_str().unwrap(), None).unwrap_err();

        assert!(error.contains("already exists and is not empty"), "{error}");
        assert!(root.path().join("mine.txt").exists());
    }

    #[test]
    fn a_clone_address_that_is_not_http_is_refused_before_git_runs() {
        let root = tempfile::tempdir().unwrap();

        for url in ["ext::sh -c 'touch pwned'", "file:///etc", "--upload-pack=touch pwned", "git@github.com:org/app.git"] {
            let error = run_in(root.path(), "projects/app", GitCommand::Clone { url: url.into(), credentials: None }).unwrap_err();

            assert!(error.contains("will not clone it"), "{error}");
        }

        assert!(!root.path().join("projects").exists() && !root.path().join("pwned").exists());
    }

    #[test]
    fn credentials_reach_git_as_a_header_for_that_forge_only_and_never_in_the_error() {
        let (url, request) = one_request_server();
        let root = tempfile::tempdir().unwrap();
        let credentials = GitCredentials { username: "oauth2".into(), password: "s3cret-token".into() };

        let error = run_in(root.path(), "projects/org/app", GitCommand::Clone { url: format!("{url}/org/app.git"), credentials: Some(credentials) }).unwrap_err();

        let expected = base64::engine::general_purpose::STANDARD.encode("oauth2:s3cret-token");
        assert!(request.join().unwrap().contains(&format!("Basic {expected}")));
        assert!(!error.contains("s3cret-token") && !error.contains(&expected), "{error}");
        assert!(!root.path().join("projects/org/app").exists());
    }

    #[test]
    fn the_credential_header_is_scoped_to_the_forges_origin() {
        let env = credential_env("https://git.example.com:8443/org/app.git", &GitCredentials { username: "u".into(), password: "p".into() }).unwrap();

        assert!(env.contains(&("GIT_CONFIG_KEY_0".to_string(), "http.https://git.example.com:8443/.extraheader".to_string())));
    }

    #[test]
    fn the_credentials_debug_output_hides_the_password() {
        let shown = format!("{:?}", GitCommand::Clone { url: "https://x".into(), credentials: Some(GitCredentials { username: "u".into(), password: "s3cret-token".into() }) });

        assert!(!shown.contains("s3cret-token"), "{shown}");
    }

    #[test]
    fn registering_a_folder_reports_whether_it_is_a_repository() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("plain")).unwrap();
        std::fs::create_dir_all(root.path().join("repo")).unwrap();
        init(&root.path().join("repo"));
        git(&root.path().join("repo"), &["remote", "add", "origin", "git@example.com:a/b.git"], &[]).unwrap();
        std::fs::write(root.path().join("file.txt"), "x").unwrap();

        let plain = run_in(root.path(), "plain", GitCommand::Register).unwrap();
        let repo = run_in(root.path(), &root.path().join("repo").to_string_lossy(), GitCommand::Register).unwrap();

        assert_eq!(plain["repository"], false);
        assert_eq!(repo["repository"], true);
        assert_eq!(repo["remotes"], serde_json::json!(["git@example.com:a/b.git"]));
        assert_eq!(plain["remotes"], serde_json::json!([]));
        assert!(repo["path"].as_str().unwrap().ends_with("/repo"));
        assert!(run_in(root.path(), "missing", GitCommand::Register).unwrap_err().contains("does not exist"));
        assert!(run_in(root.path(), "file.txt", GitCommand::Register).unwrap_err().contains("does not exist"));
    }

    #[test]
    fn a_worktree_gets_a_new_branch_in_a_sibling_folder_inside_the_root() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("projects/org/app");
        std::fs::create_dir_all(&project).unwrap();
        init(&project);
        std::fs::write(project.join("a.txt"), "one\n").unwrap();
        run(&project, GitCommand::StageAll).unwrap();
        run(&project, GitCommand::Commit { message: "first".into(), options: CommitOptions::default() }).unwrap();

        let added = run_in(root.path(), project.to_str().unwrap(), GitCommand::AddWorktree { branch: "issue/12-fix".into() }).unwrap();

        let folder = root.path().join("projects/org/app-worktrees/issue-12-fix");
        assert_eq!((added["path"].as_str(), added["branch"].as_str()), (folder.to_str(), Some("issue/12-fix")));
        assert_eq!(std::fs::read_to_string(folder.join("a.txt")).unwrap(), "one\n");
        assert_eq!(run(&folder, GitCommand::Status).unwrap()["branch"], "issue/12-fix");
    }

    #[test]
    fn a_worktree_branch_that_exists_or_is_not_a_branch_name_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("app");
        std::fs::create_dir_all(&project).unwrap();
        init(&project);
        std::fs::write(project.join("a.txt"), "one\n").unwrap();
        run(&project, GitCommand::StageAll).unwrap();
        run(&project, GitCommand::Commit { message: "first".into(), options: CommitOptions::default() }).unwrap();
        run_in(root.path(), "app", GitCommand::AddWorktree { branch: "work".into() }).unwrap();

        let existing = run_in(root.path(), "app", GitCommand::AddWorktree { branch: "work".into() }).unwrap_err();
        assert!(existing.contains("already exists"), "{existing}");

        for branch in ["--detach", "a b", "x..y", ""] {
            let error = run_in(root.path(), "app", GitCommand::AddWorktree { branch: branch.into() }).unwrap_err();

            assert!(error.contains("not a valid branch name"), "{branch}: {error}");
        }
    }
}
