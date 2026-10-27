use super::git;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const GIT_TOWN: &str = "git-town";
const MOST_INFERRED_BRANCHES: usize = 50;

fn town_program(cwd: &Path, path: &std::ffi::OsStr) -> Option<(PathBuf, bool)> {
    if executable_on(path, GIT_TOWN) {
        return Some((PathBuf::from(GIT_TOWN), false));
    }

    let mise = std::env::split_paths(path).map(|directory| directory.join("mise")).find(|candidate| candidate.is_file())?;
    let available = Command::new(&mise).args(["which", GIT_TOWN]).current_dir(cwd).stdin(Stdio::null()).output().ok()?.status.success();

    available.then_some((mise, true))
}

pub(super) fn run_town(cwd: &Path, args: &[&str]) -> Result<Value, String> {
    let (program, mise) = town_program(cwd, &std::env::var_os("PATH").unwrap_or_default()).ok_or_else(|| "Git Town is not installed on this node or configured through mise in this folder.".to_string())?;
    let mut all = if mise { vec!["exec", "--", GIT_TOWN] } else { Vec::new() };
    all.extend_from_slice(args);

    town(cwd, &program, &all)
}

fn town(cwd: &Path, program: &Path, args: &[&str]) -> Result<Value, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|cause| format!("Git Town could not be started: {cause}."))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout.trim(), stderr.trim()).trim().to_string();

    if !output.status.success() {
        return Err(if combined.is_empty() { "Git Town failed without saying why.".into() } else { combined });
    }

    Ok(json!({"output": combined}))
}

struct LocalBranch {
    name: String,
    tip: String,
    upstream: String,
    track: String,
    subject: String,
}

pub(super) fn stack(cwd: &Path) -> Result<Value, String> {
    let current = String::from_utf8_lossy(&git(cwd, &["branch", "--show-current"], &[])?).trim().to_string();
    let listing = git(
        cwd,
        &["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)%00%(objectname)%00%(upstream:short)%00%(upstream:track)%00%(contents:subject)", "refs/heads"],
        &[],
    )?;
    let text = String::from_utf8_lossy(&listing);
    let locals: Vec<LocalBranch> = text
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\0');

            Some(LocalBranch { name: parts.next()?.into(), tip: parts.next()?.into(), upstream: parts.next()?.into(), track: parts.next()?.into(), subject: parts.next()?.into() })
        })
        .collect();
    let names: Vec<&str> = locals.iter().map(|branch| branch.name.as_str()).collect();
    let main = main_branch(cwd, &names);

    let mut lineage = recorded_lineage(cwd);
    lineage.retain(|child, parent| names.contains(&child.as_str()) && names.contains(&parent.as_str()));
    let recorded = lineage.contains_key(&current) || lineage.values().any(|parent| *parent == current);

    if !recorded {
        lineage = inferred_lineage(cwd, &locals, main.as_deref());
    }

    let mut chain = vec![current.clone()];

    while let Some(parent) = lineage.get(chain.last().unwrap_or(&current)) {
        if chain.contains(parent) {
            break;
        }

        chain.push(parent.clone());
    }

    chain.reverse();

    let mut ordered: Vec<(String, usize)> = chain.iter().cloned().zip(0..).collect();
    let children_of = |parent: &str, depth: usize| -> Vec<(String, usize)> {
        let mut children: Vec<String> = lineage.iter().filter(|(_, found)| found.as_str() == parent).map(|(child, _)| child.clone()).collect();
        children.sort_by(|left, right| right.cmp(left));

        children.into_iter().map(|child| (child, depth)).collect()
    };
    let mut pending = children_of(&current, chain.len());

    while let Some((name, depth)) = pending.pop() {
        if ordered.iter().any(|(seen, _)| *seen == name) {
            continue;
        }

        pending.extend(children_of(&name, depth + 1));
        ordered.push((name, depth));
    }

    let mut rows = Vec::new();

    for (name, depth) in &ordered {
        let Some(branch) = locals.iter().find(|branch| branch.name == *name) else {
            continue;
        };
        let parent = lineage.get(name);
        let (ahead, behind) = match parent {
            Some(parent) => (commit_count(cwd, &format!("{parent}..{name}")), commit_count(cwd, &format!("{name}..{parent}"))),
            None => (0, 0),
        };
        let has_upstream = !branch.upstream.is_empty();
        let pushed = has_upstream && !branch.track.contains("ahead") && !branch.track.contains("gone");

        rows.push(json!({
            "name": name,
            "parent": parent,
            "depth": depth,
            "current": *name == current,
            "main": main.as_deref() == Some(name.as_str()),
            "ahead": ahead,
            "behind": behind,
            "has_upstream": has_upstream,
            "pushed": pushed,
            "needs_sync": behind > 0,
            "subject": branch.subject,
        }));
    }

    let git_town_available = town_program(cwd, &std::env::var_os("PATH").unwrap_or_default()).is_some();

    Ok(json!({"current": current, "main": main, "inferred": !recorded, "git_town_available": git_town_available, "branches": rows}))
}

