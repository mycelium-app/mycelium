use base64::Engine;
use mycelium_proto::link::{GitCommand, GitCredentials};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use uuid::Uuid;

const MOST_DIFF_BYTES: usize = 200 * 1024;
const MOST_MOVE_BYTES: usize = 8 * 1024 * 1024;

pub fn run_in(root: &Path, cwd: &str, command: GitCommand) -> Result<Value, String> {
    match command {
        GitCommand::Clone { url, credentials } => {
            if !url.starts_with("https://") && !url.starts_with("http://") {
                return Err(format!("{url} is not an http or https address, so this node will not clone it."));
            }

            clone(&within(root, cwd)?, &url, credentials.as_ref())
        }
        GitCommand::Register => register(&within(root, cwd)?),
        GitCommand::AddWorktree { branch } => add_worktree(&within(root, cwd)?, &branch),
        other => run(Path::new(cwd), other),
    }
}

pub fn run(cwd: &Path, command: GitCommand) -> Result<Value, String> {
    match command {
        GitCommand::Status => status(cwd),
        GitCommand::Log { limit } => log(cwd, limit),
        GitCommand::Diff { path, staged } => diff(cwd, &path, staged),
        GitCommand::Stage { paths } => changed(cwd, &["add", "--"], &paths),
        GitCommand::Unstage { paths } => changed(cwd, &["restore", "--staged", "--"], &paths),
        GitCommand::StageAll => changed(cwd, &["add", "-A"], &[]),
        GitCommand::Commit { message } => changed(cwd, &["commit", "-m", &message], &[]),
        GitCommand::Fetch => changed(cwd, &["fetch"], &[]),
        GitCommand::UndoLastCommit => changed(cwd, &["reset", "--soft", "HEAD~1"], &[]),
        GitCommand::MovePlan => move_plan(cwd),
        GitCommand::Snapshot { .. } => snapshot(cwd),
        GitCommand::Apply { branch, head, patch, bundle, credentials } => apply(cwd, &branch, &head, &patch, bundle.as_deref(), credentials.as_ref()),
        GitCommand::Undo { branch, previous, branch_before } => undo(cwd, &branch, &previous, branch_before.as_deref()),
        GitCommand::Clone { .. } | GitCommand::Register | GitCommand::AddWorktree { .. } => Err("This needs the node's projects folder, not a conversation's folder.".to_string()),
    }
}

fn git(cwd: &Path, args: &[&str], accepted: &[i32]) -> Result<Vec<u8>, String> {
    git_with_env(cwd, args, accepted, &[])
}

fn git_with_env(cwd: &Path, args: &[&str], accepted: &[i32], env: &[(String, String)]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .envs(env.iter().map(|(name, value)| (name, value)))
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|cause| format!("Git could not be started: {cause}."))?;

    if output.status.success() || output.status.code().is_some_and(|code| accepted.contains(&code)) {
        return Ok(output.stdout);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

    Err(if stderr.is_empty() { stdout } else { stderr })
}

fn within(root: &Path, requested: &str) -> Result<PathBuf, String> {
    let joined = root.join(requested);
    let outside = || format!("{} is outside {}. Choose a folder inside it.", joined.display(), root.display());

    if joined.components().any(|component| component == Component::ParentDir) {
        return Err(outside());
    }

    let inside_root = joined.starts_with(root) || root.canonicalize().is_ok_and(|canonical| joined.starts_with(canonical));

    if !inside_root || joined == root {
        return Err(outside());
    }

    Ok(joined)
}

fn clone(target: &Path, url: &str, credentials: Option<&GitCredentials>) -> Result<Value, String> {
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

fn credential_env(url: &str, credentials: &GitCredentials) -> Result<Vec<(String, String)>, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| format!("{url} is not a valid address."))?;
    let origin = parsed.origin().ascii_serialization();
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", credentials.username, credentials.password));

    Ok(vec![
        ("GIT_CONFIG_COUNT".to_string(), "1".to_string()),
        ("GIT_CONFIG_KEY_0".to_string(), format!("http.{origin}/.extraheader")),
        ("GIT_CONFIG_VALUE_0".to_string(), format!("Authorization: Basic {encoded}")),
    ])
}

fn register(folder: &Path) -> Result<Value, String> {
    if !folder.is_dir() {
        return Err(format!("The folder {} does not exist on this node.", folder.display()));
    }

    let canonical = folder.canonicalize().map_err(|cause| format!("Could not read {}: {cause}.", folder.display()))?;
    let repository = git(&canonical, &["rev-parse", "--git-dir"], &[]).is_ok();

    Ok(json!({"path": canonical.to_string_lossy(), "repository": repository}))
}

