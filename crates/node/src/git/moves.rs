use super::repository::credential_env;
use super::{MOST_MOVE_BYTES, git, git_with_env};
use base64::Engine;
use mycelium_proto::link::GitCredentials;
use serde_json::{Value, json};
use std::path::Path;
use uuid::Uuid;

fn branch_and_head(cwd: &Path) -> Result<(String, String), String> {
    let branch = String::from_utf8_lossy(&git(cwd, &["symbolic-ref", "--short", "-q", "HEAD"], &[1])?).trim().to_string();

    if branch.is_empty() {
        return Err("This folder is not on a branch. Check one out on this node, then try again.".to_string());
    }

    let head = git(cwd, &["rev-parse", "HEAD"], &[]).map_err(|_| "This folder has no commits yet. Commit once on this node, then try again.".to_string())?;

    Ok((branch, String::from_utf8_lossy(&head).trim().to_string()))
}

pub(super) fn working_tree_changes(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
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

pub(super) fn move_plan(cwd: &Path) -> Result<Value, String> {
    let (branch, head) = branch_and_head(cwd)?;
    let names = working_tree_changes(cwd, &["--name-only", "-z"])?;
    let changed_files = names.split(|byte| *byte == 0).filter(|name| !name.is_empty()).count();

    Ok(json!({"branch": branch, "head": head, "changed_files": changed_files}))
}

pub(super) fn snapshot(cwd: &Path) -> Result<Value, String> {
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

pub(super) fn apply(cwd: &Path, branch: &str, head: &str, patch: &str, bundle: Option<&str>, credentials: Option<&GitCredentials>) -> Result<Value, String> {
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

pub(super) fn undo(cwd: &Path, branch: &str, previous: &str, branch_before: Option<&str>) -> Result<Value, String> {
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
    use crate::git::run;
    use crate::git::test_support::*;
    use mycelium_proto::link::GitCommand;

    fn moved(snapshot: &Value) -> GitCommand {
        GitCommand::Apply {
            branch: snapshot["branch"].as_str().unwrap().into(),
            head: snapshot["head"].as_str().unwrap().into(),
            patch: snapshot["patch"].as_str().unwrap().into(),
            bundle: snapshot["bundle"].as_str().map(str::to_string),
            credentials: None,
        }
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
}
