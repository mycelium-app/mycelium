use crate::changes;
use base64::Engine;
use mycelium_proto::link::{CommitOptions, GitCommand, GitCredentials};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use uuid::Uuid;

const MOST_DIFF_BYTES: usize = 200 * 1024;
const MOST_MOVE_BYTES: usize = 8 * 1024 * 1024;
const MOST_READ_FILE_BYTES: u64 = 20 * 1024 * 1024;
const MOST_LISTED_DIRECTORIES: usize = 500;
const GIT_TOWN: &str = "git-town";
const MOST_INFERRED_BRANCHES: usize = 50;

pub fn run_in(root: &Path, cwd: &str, command: GitCommand) -> Result<Value, String> {
    match command {
        GitCommand::Clone { url, credentials } => {
            if !url.starts_with("https://") && !url.starts_with("http://") {
                return Err(format!("{url} is not an http or https address, so this node will not clone it."));
            }

            clone(&within(root, cwd)?, &url, credentials.as_ref())
        }
        GitCommand::Register => register(&within(root, cwd)?),
        GitCommand::Directories => directories(root, cwd),
        GitCommand::AddWorktree { branch } => add_worktree(&within(root, cwd)?, &branch),
        other => run(Path::new(cwd), other),
    }
}

pub fn run(cwd: &Path, command: GitCommand) -> Result<Value, String> {
    match command {
        GitCommand::Status => status(cwd),
        GitCommand::Log { limit } => log(cwd, limit),
        GitCommand::Graph { limit, skip } => graph(cwd, limit, skip),
        GitCommand::Show { hash } => show(cwd, &hash),
        GitCommand::Branches => branches(cwd),
        GitCommand::Stack => stack(cwd),
        GitCommand::Switch { branch, create, track } => switch(cwd, &branch, create, track.as_deref()),
        GitCommand::Diff { path, staged } => diff(cwd, &path, staged),
        GitCommand::ScopeDiff { scope } => scope_diff(cwd, &scope),
        GitCommand::ChangesLayout => changes::layout(cwd),
        GitCommand::ChangesRows { ranges } => changes::rows(cwd, &ranges),
        GitCommand::Stage { paths } => changed(cwd, &["add", "--"], &paths),
        GitCommand::Unstage { paths } => changed(cwd, &["restore", "--staged", "--"], &paths),
        GitCommand::Hunk { path, row, fingerprint, action } => changes::apply_hunk(cwd, &path, row, &fingerprint, &action),
        GitCommand::File { path, content, expected } => edit_file(cwd, &path, content.as_deref(), expected.as_deref()),
        GitCommand::Remote { action, force, url, allowed_urls, credentials } => remote(cwd, &action, force, &url, &allowed_urls, &credentials),
        GitCommand::StageAll => changed(cwd, &["add", "-A"], &[]),
        GitCommand::Commit { message, options } => commit(cwd, &message, &options),
        GitCommand::Fetch => changed(cwd, &["fetch"], &[]),
        GitCommand::Pull { rebase } => changed(cwd, if rebase { &["pull", "--rebase"] } else { &["pull"] }, &[]),
        GitCommand::Push { force } => push(cwd, force),
        GitCommand::UndoLastCommit => undo_last_commit(cwd),
        GitCommand::TownSync => run_town(cwd, &["sync", "--stack"]),
        GitCommand::TownPropose => run_town(cwd, &["propose"]),
        GitCommand::MovePlan => move_plan(cwd),
        GitCommand::Snapshot { .. } => snapshot(cwd),
        GitCommand::Apply { branch, head, patch, bundle, credentials } => apply(cwd, &branch, &head, &patch, bundle.as_deref(), credentials.as_ref()),
        GitCommand::Undo { branch, previous, branch_before } => undo(cwd, &branch, &previous, branch_before.as_deref()),
        GitCommand::ReadFile { path } => read_file(cwd, &path),
        GitCommand::FindLogo => Ok(crate::logo::find(cwd)),
        GitCommand::Head => head(cwd),
        GitCommand::Clone { .. } | GitCommand::Register | GitCommand::Directories | GitCommand::AddWorktree { .. } => Err("This needs the node's projects folder, not a conversation's folder.".to_string()),
    }
}

