use std::path::Path;
use std::process::Command;

const MOST_MATCHES: usize = 50;
const MOST_WALKED: usize = 20_000;
const SKIPPED_DIRECTORIES: &[&str] = &[".git", "node_modules", "target", "build", ".gradle", "dist", ".venv"];

pub fn matching_files(cwd: &Path, query: &str) -> Vec<String> {
    let mut paths = tracked_files(cwd).unwrap_or_else(|| walked_files(cwd));
    let needle = query.to_lowercase();

    paths.retain(|path| path.to_lowercase().contains(&needle));
    paths.sort_by_key(|path| {
        let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
        let rank = if name.starts_with(&needle) { 0 } else if name.contains(&needle) { 1 } else { 2 };

        (rank, path.len(), path.clone())
    });
    paths.truncate(MOST_MATCHES);

    paths
}

fn tracked_files(cwd: &Path) -> Option<Vec<String>> {
    let output = Command::new("git").arg("-C").arg(cwd).args(["ls-files", "--cached", "--others", "--exclude-standard", "-z"]).output().ok()?;

    if !output.status.success() {
        return None;
    }

    Some(output.stdout.split(|byte| *byte == 0).filter(|path| !path.is_empty()).map(|path| String::from_utf8_lossy(path).into_owned()).collect())
}

fn walked_files(cwd: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![cwd.to_path_buf()];

    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };

        for entry in entries.flatten() {
            if found.len() >= MOST_WALKED {
                return found;
            }

            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();

            if path.is_dir() {
                if !SKIPPED_DIRECTORIES.contains(&name.as_str()) {
                    pending.push(path);
                }
            } else if let Ok(relative) = path.strip_prefix(cwd) {
                found.push(relative.to_string_lossy().into_owned());
            }
        }
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_whose_name_starts_with_the_query_come_first_and_skipped_directories_are_left_out() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/flake")).unwrap();
        std::fs::write(dir.path().join("src/default.nix"), "").unwrap();
        std::fs::write(dir.path().join("flake.nix"), "").unwrap();
        std::fs::write(dir.path().join("node_modules/flake/flake.nix"), "").unwrap();

        assert_eq!(matching_files(dir.path(), "flake"), vec!["flake.nix".to_string()]);
        assert_eq!(matching_files(dir.path(), "NIX"), vec!["flake.nix".to_string(), "src/default.nix".to_string()]);
    }

    #[test]
    fn a_git_repository_lists_what_git_would_and_skips_ignored_files() {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| Command::new("git").arg("-C").arg(dir.path()).args(args).output().unwrap();
        git(&["init", "-q"]);
        std::fs::write(dir.path().join(".gitignore"), "secret.txt\n").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "").unwrap();
        std::fs::write(dir.path().join("readme.md"), "").unwrap();

        assert_eq!(matching_files(dir.path(), ""), vec!["readme.md".to_string(), ".gitignore".to_string()]);
    }
}