fn add_worktree(project: &Path, branch: &str) -> Result<Value, String> {
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

fn changed(cwd: &Path, args: &[&str], paths: &[String]) -> Result<Value, String> {
    let mut all = args.to_vec();
    all.extend(paths.iter().map(String::as_str));
    git(cwd, &all, &[])?;

    Ok(json!({}))
}

fn status(cwd: &Path) -> Result<Value, String> {
    let porcelain = git(cwd, &["status", "--porcelain=v2", "--branch", "--untracked-files=all", "-z"], &[])?;
    let unstaged = counts(&git(cwd, &["diff", "--numstat", "-z"], &[])?);
    let staged = counts(&git(cwd, &["diff", "--cached", "--numstat", "-z"], &[])?);

    let mut branch = String::new();
    let mut upstream = Value::Null;
    let (mut ahead, mut behind) = (0, 0);
    let mut files = Vec::new();
    let mut records = porcelain.split(|byte| *byte == 0).filter(|record| !record.is_empty());

    while let Some(record) = records.next() {
        let line = String::from_utf8_lossy(record);

        if let Some(head) = line.strip_prefix("# branch.head ") {
            branch = head.to_string();
        } else if let Some(name) = line.strip_prefix("# branch.upstream ") {
            upstream = json!(name);
        } else if let Some(counts) = line.strip_prefix("# branch.ab ") {
            let mut parts = counts.split(' ');
            ahead = parts.next().and_then(|part| part.trim_start_matches('+').parse().ok()).unwrap_or(0);
            behind = parts.next().and_then(|part| part.trim_start_matches('-').parse().ok()).unwrap_or(0);
        } else if let Some(path) = line.strip_prefix("? ") {
            files.push(json!({"path": path, "staged": false, "unstaged": false, "untracked": true, "additions": 0, "deletions": 0}));
        } else if line.starts_with("1 ") || line.starts_with("2 ") || line.starts_with("u ") {
            let renamed = line.starts_with("2 ");
            let fields: Vec<&str> = line.splitn(if renamed { 10 } else if line.starts_with("u ") { 11 } else { 9 }, ' ').collect();
            let (Some(code), Some(path)) = (fields.get(1), fields.last()) else {
                continue;
            };

            if renamed {
                records.next();
            }

            let unmerged = line.starts_with("u ");
            let mut letters = code.chars();
            let index = letters.next().unwrap_or('.');
            let worktree = letters.next().unwrap_or('.');
            let (added_staged, deleted_staged) = staged.get(*path).copied().unwrap_or((0, 0));
            let (added_unstaged, deleted_unstaged) = unstaged.get(*path).copied().unwrap_or((0, 0));

            files.push(json!({
                "path": path,
                "staged": !unmerged && index != '.',
                "unstaged": unmerged || worktree != '.',
                "untracked": false,
                "additions": added_staged + added_unstaged,
                "deletions": deleted_staged + deleted_unstaged,
            }));
        }
    }

    Ok(json!({"branch": branch, "upstream": upstream, "ahead": ahead, "behind": behind, "files": files}))
}

fn counts(numstat: &[u8]) -> HashMap<String, (u64, u64)> {
    let mut found = HashMap::new();
    let mut records = numstat.split(|byte| *byte == 0).filter(|record| !record.is_empty());

    while let Some(record) = records.next() {
        let line = String::from_utf8_lossy(record);
        let mut parts = line.splitn(3, '\t');
        let added = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
        let deleted = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
        let path = parts.next().unwrap_or("").to_string();

        let path = if path.is_empty() {
            records.next();

            records.next().map(|renamed| String::from_utf8_lossy(renamed).into_owned()).unwrap_or_default()
        } else {
            path
        };

        let total: &mut (u64, u64) = found.entry(path).or_default();
        total.0 += added;
        total.1 += deleted;
    }

    found
}

fn log(cwd: &Path, limit: u32) -> Result<Value, String> {
    let limit = limit.to_string();
    let output = match git(cwd, &["log", "--format=%H%x1f%an%x1f%aI%x1f%s", "-n", &limit], &[]) {
        Ok(output) => output,
        Err(message) if message.contains("does not have any commits") => Vec::new(),
        Err(message) => return Err(message),
    };

    let commits: Vec<Value> = String::from_utf8_lossy(&output)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\x1f');

            Some(json!({"hash": parts.next()?, "author": parts.next()?, "date": parts.next()?, "subject": parts.next()?}))
        })
        .collect();

    Ok(json!(commits))
}

fn diff(cwd: &Path, path: &str, staged: bool) -> Result<Value, String> {
    let untracked = !staged && !git(cwd, &["ls-files", "--others", "--exclude-standard", "--", path], &[])?.is_empty();
    let output = if untracked {
        git(cwd, &["diff", "--no-index", "--", "/dev/null", path], &[1])?
    } else if staged {
        git(cwd, &["diff", "--cached", "--", path], &[])?
    } else {
        git(cwd, &["diff", "--", path], &[])?
    };

    let truncated = output.len() > MOST_DIFF_BYTES;
    let kept = if truncated { &output[..MOST_DIFF_BYTES] } else { &output[..] };

    Ok(json!({"diff": String::from_utf8_lossy(kept), "truncated": truncated}))
}