pub(crate) fn git(cwd: &Path, args: &[&str], accepted: &[i32]) -> Result<Vec<u8>, String> {
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

fn directories(root: &Path, requested: &str) -> Result<Value, String> {
    let canonical_root = root.canonicalize().map_err(|cause| format!("The projects folder {} cannot be read: {cause}.", root.display()))?;
    let wanted = if requested.is_empty() { canonical_root.clone() } else { canonical_root.join(requested) };
    let path = wanted.canonicalize().map_err(|_| format!("{} does not exist on this node. Choose a folder inside {}.", wanted.display(), canonical_root.display()))?;

    if !path.starts_with(&canonical_root) {
        return Err(format!("{} is outside {}. Choose a folder inside it.", path.display(), canonical_root.display()));
    }

    let entries = std::fs::read_dir(&path).map_err(|cause| format!("{} cannot be read: {cause}.", path.display()))?;
    let mut folders = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let listed = !name.starts_with('.') && !crate::files::SKIPPED_DIRECTORIES.contains(&name.as_str());

            listed.then(|| (name, entry.path()))
        })
        .collect::<Vec<_>>();
    folders.sort_by_key(|(name, _)| name.to_lowercase());

    let truncated = folders.len() > MOST_LISTED_DIRECTORIES;
    folders.truncate(MOST_LISTED_DIRECTORIES);

    let directories = folders
        .into_iter()
        .map(|(name, folder)| json!({"name": name, "path": folder.to_string_lossy(), "repository": folder.join(".git").exists()}))
        .collect::<Vec<_>>();
    let parent = if path == canonical_root { None } else { path.parent().map(|parent| parent.to_string_lossy().into_owned()) };

    Ok(json!({"root": canonical_root.to_string_lossy(), "path": path.to_string_lossy(), "parent": parent, "directories": directories, "truncated": truncated}))
}

