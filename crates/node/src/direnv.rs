use std::path::Path;
use std::process::{Command, Stdio};

const ALLOWED: i64 = 0;

pub fn exec_prefix(folder: &Path) -> Vec<String> {
    match crate::agents::resolve_program("direnv") {
        Some(direnv) => exec_prefix_with(&direnv, folder),
        None => Vec::new(),
    }
}

fn exec_prefix_with(direnv: &Path, folder: &Path) -> Vec<String> {
    let Ok(output) = Command::new(direnv).args(["status", "--json"]).current_dir(folder).stdin(Stdio::null()).stderr(Stdio::null()).output() else {
        return Vec::new();
    };
    let status = serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap_or_default();

    if status["state"]["foundRC"]["allowed"].as_i64() != Some(ALLOWED) {
        return Vec::new();
    }

    vec![direnv.to_string_lossy().into_owned(), "exec".to_string(), folder.to_string_lossy().into_owned()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fake_direnv(dir: &Path, allowed: Option<i64>) -> std::path::PathBuf {
        let found = allowed.map_or("null".to_string(), |allowed| format!(r#"{{"allowed": {allowed}, "path": "/work/app/.envrc"}}"#));
        let program = dir.join("direnv");
        std::fs::write(&program, format!("#!/bin/sh\ncat <<'JSON'\n{{\"state\": {{\"foundRC\": {found}, \"loadedRC\": null}}}}\nJSON\n")).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();

        program
    }

    #[test]
    fn an_allowed_envrc_runs_the_command_through_direnv_exec_in_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let direnv = fake_direnv(dir.path(), Some(0));

        assert_eq!(exec_prefix_with(&direnv, dir.path()), vec![direnv.to_string_lossy().into_owned(), "exec".to_string(), dir.path().to_string_lossy().into_owned()]);
    }

    #[test]
    fn an_envrc_not_allowed_yet_or_denied_starts_the_command_as_it_is() {
        let dir = tempfile::tempdir().unwrap();

        for allowed in [1, 2] {
            let direnv = fake_direnv(dir.path(), Some(allowed));

            assert!(exec_prefix_with(&direnv, dir.path()).is_empty(), "allowed {allowed}");
        }
    }

    #[test]
    fn a_folder_without_an_envrc_starts_the_command_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let direnv = fake_direnv(dir.path(), None);

        assert!(exec_prefix_with(&direnv, dir.path()).is_empty());
    }
}
