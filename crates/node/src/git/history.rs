use super::{MOST_DIFF_BYTES, MOST_MOVE_BYTES, git};
use super::moves::working_tree_changes;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;

pub(super) fn head(cwd: &Path) -> Result<Value, String> {
    let branch = git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"], &[]).ok().map(|output| String::from_utf8_lossy(&output).trim().to_string()).filter(|name| !name.is_empty() && name != "HEAD");

    Ok(json!({"branch": branch}))
}

pub(crate) fn status(cwd: &Path) -> Result<Value, String> {
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

    let prepared_message = prepared_message(cwd, "MERGE_MSG")?.or(prepared_message(cwd, "SQUASH_MSG")?);

    Ok(json!({"branch": branch, "upstream": upstream, "ahead": ahead, "behind": behind, "files": files, "prepared_message": prepared_message}))
}

fn prepared_message(cwd: &Path, name: &str) -> Result<Option<String>, String> {
    let path = String::from_utf8_lossy(&git(cwd, &["rev-parse", "--git-path", name], &[])?).trim().to_string();
    let Ok(text) = std::fs::read_to_string(cwd.join(path)) else {
        return Ok(None);
    };
    let message = text.lines().filter(|line| !line.starts_with('#')).collect::<Vec<_>>().join("\n").trim().to_string();

    Ok(Some(message).filter(|message| !message.is_empty()))
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

pub(super) fn log(cwd: &Path, limit: u32) -> Result<Value, String> {
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

pub(super) fn graph(cwd: &Path, limit: u32, skip: u32) -> Result<Value, String> {
    let limit = limit.to_string();
    let skip = skip.to_string();
    let output = match git(
        cwd,
        &["log", "--all", "--topo-order", "--decorate=full", "--format=%H%x00%P%x00%an%x00%aI%x00%D%x00%s", "-n", &limit, "--skip", &skip],
        &[],
    ) {
        Ok(output) => output,
        Err(message) if message.contains("does not have any commits") => Vec::new(),
        Err(message) => return Err(message),
    };

    let commits: Vec<Value> = String::from_utf8_lossy(&output)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\0');
            let hash = parts.next()?;
            let parents: Vec<&str> = parts.next()?.split_whitespace().collect();
            let author = parts.next()?;
            let date = parts.next()?;
            let refs: Vec<Value> = parts.next()?.split(", ").filter(|name| !name.is_empty()).filter_map(graph_ref).collect();

            Some(json!({"hash": hash, "parents": parents, "author": author, "date": date, "refs": refs, "subject": parts.next()?}))
        })
        .collect();

    Ok(json!(commits))
}

fn graph_ref(decoration: &str) -> Option<Value> {
    if decoration == "HEAD" {
        return Some(json!({"name": "HEAD", "kind": "head", "current": true}));
    }

    if let Some(branch) = decoration.strip_prefix("HEAD -> refs/heads/") {
        return Some(json!({"name": branch, "kind": "branch", "current": true}));
    }

    if let Some(tag) = decoration.strip_prefix("tag: refs/tags/") {
        return Some(json!({"name": tag, "kind": "tag", "current": false}));
    }

    if let Some(branch) = decoration.strip_prefix("refs/heads/") {
        return Some(json!({"name": branch, "kind": "branch", "current": false}));
    }

    let remote = decoration.strip_prefix("refs/remotes/")?;

    if remote.ends_with("/HEAD") {
        return None;
    }

    Some(json!({"name": remote, "kind": "remote", "current": false}))
}

pub(super) fn show(cwd: &Path, hash: &str) -> Result<Value, String> {
    if hash.len() < 4 || !hash.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(format!("{hash} is not a commit."));
    }

    let output = git(cwd, &["show", "--format=", "--no-color", "--patch", hash, "--"], &[])?;
    Ok(cut_diff(&output, MOST_DIFF_BYTES))
}