fn commit_count(cwd: &Path, range: &str) -> u64 {
    git(cwd, &["rev-list", "--count", range], &[]).ok().and_then(|output| String::from_utf8_lossy(&output).trim().parse().ok()).unwrap_or(0)
}

fn config_value(cwd: &Path, key: &str) -> Option<String> {
    let output = git(cwd, &["config", "--get", key], &[1]).ok()?;
    let value = String::from_utf8_lossy(&output).trim().to_string();

    (!value.is_empty()).then_some(value)
}

fn main_branch(cwd: &Path, names: &[&str]) -> Option<String> {
    let toml = git(cwd, &["rev-parse", "--show-toplevel"], &[]).ok().and_then(|top| std::fs::read_to_string(Path::new(String::from_utf8_lossy(&top).trim()).join(".git-town.toml")).ok());
    let from_file = toml.and_then(|text| {
        text.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;

            (key.trim() == "main-branch").then(|| value.trim().trim_matches(['"', '\'']).to_string())
        })
    });
    let from_origin = git(cwd, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"], &[1, 128]).ok().and_then(|found| String::from_utf8_lossy(&found).trim().strip_prefix("origin/").map(String::from));

    from_file
        .or_else(|| config_value(cwd, "git-town.main-branch"))
        .or_else(|| config_value(cwd, "git-town.main-branch-name"))
        .or(from_origin)
        .filter(|name| names.contains(&name.as_str()))
        .or_else(|| ["main", "master"].into_iter().find(|name| names.contains(name)).map(String::from))
}

fn recorded_lineage(cwd: &Path) -> HashMap<String, String> {
    let listing = git(cwd, &["config", "--get-regexp", r"^git-town-branch\..*\.parent$"], &[1]).unwrap_or_default();

    String::from_utf8_lossy(&listing)
        .lines()
        .filter_map(|line| {
            let (key, parent) = line.split_once(' ')?;
            let child = key.strip_prefix("git-town-branch.")?.strip_suffix(".parent")?;

            Some((child.to_string(), parent.trim().to_string()))
        })
        .collect()
}

fn inferred_lineage(cwd: &Path, locals: &[LocalBranch], main: Option<&str>) -> HashMap<String, String> {
    let mut lineage = HashMap::new();

    for branch in locals.iter().filter(|branch| Some(branch.name.as_str()) != main).take(MOST_INFERRED_BRANCHES) {
        let merged = git(cwd, &["for-each-ref", &format!("--merged={}", branch.name), "--format=%(refname:short)", "refs/heads"], &[]).unwrap_or_default();
        let closest = String::from_utf8_lossy(&merged)
            .lines()
            .filter(|candidate| *candidate != branch.name)
            .filter(|candidate| Some(*candidate) == main || locals.iter().any(|other| other.name == *candidate && other.tip != branch.tip))
            .min_by_key(|candidate| (commit_count(cwd, &format!("{candidate}..{}", branch.name)), Some(*candidate) != main, candidate.to_string()))
            .map(String::from);

        if let Some(parent) = closest {
            lineage.insert(branch.name.clone(), parent);
        }
    }

    lineage
}