fn branch_and_head(cwd: &Path) -> Result<(String, String), String> {
    let branch = String::from_utf8_lossy(&git(cwd, &["symbolic-ref", "--short", "-q", "HEAD"], &[1])?).trim().to_string();

    if branch.is_empty() {
        return Err("This folder is not on a branch. Check one out on this node, then try again.".to_string());
    }

    let head = git(cwd, &["rev-parse", "HEAD"], &[]).map_err(|_| "This folder has no commits yet. Commit once on this node, then try again.".to_string())?;

    Ok((branch, String::from_utf8_lossy(&head).trim().to_string()))
}

fn working_tree_changes(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let real_index = String::from_utf8_lossy(&git(cwd, &["rev-parse", "--path-format=absolute", "--git-path", "index"], &[])?).trim().to_string();
    let scratch = std::env::temp_dir().join(format!("mycelium-index-{}", Uuid::new_v4()));
    let env = [("GIT_INDEX_FILE".to_string(), scratch.to_string_lossy().into_owned())];

    let prepared = if Path::new(&real_index).exists() {
        std::fs::copy(&real_index, &scratch).map(|_| Vec::new()).map_err(|cause| format!("Could not read the folder's index: {cause}."))
    } else {
        git_with_env(cwd, &["read-tree", "HEAD"], &[], &env)
    };

    let mut diff = vec!["diff", "--cached", "--no-renames", "--no-ext-diff", "--no-textconv"];
    diff.extend_from_slice(args);
    diff.push("HEAD");
    let changes = prepared.and_then(|_| git_with_env(cwd, &["add", "-A"], &[], &env)).and_then(|_| git_with_env(cwd, &diff, &[], &env));
    let _ = std::fs::remove_file(&scratch);

    changes
}

fn move_plan(cwd: &Path) -> Result<Value, String> {
    let (branch, head) = branch_and_head(cwd)?;
    let names = working_tree_changes(cwd, &["--name-only", "-z"])?;
    let changed_files = names.split(|byte| *byte == 0).filter(|name| !name.is_empty()).count();

    Ok(json!({"branch": branch, "head": head, "changed_files": changed_files}))
}

fn snapshot(cwd: &Path) -> Result<Value, String> {
    let (branch, head) = branch_and_head(cwd)?;
    let patch = working_tree_changes(cwd, &["--binary", "--full-index"])?;
    let changed_files = patch.split(|byte| *byte == b'\n').filter(|line| line.starts_with(b"diff --git ")).count();
    let branch_ref = format!("refs/heads/{branch}");
    let unpushed = String::from_utf8_lossy(&git(cwd, &["rev-list", "--count", &branch_ref, "--not", "--remotes"], &[])?).trim().to_string();

    let bundle = if unpushed == "0" {
        None
    } else {
        let scratch = std::env::temp_dir().join(format!("mycelium-bundle-{}", Uuid::new_v4()));
        let created = git(cwd, &["bundle", "create", &scratch.to_string_lossy(), &branch_ref, "--not", "--remotes"], &[]).and_then(|_| std::fs::read(&scratch).map_err(|cause| format!("Could not read the bundle of unpushed commits: {cause}.")));
        let _ = std::fs::remove_file(&scratch);

        Some(created?)
    };

    let size = patch.len() + bundle.as_ref().map_or(0, Vec::len);

    if size > MOST_MOVE_BYTES {
        return Err(format!("The unsaved work is {} MB, which is more than a move carries ({} MB). Commit and push it to the forge, then move again.", size.div_ceil(1024 * 1024), MOST_MOVE_BYTES / (1024 * 1024)));
    }

    let engine = base64::engine::general_purpose::STANDARD;

    Ok(json!({"branch": branch, "head": head, "changed_files": changed_files, "patch": engine.encode(&patch), "bundle": bundle.map(|bytes| engine.encode(bytes))}))
}

