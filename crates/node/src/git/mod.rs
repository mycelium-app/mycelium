mod commits;
mod folder;
pub(crate) mod history;
mod moves;
mod repository;
mod stack;
#[cfg(test)]
pub(crate) mod test_support;

use crate::changes;
use anyhow::{Result, anyhow, bail};
use commits::{changed, commit, push, remote, undo_last_commit};
use folder::{directories, edit_file, read_file};
use history::{branches, diff, graph, head, log, scope_diff, show, status, switch};
use moves::{apply, move_plan, snapshot, undo};
use mycelium_proto::link::GitCommand;
use repository::{add_worktree, clone, register};
use serde_json::Value;
use stack::{run_town, stack};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

const MOST_DIFF_BYTES: usize = 200 * 1024;
const MOST_MOVE_BYTES: usize = 8 * 1024 * 1024;

pub fn run_in(root: &Path, cwd: &str, command: GitCommand) -> Result<Value> {
    match command {
        GitCommand::Clone { url, credentials } => {
            if !url.starts_with("https://") && !url.starts_with("http://") {
                bail!("{url} is not an http or https address, so this node will not clone it.");
            }

            clone(&within(root, cwd)?, &url, credentials.as_ref())
        }
        GitCommand::Register => register(&within(root, cwd)?),
        GitCommand::Directories => directories(root, cwd),
        GitCommand::AddWorktree { branch } => add_worktree(&within(root, cwd)?, &branch),
        other => run(Path::new(cwd), other),
    }
}

pub fn run(cwd: &Path, command: GitCommand) -> Result<Value> {
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
        GitCommand::RunTests { command, timeout_secs } => crate::test_run::run(cwd, command.as_deref(), timeout_secs),
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
        GitCommand::Clone { .. } | GitCommand::Register | GitCommand::Directories | GitCommand::AddWorktree { .. } => bail!("This needs the node's projects folder, not a conversation's folder."),
    }
}

pub(crate) fn git(cwd: &Path, args: &[&str], accepted: &[i32]) -> Result<Vec<u8>> {
    git_with_env(cwd, args, accepted, &[])
}

fn git_with_env(cwd: &Path, args: &[&str], accepted: &[i32], env: &[(String, String)]) -> Result<Vec<u8>> {
    git_with_stdin(cwd, args, accepted, env, &[])
}

pub(crate) fn git_with_input(cwd: &Path, args: &[&str], input: &[u8]) -> Result<Vec<u8>> {
    git_with_stdin(cwd, args, &[], &[], input)
}

fn git_with_stdin(cwd: &Path, args: &[&str], accepted: &[i32], env: &[(String, String)], input: &[u8]) -> Result<Vec<u8>> {
    let could_not_start = |cause: std::io::Error| anyhow!("Git could not be started: {cause}.");
    let mut child = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .envs(env.iter().map(|(name, value)| (name, value)))
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(if input.is_empty() { Stdio::null() } else { Stdio::piped() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(could_not_start)?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input).map_err(could_not_start)?;
    }

    let output = child.wait_with_output().map_err(could_not_start)?;

    if output.status.success() || output.status.code().is_some_and(|code| accepted.contains(&code)) {
        return Ok(output.stdout);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

    Err(anyhow!(if stderr.is_empty() { stdout } else { stderr }))
}

pub(crate) fn within(root: &Path, requested: &str) -> Result<PathBuf> {
    let joined = root.join(requested);
    let outside = || anyhow!("{} is outside {}. Choose a folder inside it.", joined.display(), root.display());

    if joined.components().any(|component| component == Component::ParentDir) {
        return Err(outside());
    }

    let inside_root = joined.starts_with(root) || root.canonicalize().is_ok_and(|canonical| joined.starts_with(canonical));

    if !inside_root || joined == root {
        return Err(outside());
    }

    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_reaches_git_on_standard_input_and_a_failure_returns_its_message() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"], &[]).unwrap();

        let hash = git_with_input(dir.path(), &["hash-object", "--stdin"], b"hello\n").unwrap();
        let refused = git_with_input(dir.path(), &["apply", "--check"], b"not a patch\n").unwrap_err().to_string();

        assert_eq!(String::from_utf8(hash).unwrap().trim(), "ce013625030ba8dba906f756967f9e9ca394464a");
        assert!(!refused.is_empty());
    }

    #[test]
    fn a_folder_outside_the_projects_root_or_climbing_out_of_it_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();

        for requested in ["../escape", "projects/../../escape", elsewhere.path().to_str().unwrap(), ""] {
            let error = run_in(root.path(), requested, GitCommand::Register).unwrap_err().to_string();

            assert!(error.contains("outside"), "{requested}: {error}");
        }
    }
}