fn executable_on(path: &std::ffi::OsStr, program: &str) -> bool {
    std::env::split_paths(path).any(|directory| directory.join(program).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::run;
    use crate::git::test_support::*;
    use mycelium_proto::link::GitCommand;

    fn commit_on(dir: &tempfile::TempDir, branch: &str, create: bool, file_name: &str) {
        run(dir.path(), GitCommand::Switch { branch: branch.into(), create, track: None }).unwrap();
        write(dir, file_name, "content\n");
        commit_all(dir, &format!("work in {file_name}"));
    }

    fn town_stack(dir: &tempfile::TempDir) -> Vec<(String, u64, u64, bool)> {
        let stack = run(dir.path(), GitCommand::Stack).unwrap();

        stack["branches"].as_array().unwrap().iter().map(|row| (row["name"].as_str().unwrap().to_string(), row["depth"].as_u64().unwrap(), row["ahead"].as_u64().unwrap(), row["needs_sync"].as_bool().unwrap())).collect()
    }

    fn stacked_repository() -> tempfile::TempDir {
        let dir = repository();
        write(&dir, "base.txt", "content\n");
        commit_all(&dir, "base");
        commit_on(&dir, "one", true, "one.txt");
        commit_on(&dir, "two", true, "two.txt");
        commit_on(&dir, "one", false, "one-more.txt");

        dir
    }

    #[test]
    fn the_stack_follows_the_parents_git_town_recorded_from_main_down_through_the_children() {
        let dir = stacked_repository();
        git(dir.path(), &["config", "git-town.main-branch", "main"], &[]).unwrap();
        git(dir.path(), &["config", "git-town-branch.one.parent", "main"], &[]).unwrap();
        git(dir.path(), &["config", "git-town-branch.two.parent", "one"], &[]).unwrap();

        let stack = run(dir.path(), GitCommand::Stack).unwrap();

        assert_eq!((stack["current"].clone(), stack["main"].clone(), stack["inferred"].clone()), (json!("one"), json!("main"), json!(false)));
        assert_eq!(town_stack(&dir), vec![("main".into(), 0, 0, false), ("one".into(), 1, 2, false), ("two".into(), 2, 1, true)]);
        assert_eq!(stack["branches"][1]["current"], true);
        assert_eq!(stack["branches"][2]["parent"], "one");
        assert_eq!(stack["branches"][2]["subject"], "work in two.txt");
    }

    #[test]
    fn a_branch_whose_parent_gained_commits_needs_syncing_and_the_old_main_branch_key_is_read() {
        let dir = repository();
        commit_on(&dir, "trunk", true, "base.txt");
        commit_on(&dir, "topic.v2/part", true, "topic.txt");
        commit_on(&dir, "trunk", false, "later.txt");
        git(dir.path(), &["config", "git-town.main-branch-name", "trunk"], &[]).unwrap();
        git(dir.path(), &["config", "git-town-branch.topic.v2/part.parent", "trunk"], &[]).unwrap();
        git(dir.path(), &["switch", "topic.v2/part"], &[]).unwrap();

        let stack = run(dir.path(), GitCommand::Stack).unwrap();

        assert_eq!((stack["main"].clone(), stack["inferred"].clone()), (json!("trunk"), json!(false)));
        assert_eq!(town_stack(&dir), vec![("trunk".into(), 0, 0, false), ("topic.v2/part".into(), 1, 1, true)]);
    }

    #[test]
    fn without_git_town_lineage_the_stack_is_inferred_from_which_branches_contain_which() {
        let dir = repository();
        write(&dir, "base.txt", "content\n");
        commit_all(&dir, "base");
        commit_on(&dir, "one", true, "one.txt");
        commit_on(&dir, "two", true, "two.txt");
        git(dir.path(), &["switch", "one"], &[]).unwrap();

        let stack = run(dir.path(), GitCommand::Stack).unwrap();

        assert_eq!(stack["inferred"], true);
        assert_eq!(town_stack(&dir), vec![("main".into(), 0, 0, false), ("one".into(), 1, 1, false), ("two".into(), 2, 1, false)]);
    }

    #[test]
    fn a_branch_is_pushed_only_when_its_upstream_has_everything() {
        let origin = tempfile::tempdir().unwrap();
        git(origin.path(), &["init", "--bare", "-b", "main"], &[]).unwrap();
        let dir = stacked_repository();
        git(dir.path(), &["config", "git-town-branch.one.parent", "main"], &[]).unwrap();
        git(dir.path(), &["config", "git-town-branch.two.parent", "one"], &[]).unwrap();
        git(dir.path(), &["remote", "add", "origin", origin.path().to_str().unwrap()], &[]).unwrap();
        git(dir.path(), &["push", "-u", "origin", "main"], &[]).unwrap();
        git(dir.path(), &["push", "-u", "origin", "one"], &[]).unwrap();
        commit_on(&dir, "one", false, "unpublished.txt");

        let stack = run(dir.path(), GitCommand::Stack).unwrap();
        let rows = stack["branches"].as_array().unwrap();
        let pushed: Vec<(&str, bool, bool)> = rows.iter().map(|row| (row["name"].as_str().unwrap(), row["has_upstream"].as_bool().unwrap(), row["pushed"].as_bool().unwrap())).collect();

        assert_eq!(pushed, vec![("main", true, true), ("one", true, false), ("two", false, false)]);
    }

    #[test]
    fn git_town_is_available_only_when_its_binary_is_on_the_path() {
        let empty = tempfile::tempdir().unwrap();
        let installed = tempfile::tempdir().unwrap();
        std::fs::write(installed.path().join(GIT_TOWN), "").unwrap();

        assert!(!executable_on(empty.path().as_os_str(), GIT_TOWN));
        assert!(executable_on(installed.path().as_os_str(), GIT_TOWN));
    }

    #[test]
    fn git_town_configured_through_mise_is_discovered_and_run_through_mise() {
        use std::os::unix::fs::PermissionsExt;

        let dir = repository();
        let tools = tempfile::tempdir().unwrap();
        let mise = tools.path().join("mise");
        std::fs::write(&mise, "#!/bin/sh\nif [ \"$1\" = which ]; then echo /managed/git-town; exit 0; fi\necho \"$@\"\n").unwrap();
        std::fs::set_permissions(&mise, std::fs::Permissions::from_mode(0o755)).unwrap();
        let found = town_program(dir.path(), tools.path().as_os_str()).unwrap();
        assert_eq!(found, (mise.clone(), true));
        let answer = town(dir.path(), &found.0, &["exec", "--", GIT_TOWN, "sync", "--stack"]).unwrap();
        assert_eq!(answer["output"], "exec -- git-town sync --stack");
    }

    #[test]
    fn git_towns_own_output_is_returned_and_shown_when_it_fails() {
        use std::os::unix::fs::PermissionsExt;

        let dir = repository();
        let script = |body: &str| {
            let path = dir.path().join(format!("fake-{}", body.len()));
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

            path
        };

        let worked = town(dir.path(), &script("echo synced \"$@\""), &["sync", "--stack"]).unwrap();
        let failed = town(dir.path(), &script("echo conflict in a.txt >&2; exit 1"), &["sync"]).unwrap_err();
        let missing = town(dir.path(), &dir.path().join("absent"), &[]).unwrap_err();

        assert_eq!(worked["output"], "synced sync --stack");
        assert_eq!(failed, "conflict in a.txt");
        assert!(missing.starts_with("Git Town could not be started"), "{missing}");
    }
}
