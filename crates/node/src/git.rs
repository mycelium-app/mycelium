use mycelium_proto::link::GitCommand;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

const MOST_DIFF_BYTES: usize = 200 * 1024;

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
    }
}

fn git(cwd: &Path, args: &[&str], accepted: &[i32]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
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
}