pub(super) fn branches(cwd: &Path) -> Result<Value, String> {
    let current = git(cwd, &["branch", "--show-current"], &[])?;
    let listing = git(
        cwd,
        &["for-each-ref", "--sort=-committerdate", "--format=%(refname)%00%(authorname)%00%(committerdate:iso-strict)%00%(contents:subject)", "refs/heads", "refs/remotes"],
        &[],
    )?;
    let text = String::from_utf8_lossy(&listing);
    let mut local = Vec::new();
    let mut remote = Vec::new();

    for line in text.lines() {
        let mut parts = line.split('\0');
        let (Some(refname), Some(author), Some(date), Some(subject)) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
            continue;
        };

        if let Some(name) = refname.strip_prefix("refs/heads/") {
            local.push(json!({"name": name, "author": author, "date": date, "subject": subject}));
        } else if let Some(name) = refname.strip_prefix("refs/remotes/").filter(|name| !name.ends_with("/HEAD")) {
            remote.push(json!({"name": name, "author": author, "date": date, "subject": subject}));
        }
    }

    Ok(json!({"current": String::from_utf8_lossy(&current).trim(), "local": local, "remote": remote}))
}

pub(super) fn switch(cwd: &Path, branch: &str, create: bool, track: Option<&str>) -> Result<Value, String> {
    git(cwd, &["check-ref-format", "--branch", branch], &[]).map_err(|_| format!("{branch} is not a valid branch name."))?;

    let output = match (create, track) {
        (true, Some(remote_branch)) => git(cwd, &["switch", "-c", branch, "--track", remote_branch], &[])?,
        (true, None) => git(cwd, &["switch", "-c", branch], &[])?,
        (false, _) => git(cwd, &["switch", branch], &[])?,
    };

    Ok(json!({"output": String::from_utf8_lossy(&output)}))
}

pub(super) fn diff(cwd: &Path, path: &str, staged: bool) -> Result<Value, String> {
    let untracked = !staged && !git(cwd, &["ls-files", "--others", "--exclude-standard", "--", path], &[])?.is_empty();
    let output = if untracked {
        git(cwd, &["diff", "--no-index", "--", "/dev/null", path], &[1])?
    } else if staged {
        git(cwd, &["diff", "--cached", "--", path], &[])?
    } else {
        git(cwd, &["diff", "--", path], &[])?
    };

    Ok(cut_diff(&output, MOST_DIFF_BYTES))
}

pub(super) fn scope_diff(cwd: &Path, scope: &str) -> Result<Value, String> {
    let output = match scope {
        "uncommitted" => working_tree_changes(cwd, &["--no-color"])?,
        "last_commit" => git(cwd, &["show", "--format=", "--no-color", "--no-renames", "--patch", "HEAD", "--"], &[])?,
        "branch" => {
            let head = String::from_utf8_lossy(&git(cwd, &["rev-parse", "HEAD"], &[])?).trim().to_string();
            let mut base = head.clone();

            for candidate in ["origin/main", "origin/master", "main", "master"] {
                let merged = git(cwd, &["merge-base", candidate, "HEAD"], &[1, 128]).map(|found| String::from_utf8_lossy(&found).trim().to_string()).unwrap_or_default();

                if !merged.is_empty() && merged != head {
                    base = merged;
                    break;
                }
            }

            git(cwd, &["diff", "--no-color", "--no-renames", "--no-ext-diff", &base, "HEAD", "--"], &[])?
        }
        other => return Err(format!("{other} is not a walkthrough scope. Use branch, uncommitted or last_commit.")),
    };
    Ok(cut_diff(&output, MOST_MOVE_BYTES))
}