fn apply(cwd: &Path, branch: &str, head: &str, patch: &str, bundle: Option<&str>, credentials: Option<&GitCredentials>) -> Result<Value, String> {
    let engine = base64::engine::general_purpose::STANDARD;
    let branch_ref = format!("refs/heads/{branch}");

    if branch.starts_with('-') || git(cwd, &["check-ref-format", &branch_ref], &[]).is_err() {
        return Err(format!("{branch} is not a valid branch name."));
    }

    if head.len() < 40 || !head.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(format!("{head} is not a commit."));
    }

    if !git(cwd, &["status", "--porcelain", "--untracked-files=all"], &[])?.is_empty() {
        return Err("The project folder on this node has uncommitted changes. Commit or discard them there, then try again.".to_string());
    }

    let has_head = || git(cwd, &["cat-file", "-e", &format!("{head}^{{commit}}")], &[]).is_ok();

    if !has_head() {
        let origin = String::from_utf8_lossy(&git(cwd, &["remote", "get-url", "origin"], &[]).unwrap_or_default()).trim().to_string();
        let env = credentials.and_then(|credentials| credential_env(&origin, credentials).ok()).unwrap_or_default();
        let _ = git_with_env(cwd, &["fetch", "--quiet"], &[], &env);
    }

    if let (false, Some(bundle)) = (has_head(), bundle) {
        let bytes = engine.decode(bundle).map_err(|_| "The snapshot of unpushed commits was damaged in transit.".to_string())?;
        let scratch = std::env::temp_dir().join(format!("mycelium-bundle-{}", Uuid::new_v4()));
        std::fs::write(&scratch, bytes).map_err(|cause| format!("Could not write the bundle of unpushed commits: {cause}."))?;
        let fetched = git(cwd, &["fetch", "--quiet", &scratch.to_string_lossy(), &branch_ref], &[]);
        let _ = std::fs::remove_file(&scratch);
        fetched?;
    }

    if !has_head() {
        return Err(format!("This node does not have commit {} and could not fetch it. Push the branch to the forge from the other node, then try again.", &head[..12.min(head.len())]));
    }

    let existing = git(cwd, &["rev-parse", "--verify", "-q", &branch_ref], &[1]).is_ok_and(|output| !output.is_empty());

    let only_here = String::from_utf8_lossy(&git(cwd, &["rev-list", "--count", &format!("{head}..{branch_ref}")], &[]).unwrap_or_default()).trim().to_string();

    if existing && only_here != "0" {
        return Err(format!("{branch} on this node has commits the other node does not. Rename or push that branch there, then try again."));
    }

    let previous = String::from_utf8_lossy(&git(cwd, &["symbolic-ref", "--short", "-q", "HEAD"], &[1])?).trim().to_string();
    let previous = if previous.is_empty() { String::from_utf8_lossy(&git(cwd, &["rev-parse", "HEAD"], &[]).unwrap_or_default()).trim().to_string() } else { previous };
    let branch_before = if existing { Some(String::from_utf8_lossy(&git(cwd, &["rev-parse", &branch_ref], &[])?).trim().to_string()) } else { None };
    git(cwd, &["checkout", "--quiet", "-B", branch, head], &[])?;

    let bytes = engine.decode(patch).map_err(|_| "The snapshot of unsaved work was damaged in transit.".to_string())?;
    let applied_files = bytes.split(|byte| *byte == b'\n').filter(|line| line.starts_with(b"diff --git ")).count();

    if !bytes.is_empty() {
        let scratch = std::env::temp_dir().join(format!("mycelium-patch-{}", Uuid::new_v4()));
        std::fs::write(&scratch, &bytes).map_err(|cause| format!("Could not write the unsaved work: {cause}."))?;
        let applied = git(cwd, &["apply", "--binary", "--whitespace=nowarn", &scratch.to_string_lossy()], &[]);
        let _ = std::fs::remove_file(&scratch);

        if let Err(message) = applied {
            if !previous.is_empty() {
                let _ = git(cwd, &["checkout", "--quiet", &previous], &[]);
            }

            if !existing {
                let _ = git(cwd, &["branch", "-D", branch], &[]);
            }

            return Err(format!("The unsaved work did not apply on this node: {message}"));
        }
    }

    Ok(json!({"branch": branch, "head": head, "applied_files": applied_files, "previous": previous, "branch_before": branch_before}))
}