fn head(cwd: &Path) -> Result<Value, String> {
    let branch = git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"], &[]).ok().map(|output| String::from_utf8_lossy(&output).trim().to_string()).filter(|name| !name.is_empty() && name != "HEAD");

    Ok(json!({"branch": branch}))
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
    let configured = if repository { git(&canonical, &["config", "--get-regexp", r"^remote\..*\.url$"], &[1]).unwrap_or_default() } else { Vec::new() };
    let remotes: Vec<&str> = std::str::from_utf8(&configured).unwrap_or_default().lines().filter_map(|line| line.split_once(' ').map(|(_, url)| url.trim())).collect();

    Ok(json!({"path": canonical.to_string_lossy(), "repository": repository, "remotes": remotes}))
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

fn commit(cwd: &Path, message: &str, options: &CommitOptions) -> Result<Value, String> {
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

fn push(cwd: &Path, force: bool) -> Result<Value, String> {
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

fn undo_last_commit(cwd: &Path) -> Result<Value, String> {
    let message = String::from_utf8_lossy(&git(cwd, &["log", "-1", "--format=%B"], &[])?).trim_end().to_string();
    git(cwd, &["reset", "--soft", "HEAD~1"], &[])?;

    Ok(json!({"message": message}))
}

fn town_program(cwd: &Path, path: &std::ffi::OsStr) -> Option<(PathBuf, bool)> {
    if executable_on(path, GIT_TOWN) {
        return Some((PathBuf::from(GIT_TOWN), false));
    }

    let mise = std::env::split_paths(path).map(|directory| directory.join("mise")).find(|candidate| candidate.is_file())?;
    let available = Command::new(&mise).args(["which", GIT_TOWN]).current_dir(cwd).stdin(Stdio::null()).output().ok()?.status.success();

    available.then_some((mise, true))
}

fn run_town(cwd: &Path, args: &[&str]) -> Result<Value, String> {
    let (program, mise) = town_program(cwd, &std::env::var_os("PATH").unwrap_or_default()).ok_or_else(|| "Git Town is not installed on this node or configured through mise in this folder.".to_string())?;
    let mut all = if mise { vec!["exec", "--", GIT_TOWN] } else { Vec::new() };
    all.extend_from_slice(args);

    town(cwd, &program, &all)
}

fn remote(cwd: &Path, action: &str, force: bool, url: &str, allowed_urls: &[String], credentials: &GitCredentials) -> Result<Value, String> {
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

fn graph(cwd: &Path, limit: u32, skip: u32) -> Result<Value, String> {
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

fn show(cwd: &Path, hash: &str) -> Result<Value, String> {
    if hash.len() < 4 || !hash.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(format!("{hash} is not a commit."));
    }

    let output = git(cwd, &["show", "--format=", "--no-color", "--patch", hash, "--"], &[])?;
    let truncated = output.len() > MOST_DIFF_BYTES;
    let kept = if truncated { &output[..MOST_DIFF_BYTES] } else { &output[..] };

    Ok(json!({"diff": String::from_utf8_lossy(kept), "truncated": truncated}))
}

fn branches(cwd: &Path) -> Result<Value, String> {
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

struct LocalBranch {
    name: String,
    tip: String,
    upstream: String,
    track: String,
    subject: String,
}

fn stack(cwd: &Path) -> Result<Value, String> {
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

fn switch(cwd: &Path, branch: &str, create: bool, track: Option<&str>) -> Result<Value, String> {
    git(cwd, &["check-ref-format", "--branch", branch], &[]).map_err(|_| format!("{branch} is not a valid branch name."))?;

    let output = match (create, track) {
        (true, Some(remote_branch)) => git(cwd, &["switch", "-c", branch, "--track", remote_branch], &[])?,
        (true, None) => git(cwd, &["switch", "-c", branch], &[])?,
        (false, _) => git(cwd, &["switch", branch], &[])?,
    };

    Ok(json!({"output": String::from_utf8_lossy(&output)}))
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

fn read_file(cwd: &Path, path: &str) -> Result<Value, String> {
    let root = cwd.canonicalize().map_err(|cause| format!("The conversation's folder could not be read: {cause}."))?;
    let outside = || format!("{path} is outside the conversation's folder. Give a path inside it.");
    let file = root.join(path).canonicalize().map_err(|_| format!("{path} was not found in the conversation's folder."))?;

    if !file.starts_with(&root) {
        return Err(outside());
    }

    let size = std::fs::metadata(&file).map_err(|cause| format!("{path} could not be read: {cause}."))?.len();

    if !file.is_file() {
        return Err(format!("{path} is not a file."));
    }

    if size > MOST_READ_FILE_BYTES {
        return Err(format!("{path} is larger than 20 MB. Use a smaller file."));
    }

    let bytes = std::fs::read(&file).map_err(|cause| format!("{path} could not be read: {cause}."))?;

    Ok(json!({"data": base64::engine::general_purpose::STANDARD.encode(bytes), "size": size}))
}

fn edit_file(cwd: &Path, path: &str, content: Option<&str>, expected: Option<&str>) -> Result<Value, String> {
    let root = cwd.canonicalize().map_err(|cause| cause.to_string())?;
    let file = root.join(path).canonicalize().map_err(|_| format!("{path} was not found."))?;

    if !file.starts_with(&root) || file.components().any(|part| part.as_os_str() == ".git") {
        return Err("Choose a file inside the conversation's folder, outside .git.".into());
    }

    let bytes = std::fs::read(&file).map_err(|cause| cause.to_string())?;

    if bytes.len() > MOST_DIFF_BYTES || bytes.contains(&0) {
        return Err("Only text files up to 200 KB can be edited here.".into());
    }

    let current = String::from_utf8(bytes).map_err(|_| "This file is not UTF-8 text.".to_string())?;

    if let Some(content) = content {
        if expected != Some(current.as_str()) {
            return Err("The file changed on disk. Reopen the editor before saving.".into());
        }

        if content.len() > MOST_DIFF_BYTES {
            return Err("Only text files up to 200 KB can be edited here.".into());
        }

        std::fs::write(&file, content).map_err(|cause| cause.to_string())?;

        return Ok(json!({"content": content}));
    }

    Ok(json!({"content": current}))
}

fn scope_diff(cwd: &Path, scope: &str) -> Result<Value, String> {
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
    let truncated = output.len() > MOST_MOVE_BYTES;
    let kept = if truncated { &output[..MOST_MOVE_BYTES] } else { &output[..] };

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
        run(dir.path(), GitCommand::Commit { message: message.into(), options: CommitOptions::default() }).unwrap();
    }

    fn file<'a>(status: &'a Value, path: &str) -> &'a Value {
        status["files"].as_array().unwrap().iter().find(|file| file["path"] == path).unwrap_or_else(|| panic!("{path} missing from {status}"))
    }

    #[test]
    fn directories_lists_the_folders_under_the_projects_root_and_marks_git_checkouts() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("Zed")).unwrap();
        std::fs::create_dir_all(root.path().join("alpha")).unwrap();
        init(&root.path().join("alpha"));
        std::fs::write(root.path().join("file.txt"), "x").unwrap();

        let listing = run_in(root.path(), "", GitCommand::Directories).unwrap();
        let names: Vec<_> = listing["directories"].as_array().unwrap().iter().map(|entry| (entry["name"].as_str().unwrap(), entry["repository"].as_bool().unwrap())).collect();

        assert_eq!(names, vec![("alpha", true), ("Zed", false)]);
        assert_eq!(listing["truncated"], false);
    }

    #[test]
    fn directories_leaves_out_hidden_and_build_folders() {
        let root = tempfile::tempdir().unwrap();

        for name in [".cache", "node_modules", "target", "app"] {
            std::fs::create_dir_all(root.path().join(name)).unwrap();
        }

        let listing = run_in(root.path(), "", GitCommand::Directories).unwrap();
        let names: Vec<_> = listing["directories"].as_array().unwrap().iter().map(|entry| entry["name"].as_str().unwrap()).collect();

        assert_eq!(names, vec!["app"]);
    }

    #[test]
    fn directories_refuses_a_folder_outside_the_projects_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();

        let error = run_in(root.path(), &outside.path().to_string_lossy(), GitCommand::Directories).unwrap_err();

        assert!(error.contains("Choose a folder inside it."), "{error}");
    }

    #[test]
    fn directories_at_the_root_has_no_parent_and_a_child_points_back_to_it() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("app")).unwrap();

        let at_root = run_in(root.path(), "", GitCommand::Directories).unwrap();
        let inside = run_in(root.path(), "app", GitCommand::Directories).unwrap();

        assert!(at_root["parent"].is_null());
        assert_eq!(inside["parent"], at_root["path"]);
    }

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
    fn editing_preserves_external_changes_and_refuses_paths_outside_the_repository() {
        let dir = repository();
        write(&dir, "a.txt", "original\n");
        let read = edit_file(dir.path(), "a.txt", None, None).unwrap();
        assert_eq!(read["content"], "original\n");
        edit_file(dir.path(), "a.txt", Some("edited\n"), Some("original\n")).unwrap();
        assert!(edit_file(dir.path(), "a.txt", Some("lost\n"), Some("original\n")).unwrap_err().contains("changed on disk"));
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "edited\n");
        assert!(edit_file(dir.path(), ".git/config", Some("lost"), None).is_err());
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("private"), "private").unwrap();
        std::os::unix::fs::symlink(outside.path().join("private"), dir.path().join("link")).unwrap();
        assert!(edit_file(dir.path(), "link", Some("lost"), Some("private")).is_err());
        assert_eq!(std::fs::read_to_string(outside.path().join("private")).unwrap(), "private");
    }

    #[test]
    fn one_hunk_can_be_staged_unstaged_or_reverted_without_changing_the_other() {
        let dir = repository();
        let original = (1..30).map(|line| format!("line {line}\n")).collect::<String>();
        write(&dir, "a.txt", &original);
        commit_all(&dir, "initial");
        let edited = original.replace("line 2\n", "first edit\n").replace("line 25\n", "second edit\n");
        write(&dir, "a.txt", &edited);
        let layout = changes::layout(dir.path()).unwrap();
        let file = &layout["files"][0];
        let fingerprint = file["fingerprint"].as_str().unwrap();
        assert_eq!(file["hunks"].as_array().unwrap().len(), 2);
        changes::apply_hunk(dir.path(), "a.txt", 0, fingerprint, "stage").unwrap();
        let staged = String::from_utf8(git(dir.path(), &["diff", "--cached"], &[]).unwrap()).unwrap();
        assert!(staged.contains("first edit"));
        assert!(!staged.contains("second edit"));
        assert!(changes::apply_hunk(dir.path(), "a.txt", 0, fingerprint, "revert").is_err());
        let layout = changes::layout(dir.path()).unwrap();
        let file = &layout["files"][0];
        changes::apply_hunk(dir.path(), "a.txt", 0, file["fingerprint"].as_str().unwrap(), "unstage").unwrap();
        assert!(git(dir.path(), &["diff", "--cached"], &[]).unwrap().is_empty());
        let layout = changes::layout(dir.path()).unwrap();
        let file = &layout["files"][0];
        let second_row = file["hunks"][1].as_u64().unwrap() as u32;
        changes::apply_hunk(dir.path(), "a.txt", second_row, file["fingerprint"].as_str().unwrap(), "revert").unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), original.replace("line 2\n", "first edit\n"));
    }

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
    fn read_file_returns_the_bytes_of_a_file_inside_the_folder_and_refuses_one_outside_it() {
        let dir = repository();
        write(&dir, "docs/brief.txt", "hello");
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "no").unwrap();

        let read = run(dir.path(), GitCommand::ReadFile { path: "docs/brief.txt".into() }).unwrap();
        let escaped = run(dir.path(), GitCommand::ReadFile { path: outside.path().join("secret.txt").to_string_lossy().into_owned() }).unwrap_err();
        let missing = run(dir.path(), GitCommand::ReadFile { path: "nothing.txt".into() }).unwrap_err();

        assert_eq!(read["data"], base64::engine::general_purpose::STANDARD.encode("hello"));
        assert_eq!(read["size"], 5);
        assert!(escaped.contains("outside the conversation's folder"));
        assert!(missing.contains("was not found"));
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

        let undone = run(dir.path(), GitCommand::UndoLastCommit).unwrap();

        assert_eq!(undone["message"], "Second change");
        assert_eq!(run(dir.path(), GitCommand::Log { limit: 10 }).unwrap().as_array().unwrap().len(), 1);
        assert_eq!(file(&run(dir.path(), GitCommand::Status).unwrap(), "a.txt")["staged"], true);
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