fn cut_diff(output: &[u8], most: usize) -> Value {
    let truncated = output.len() > most;
    let kept = if truncated { &output[..most] } else { output };

    json!({"diff": String::from_utf8_lossy(kept), "truncated": truncated})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::run;
    use crate::git::test_support::*;
    use mycelium_proto::link::GitCommand;

    #[test]
    fn head_reports_the_checked_out_branch_and_null_outside_a_repository() {
        let dir = repository();
        write(&dir, "a.txt", "a\n");
        commit_all(&dir, "first");
        let plain = tempfile::tempdir().unwrap();

        assert_eq!(run(dir.path(), GitCommand::Head).unwrap()["branch"], "main");
        assert!(run(plain.path(), GitCommand::Head).unwrap()["branch"].is_null());
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
    fn status_carries_the_prepared_message_while_a_squash_or_merge_waits_to_be_committed() {
        let dir = repository();
        write(&dir, "a.txt", "one\n");
        commit_all(&dir, "first");
        git(dir.path(), &["switch", "-c", "feature"], &[]).unwrap();
        write(&dir, "a.txt", "two\n");
        commit_all(&dir, "feature work");
        git(dir.path(), &["switch", "main"], &[]).unwrap();

        assert_eq!(run(dir.path(), GitCommand::Status).unwrap()["prepared_message"], Value::Null);

        git(dir.path(), &["merge", "--squash", "feature"], &[]).unwrap();
        let squashed = run(dir.path(), GitCommand::Status).unwrap()["prepared_message"].as_str().unwrap().to_string();

        assert!(squashed.contains("feature work"), "{squashed}");

        git(dir.path(), &["reset", "--hard", "--quiet"], &[]).unwrap();
        std::fs::remove_file(dir.path().join(".git/SQUASH_MSG")).ok();
        write(&dir, "b.txt", "main side\n");
        commit_all(&dir, "main work");
        git(dir.path(), &["merge", "--no-ff", "--no-commit", "feature"], &[]).unwrap();
        let merged = run(dir.path(), GitCommand::Status).unwrap()["prepared_message"].as_str().unwrap().to_string();

        assert_eq!(merged, "Merge branch 'feature'");
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

    fn branchy_repository() -> tempfile::TempDir {
        let dir = repository();
        write(&dir, "a.txt", "one\n");
        commit_all(&dir, "first");
        git(dir.path(), &["switch", "-c", "feature"], &[]).unwrap();
        write(&dir, "b.txt", "feature\n");
        commit_all(&dir, "on feature");
        git(dir.path(), &["switch", "main"], &[]).unwrap();
        write(&dir, "c.txt", "main\n");
        commit_all(&dir, "on main");
        git(dir.path(), &["merge", "--no-ff", "feature", "-m", "merge feature"], &[]).unwrap();
        git(dir.path(), &["tag", "v1"], &[]).unwrap();

        dir
    }

    #[test]
    fn the_graph_lists_every_branch_with_parents_and_structured_refs() {
        let dir = branchy_repository();
        git(dir.path(), &["branch", "side", "HEAD~1"], &[]).unwrap();

        let graph = run(dir.path(), GitCommand::Graph { limit: 10, skip: 0 }).unwrap();
        let commits = graph.as_array().unwrap();
        let tip = &commits[0];

        assert_eq!(commits.len(), 4);
        assert_eq!(tip["subject"], "merge feature");
        assert_eq!(tip["parents"].as_array().unwrap().len(), 2);
        assert_eq!(tip["refs"], json!([{"name": "main", "kind": "branch", "current": true}, {"name": "v1", "kind": "tag", "current": false}]));
        assert!(commits.iter().any(|commit| commit["refs"] == json!([{"name": "feature", "kind": "branch", "current": false}])));
        assert!(commits.iter().any(|commit| commit["subject"] == "first" && commit["parents"] == json!([])));
    }

    #[test]
    fn the_graph_pages_with_skip_and_is_empty_without_commits() {
        let dir = branchy_repository();

        let page = run(dir.path(), GitCommand::Graph { limit: 2, skip: 2 }).unwrap();

        assert_eq!(page.as_array().unwrap().len(), 2);
        assert_eq!(run(repository().path(), GitCommand::Graph { limit: 10, skip: 0 }).unwrap(), json!([]));
    }

    #[test]
    fn showing_a_commit_returns_its_patch_and_refuses_anything_that_is_not_a_hash() {
        let dir = branchy_repository();
        let hash = run(dir.path(), GitCommand::Graph { limit: 10, skip: 0 }).unwrap()[1]["hash"].as_str().unwrap().to_string();

        let shown = run(dir.path(), GitCommand::Show { hash }).unwrap();

        assert!(shown["diff"].as_str().unwrap().contains("diff --git"));
        assert!(run(dir.path(), GitCommand::Show { hash: "--output=x".into() }).is_err());
    }

    fn switch_to(dir: &tempfile::TempDir, branch: &str, create: bool, track: Option<&str>) -> Result<Value, String> {
        run(dir.path(), GitCommand::Switch { branch: branch.into(), create, track: track.map(String::from) })
    }

    #[test]
    fn branches_list_local_and_remote_with_the_current_one_named() {
        let origin = bare_origin_with_a_commit();
        let clone = clone_of(&origin);
        git(clone.path(), &["branch", "local-only"], &[]).unwrap();

        let branches = run(clone.path(), GitCommand::Branches).unwrap();
        let names = |key: &str| branches[key].as_array().unwrap().iter().map(|branch| branch["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();

        assert_eq!(branches["current"], "main");
        assert!(names("local").contains(&"local-only".to_string()));
        assert_eq!(names("remote"), vec!["origin/main"]);
        assert_eq!(branches["local"][0]["subject"], "first");
    }

    #[test]
    fn switching_moves_to_an_existing_branch_and_creating_makes_a_new_one() {
        let dir = branchy_repository();

        switch_to(&dir, "feature", false, None).unwrap();
        assert_eq!(run(dir.path(), GitCommand::Branches).unwrap()["current"], "feature");

        switch_to(&dir, "fresh", true, None).unwrap();
        assert_eq!(run(dir.path(), GitCommand::Branches).unwrap()["current"], "fresh");
        assert!(switch_to(&dir, "bad name", true, None).is_err());
        assert!(switch_to(&dir, "-c", false, None).is_err());
    }

    #[test]
    fn a_remote_only_branch_is_created_tracking_its_remote() {
        let origin = bare_origin_with_a_commit();
        let clone = clone_of(&origin);
        git(clone.path(), &["push", "origin", "main:topic"], &[]).unwrap();
        git(clone.path(), &["fetch"], &[]).unwrap();

        switch_to(&clone, "topic", true, Some("origin/topic")).unwrap();

        assert_eq!(git(clone.path(), &["rev-parse", "--abbrev-ref", "topic@{upstream}"], &[]).unwrap(), b"origin/topic\n");
    }

    #[test]
    fn switching_with_changes_that_would_be_overwritten_is_refused_and_keeps_them() {
        let dir = branchy_repository();
        switch_to(&dir, "feature", false, None).unwrap();
        write(&dir, "c.txt", "feature version\n");
        commit_all(&dir, "diverge c");
        switch_to(&dir, "main", false, None).unwrap();
        write(&dir, "c.txt", "uncommitted\n");

        let refused = switch_to(&dir, "feature", false, None).unwrap_err();

        assert!(refused.contains("c.txt"), "{refused}");
        assert_eq!(run(dir.path(), GitCommand::Branches).unwrap()["current"], "main");
        assert_eq!(std::fs::read_to_string(dir.path().join("c.txt")).unwrap(), "uncommitted\n");
    }

    #[test]
    fn the_log_of_a_repository_without_commits_is_empty() {
        let dir = repository();

        assert_eq!(run(dir.path(), GitCommand::Log { limit: 10 }).unwrap(), json!([]));
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
    fn a_scope_diff_covers_the_working_tree_the_last_commit_or_the_branch_against_main() {
        let dir = repository();
        write(&dir, "a.txt", "one\n");
        commit_all(&dir, "base");
        git(dir.path(), &["switch", "-c", "topic"], &[]).unwrap();
        write(&dir, "a.txt", "one\ntwo\n");
        commit_all(&dir, "second");
        write(&dir, "b.txt", "fresh\n");
        let scoped = |scope: &str| run(dir.path(), GitCommand::ScopeDiff { scope: scope.into() }).unwrap()["diff"].as_str().unwrap().to_string();

        let uncommitted = scoped("uncommitted");
        let last = scoped("last_commit");
        let branch = scoped("branch");

        assert!(uncommitted.contains("+fresh") && !uncommitted.contains("+two"), "{uncommitted}");
        assert!(last.contains("+two") && !last.contains("+fresh"), "{last}");
        assert!(branch.contains("+two") && !branch.contains("+fresh"), "{branch}");
        assert!(run(dir.path(), GitCommand::ScopeDiff { scope: "everything".into() }).is_err());
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