fn undo(cwd: &Path, branch: &str, previous: &str, branch_before: Option<&str>) -> Result<Value, String> {
    if previous.is_empty() || previous.starts_with('-') || branch.starts_with('-') || branch_before.is_some_and(|sha| sha.starts_with('-')) {
        return Err("There is nothing to go back to: the branch the folder was on is unknown.".to_string());
    }

    git(cwd, &["reset", "--hard", "--quiet"], &[])?;
    git(cwd, &["clean", "-fdq"], &[])?;

    match branch_before {
        Some(sha) if previous == branch => {
            git(cwd, &["checkout", "--quiet", "-B", branch, sha], &[])?;
        }
        Some(sha) => {
            git(cwd, &["checkout", "--quiet", previous], &[])?;
            git(cwd, &["branch", "-f", branch, sha], &[])?;
        }
        None => {
            git(cwd, &["checkout", "--quiet", previous], &[])?;
            git(cwd, &["branch", "-D", branch], &[])?;
        }
    }

    Ok(json!({"branch": previous}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path());

        dir
    }

    fn init(path: &Path) {
        for args in [&["init", "-b", "main"][..], &["config", "user.name", "Tester"], &["config", "user.email", "tester@example.com"], &["config", "commit.gpgsign", "false"]] {
            git(path, args, &[]).unwrap();
        }
    }

    fn write(dir: &tempfile::TempDir, path: &str, text: &str) {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }

    fn commit_all(dir: &tempfile::TempDir, message: &str) {
        run(dir.path(), GitCommand::StageAll).unwrap();
        run(dir.path(), GitCommand::Commit { message: message.into() }).unwrap();
    }

    fn file<'a>(status: &'a Value, path: &str) -> &'a Value {
        status["files"].as_array().unwrap().iter().find(|file| file["path"] == path).unwrap_or_else(|| panic!("{path} missing from {status}"))
    }

    #[test]
    fn status_separates_staged_unstaged_and_untracked_files_with_line_counts() {
        let dir = repository();
        write(&dir, "a.txt", "one\ntwo\n");
        write(&dir, "b.txt", "keep\n");
        commit_all(&dir, "first");
        write(&dir, "a.txt", "one\nthree\nfour\n");
        write(&dir, "b.txt", "changed\n");
        write(&dir, "src/new.txt", "new\n");
        run(dir.path(), GitCommand::Stage { paths: vec!["b.txt".into()] }).unwrap();

        let status = run(dir.path(), GitCommand::Status).unwrap();

        assert_eq!(status["branch"], "main");
        assert_eq!(status["upstream"], Value::Null);
        assert_eq!(file(&status, "a.txt"), &json!({"path": "a.txt", "staged": false, "unstaged": true, "untracked": false, "additions": 2, "deletions": 1}));
        assert_eq!(file(&status, "b.txt"), &json!({"path": "b.txt", "staged": true, "unstaged": false, "untracked": false, "additions": 1, "deletions": 1}));
        assert_eq!(file(&status, "src/new.txt")["untracked"], true);
    }

    #[test]
    fn status_reports_how_far_the_branch_is_from_its_upstream() {
        let origin = tempfile::tempdir().unwrap();
        git(origin.path(), &["init", "--bare", "-b", "main"], &[]).unwrap();
        let dir = repository();
        git(dir.path(), &["remote", "add", "origin", origin.path().to_str().unwrap()], &[]).unwrap();
        write(&dir, "a.txt", "one\n");
        commit_all(&dir, "first");
        git(dir.path(), &["push", "-u", "origin", "main"], &[]).unwrap();
        write(&dir, "a.txt", "two\n");
        commit_all(&dir, "second");

        let status = run(dir.path(), GitCommand::Status).unwrap();

        assert_eq!((status["upstream"].as_str(), status["ahead"].as_u64(), status["behind"].as_u64()), (Some("origin/main"), Some(1), Some(0)));
        assert!(run(dir.path(), GitCommand::Fetch).is_ok());
    }

    #[test]
    fn a_renamed_file_is_listed_once_under_its_new_name() {
        let dir = repository();
        write(&dir, "old.txt", "some content here\nmore\n");
        commit_all(&dir, "first");
        git(dir.path(), &["mv", "old.txt", "new.txt"], &[]).unwrap();

        let status = run(dir.path(), GitCommand::Status).unwrap();

        assert_eq!(status["files"].as_array().unwrap().len(), 1);
        assert_eq!(file(&status, "new.txt")["staged"], true);
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

        run(dir.path(), GitCommand::UndoLastCommit).unwrap();

        assert_eq!(run(dir.path(), GitCommand::Log { limit: 10 }).unwrap().as_array().unwrap().len(), 1);
        assert_eq!(file(&run(dir.path(), GitCommand::Status).unwrap(), "a.txt")["staged"], true);
    }

    #[test]
    fn the_log_of_a_repository_without_commits_is_empty() {
        let dir = repository();

        assert_eq!(run(dir.path(), GitCommand::Log { limit: 10 }).unwrap(), json!([]));
    }

    #[test]
    fn committing_nothing_returns_gits_own_message() {
        let dir = repository();

        let error = run(dir.path(), GitCommand::Commit { message: "empty".into() }).unwrap_err();

        assert!(error.contains("nothing to commit"), "{error}");
    }

    #[test]
    fn a_commit_message_is_never_read_as_an_option() {
        let dir = repository();
        write(&dir, "a.txt", "one\n");
        run(dir.path(), GitCommand::StageAll).unwrap();

        run(dir.path(), GitCommand::Commit { message: "--amend; touch pwned".into() }).unwrap();

        let log = run(dir.path(), GitCommand::Log { limit: 1 }).unwrap();
        assert_eq!(log[0]["subject"], "--amend; touch pwned");
        assert!(!dir.path().join("pwned").exists());
    }

    #[test]
    fn diff_shows_unstaged_staged_and_untracked_changes() {
        let dir = repository();
        write(&dir, "a.txt", "one\n");
        commit_all(&dir, "first");
        write(&dir, "a.txt", "two\n");
        write(&dir, "fresh.txt", "brand new\n");

        let unstaged = run(dir.path(), GitCommand::Diff { path: "a.txt".into(), staged: false }).unwrap();
        assert!(unstaged["diff"].as_str().unwrap().contains("+two"));
        assert_eq!(unstaged["truncated"], false);

        let nothing_staged = run(dir.path(), GitCommand::Diff { path: "a.txt".into(), staged: true }).unwrap();
        assert_eq!(nothing_staged["diff"], "");

        run(dir.path(), GitCommand::Stage { paths: vec!["a.txt".into()] }).unwrap();
        let staged = run(dir.path(), GitCommand::Diff { path: "a.txt".into(), staged: true }).unwrap();
        assert!(staged["diff"].as_str().unwrap().contains("-one"));

        let untracked = run(dir.path(), GitCommand::Diff { path: "fresh.txt".into(), staged: false }).unwrap();
        assert!(untracked["diff"].as_str().unwrap().contains("+brand new"));
    }

    #[test]
    fn a_very_large_diff_is_cut_and_flagged() {
        let dir = repository();
        write(&dir, "big.txt", &"line of text\n".repeat(30_000));

        let diff = run(dir.path(), GitCommand::Diff { path: "big.txt".into(), staged: false }).unwrap();

        assert_eq!(diff["truncated"], true);
        assert!(diff["diff"].as_str().unwrap().len() <= MOST_DIFF_BYTES + 3);
    }

    #[test]
    fn a_folder_that_is_not_a_repository_returns_gits_message() {
        let dir = tempfile::tempdir().unwrap();

        let error = run(dir.path(), GitCommand::Status).unwrap_err();

        assert!(error.contains("not a git repository"), "{error}");
    }

    fn moved(snapshot: &Value) -> GitCommand {
        GitCommand::Apply {
            branch: snapshot["branch"].as_str().unwrap().into(),
            head: snapshot["head"].as_str().unwrap().into(),
            patch: snapshot["patch"].as_str().unwrap().into(),
            bundle: snapshot["bundle"].as_str().map(str::to_string),
            credentials: None,
        }
    }

    fn clone_of(origin: &tempfile::TempDir) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["clone", "--", origin.path().to_str().unwrap(), "."], &[]).unwrap();
        for args in [&["config", "user.name", "Tester"][..], &["config", "user.email", "tester@example.com"], &["config", "commit.gpgsign", "false"]] {
            git(dir.path(), args, &[]).unwrap();
        }

        dir
    }

    #[test]
    fn a_snapshot_applied_on_the_same_base_recreates_modified_deleted_and_untracked_files() {
        let origin = bare_origin_with_a_commit();
        let source = clone_of(&origin);
        let target = clone_of(&origin);
        write(&source, "b.txt", "keep\n");
        commit_all(&source, "second");
        git(source.path(), &["push", "origin", "HEAD"], &[]).unwrap();
        write(&source, "a.txt", "changed\n");
        std::fs::remove_file(source.path().join("b.txt")).unwrap();
        write(&source, "deep/new.txt", "untracked\n");
        std::fs::write(source.path().join("blob.bin"), [0u8, 159, 146, 150, 0, 1]).unwrap();

        let snapshot = run(source.path(), GitCommand::Snapshot { session_id: None }).unwrap();
        let applied = run(target.path(), moved(&snapshot)).unwrap();

        assert_eq!((snapshot["branch"].as_str(), snapshot["changed_files"].as_u64(), applied["applied_files"].as_u64()), (Some("main"), Some(4), Some(4)));
        assert_eq!(std::fs::read_to_string(target.path().join("a.txt")).unwrap(), "changed\n");
        assert!(!target.path().join("b.txt").exists());
        assert_eq!(std::fs::read_to_string(target.path().join("deep/new.txt")).unwrap(), "untracked\n");
        assert_eq!(std::fs::read(target.path().join("blob.bin")).unwrap(), [0u8, 159, 146, 150, 0, 1]);
    }

    #[test]
    fn taking_a_snapshot_leaves_the_source_folder_and_its_staging_alone() {
        let dir = repository();
        write(&dir, "a.txt", "one\n");
        commit_all(&dir, "first");
        write(&dir, "a.txt", "two\n");
        write(&dir, "fresh.txt", "new\n");
        run(dir.path(), GitCommand::Stage { paths: vec!["a.txt".into()] }).unwrap();
        let before = run(dir.path(), GitCommand::Status).unwrap();

        run(dir.path(), GitCommand::Snapshot { session_id: None }).unwrap();

        assert_eq!(run(dir.path(), GitCommand::Status).unwrap(), before);
        assert_eq!(std::fs::read_to_string(dir.path().join("fresh.txt")).unwrap(), "new\n");
    }

    #[test]
    fn unpushed_commits_travel_in_a_bundle_and_the_branch_lands_on_the_same_commit() {
        let origin = bare_origin_with_a_commit();
        let source = clone_of(&origin);
        let target = clone_of(&origin);
        git(source.path(), &["checkout", "-b", "feature/moves"], &[]).unwrap();
        write(&source, "local.txt", "only here\n");
        commit_all(&source, "unpushed");
        write(&source, "a.txt", "dirty\n");

        let snapshot = run(source.path(), GitCommand::Snapshot { session_id: None }).unwrap();
        run(target.path(), moved(&snapshot)).unwrap();

        assert!(snapshot["bundle"].is_string());
        assert_eq!(run(target.path(), GitCommand::Status).unwrap()["branch"], "feature/moves");
        assert_eq!(git(target.path(), &["rev-parse", "HEAD"], &[]).unwrap(), git(source.path(), &["rev-parse", "HEAD"], &[]).unwrap());
        assert_eq!(std::fs::read_to_string(target.path().join("local.txt")).unwrap(), "only here\n");
        assert_eq!(std::fs::read_to_string(target.path().join("a.txt")).unwrap(), "dirty\n");
    }

    #[test]
    fn pushed_work_needs_no_bundle_and_the_target_fetches_the_branch_from_its_remote() {
        let origin = bare_origin_with_a_commit();
        let source = clone_of(&origin);
        let target = clone_of(&origin);
        write(&source, "a.txt", "pushed\n");
        commit_all(&source, "pushed");
        git(source.path(), &["push", "origin", "HEAD"], &[]).unwrap();

        let snapshot = run(source.path(), GitCommand::Snapshot { session_id: None }).unwrap();
        run(target.path(), moved(&snapshot)).unwrap();

        assert!(snapshot["bundle"].is_null());
        assert_eq!(std::fs::read_to_string(target.path().join("a.txt")).unwrap(), "pushed\n");
    }

    #[test]
    fn applying_onto_a_folder_with_uncommitted_changes_is_refused_and_touches_nothing() {
        let origin = bare_origin_with_a_commit();
        let source = clone_of(&origin);
        let target = clone_of(&origin);
        write(&source, "a.txt", "moved\n");
        write(&target, "a.txt", "mine\n");

        let snapshot = run(source.path(), GitCommand::Snapshot { session_id: None }).unwrap();
        let error = run(target.path(), moved(&snapshot)).unwrap_err();

        assert!(error.contains("uncommitted changes"), "{error}");
        assert_eq!(std::fs::read_to_string(target.path().join("a.txt")).unwrap(), "mine\n");
    }

    #[test]
    fn a_patch_that_does_not_apply_puts_the_target_back_on_its_branch() {
        let origin = bare_origin_with_a_commit();
        let target = clone_of(&origin);
        let head = String::from_utf8_lossy(&git(target.path(), &["rev-parse", "HEAD"], &[]).unwrap()).trim().to_string();
        let patch = base64::engine::general_purpose::STANDARD.encode("diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-not the line\n+replacement\n");

        let error = run(target.path(), GitCommand::Apply { branch: "feature/new".into(), head, patch, bundle: None, credentials: None }).unwrap_err();

        assert!(error.contains("did not apply"), "{error}");
        assert_eq!(run(target.path(), GitCommand::Status).unwrap()["branch"], "main");
        assert!(git(target.path(), &["rev-parse", "--verify", "-q", "refs/heads/feature/new"], &[1]).unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(target.path().join("a.txt")).unwrap(), "one\n");
    }

    fn moved_from_unpushed_work(source: &tempfile::TempDir, target: &tempfile::TempDir, branch: &str) -> Value {
        git(source.path(), &["checkout", "--quiet", "-B", branch], &[]).unwrap();
        write(source, "b.txt", "committed here\n");
        commit_all(source, "unpushed");
        write(source, "a.txt", "changed\n");
        write(source, "deep/new.txt", "untracked\n");
        let snapshot = run(source.path(), GitCommand::Snapshot { session_id: None }).unwrap();

        run(target.path(), moved(&snapshot)).unwrap()
    }

    fn undo_of(applied: &Value, branch: &str) -> GitCommand {
        GitCommand::Undo { branch: branch.into(), previous: applied["previous"].as_str().unwrap().into(), branch_before: applied["branch_before"].as_str().map(str::to_string) }
    }

    #[test]
    fn undoing_a_move_onto_a_new_branch_leaves_the_folder_as_it_was() {
        let origin = bare_origin_with_a_commit();
        let source = clone_of(&origin);
        let target = clone_of(&origin);
        let head_before = git(target.path(), &["rev-parse", "HEAD"], &[]).unwrap();

        let applied = moved_from_unpushed_work(&source, &target, "feature/moves");
        run(target.path(), undo_of(&applied, "feature/moves")).unwrap();

        assert!(git(target.path(), &["status", "--porcelain", "--untracked-files=all"], &[]).unwrap().is_empty());
        assert_eq!(run(target.path(), GitCommand::Status).unwrap()["branch"], "main");
        assert_eq!(git(target.path(), &["rev-parse", "HEAD"], &[]).unwrap(), head_before);
        assert!(git(target.path(), &["rev-parse", "--verify", "-q", "refs/heads/feature/moves"], &[1]).unwrap().is_empty());
        assert!(!target.path().join("deep").exists() && !target.path().join("b.txt").exists());
        assert_eq!(std::fs::read_to_string(target.path().join("a.txt")).unwrap(), "one\n");
    }

    #[test]
    fn undoing_a_move_onto_the_branch_the_folder_was_on_puts_that_branch_back_where_it_was() {
        let origin = bare_origin_with_a_commit();
        let source = clone_of(&origin);
        let target = clone_of(&origin);
        let head_before = git(target.path(), &["rev-parse", "HEAD"], &[]).unwrap();

        let applied = moved_from_unpushed_work(&source, &target, "main");
        assert_ne!(git(target.path(), &["rev-parse", "HEAD"], &[]).unwrap(), head_before);
        run(target.path(), undo_of(&applied, "main")).unwrap();

        assert!(git(target.path(), &["status", "--porcelain", "--untracked-files=all"], &[]).unwrap().is_empty());
        assert_eq!(run(target.path(), GitCommand::Status).unwrap()["branch"], "main");
        assert_eq!(git(target.path(), &["rev-parse", "HEAD"], &[]).unwrap(), head_before);
        assert!(!target.path().join("deep").exists() && !target.path().join("b.txt").exists());
    }

    #[test]
    fn undoing_a_move_keeps_an_existing_branch_at_its_old_tip_and_returns_to_the_branch_before() {
        let origin = bare_origin_with_a_commit();
        let source = clone_of(&origin);
        let target = clone_of(&origin);
        git(target.path(), &["branch", "feature/moves"], &[]).unwrap();
        let tip_before = git(target.path(), &["rev-parse", "feature/moves"], &[]).unwrap();

        let applied = moved_from_unpushed_work(&source, &target, "feature/moves");
        run(target.path(), undo_of(&applied, "feature/moves")).unwrap();

        assert!(git(target.path(), &["status", "--porcelain", "--untracked-files=all"], &[]).unwrap().is_empty());
        assert_eq!(run(target.path(), GitCommand::Status).unwrap()["branch"], "main");
        assert_eq!(git(target.path(), &["rev-parse", "feature/moves"], &[]).unwrap(), tip_before);
    }

    #[test]
    fn a_branch_with_commits_the_snapshot_lacks_is_not_overwritten() {
        let origin = bare_origin_with_a_commit();
        let source = clone_of(&origin);
        let target = clone_of(&origin);
        write(&target, "theirs.txt", "target only\n");
        commit_all(&target, "target work");

        let snapshot = run(source.path(), GitCommand::Snapshot { session_id: None }).unwrap();
        let error = run(target.path(), moved(&snapshot)).unwrap_err();

        assert!(error.contains("has commits the other node does not"), "{error}");
        assert!(target.path().join("theirs.txt").exists());
    }

    #[test]
    fn a_commit_the_target_cannot_get_is_named_in_the_refusal() {
        let origin = bare_origin_with_a_commit();
        let target = clone_of(&origin);
        let head = "0123456789abcdef0123456789abcdef01234567".to_string();

        let error = run(target.path(), GitCommand::Apply { branch: "main".into(), head, patch: String::new(), bundle: None, credentials: None }).unwrap_err();

        assert!(error.contains("does not have commit 0123456789ab"), "{error}");
    }

    #[test]
    fn work_over_the_limit_is_refused_with_the_way_to_move_it() {
        let dir = repository();
        write(&dir, "a.txt", "one\n");
        commit_all(&dir, "first");
        write(&dir, "big.txt", &"line of text\n".repeat(700_000));

        let error = run(dir.path(), GitCommand::Snapshot { session_id: None }).unwrap_err();

        assert!(error.contains("Commit and push it to the forge"), "{error}");
    }

    #[test]
    fn the_plan_counts_changed_and_untracked_files_and_needs_a_branch() {
        let dir = repository();
        write(&dir, "a.txt", "one\n");
        commit_all(&dir, "first");
        write(&dir, "a.txt", "two\n");
        write(&dir, "src/new.txt", "new\n");

        let plan = run(dir.path(), GitCommand::MovePlan).unwrap();
        git(dir.path(), &["checkout", "--detach"], &[]).unwrap();
        let detached = run(dir.path(), GitCommand::MovePlan).unwrap_err();

        assert_eq!((plan["branch"].as_str(), plan["changed_files"].as_u64()), (Some("main"), Some(2)));
        assert!(detached.contains("not on a branch"), "{detached}");
    }

    fn bare_origin_with_a_commit() -> tempfile::TempDir {
        let work = repository();
        write(&work, "a.txt", "one\n");
        commit_all(&work, "first");
        let origin = tempfile::tempdir().unwrap();
        git(origin.path(), &["clone", "--bare", "--", work.path().to_str().unwrap(), "."], &[]).unwrap();

        origin
    }

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
    fn a_folder_outside_the_projects_root_or_climbing_out_of_it_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();

        for requested in ["../escape", "projects/../../escape", elsewhere.path().to_str().unwrap(), ""] {
            let error = run_in(root.path(), requested, GitCommand::Register).unwrap_err();

            assert!(error.contains("outside"), "{requested}: {error}");
        }
    }

    #[test]
    fn registering_a_folder_reports_whether_it_is_a_repository() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("plain")).unwrap();
        std::fs::create_dir_all(root.path().join("repo")).unwrap();
        init(&root.path().join("repo"));
        std::fs::write(root.path().join("file.txt"), "x").unwrap();

        let plain = run_in(root.path(), "plain", GitCommand::Register).unwrap();
        let repo = run_in(root.path(), &root.path().join("repo").to_string_lossy(), GitCommand::Register).unwrap();

        assert_eq!(plain["repository"], false);
        assert_eq!(repo["repository"], true);
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
        run(&project, GitCommand::Commit { message: "first".into() }).unwrap();

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
        run(&project, GitCommand::Commit { message: "first".into() }).unwrap();
        run_in(root.path(), "app", GitCommand::AddWorktree { branch: "work".into() }).unwrap();

        let existing = run_in(root.path(), "app", GitCommand::AddWorktree { branch: "work".into() }).unwrap_err();
        assert!(existing.contains("already exists"), "{existing}");

        for branch in ["--detach", "a b", "x..y", ""] {
            let error = run_in(root.path(), "app", GitCommand::AddWorktree { branch: branch.into() }).unwrap_err();

            assert!(error.contains("not a valid branch name"), "{branch}: {error}");
        }
    }
}
