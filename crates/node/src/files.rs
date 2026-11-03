use crate::git::{git, within};
use std::path::Path;

const MOST_MATCHES: usize = 50;
const MOST_WALKED: usize = 20_000;
pub(crate) const SKIPPED_DIRECTORIES: &[&str] = &[".git", "node_modules", "target", "build", ".gradle", "dist", ".venv"];

pub fn matching_files(root: &Path, cwd: &str, query: &str) -> Vec<String> {
    let Ok(cwd) = within(root, cwd) else {
        return Vec::new();
    };
    let mut paths = tracked_files(&cwd).unwrap_or_else(|| walked_files(&cwd));
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
    let listing = git(cwd, &["ls-files", "--cached", "--others", "--exclude-standard", "-z"], &[]).ok()?;

    Some(listing.split(|byte| *byte == 0).filter(|path| !path.is_empty()).map(|path| String::from_utf8_lossy(path).into_owned()).collect())
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
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("app");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules/flake")).unwrap();
        std::fs::write(dir.join("src/default.nix"), "").unwrap();
        std::fs::write(dir.join("flake.nix"), "").unwrap();
        std::fs::write(dir.join("node_modules/flake/flake.nix"), "").unwrap();

        assert_eq!(matching_files(root.path(), "app", "flake"), vec!["flake.nix".to_string()]);
        assert_eq!(matching_files(root.path(), "app", "NIX"), vec!["flake.nix".to_string(), "src/default.nix".to_string()]);
    }

    #[test]
    fn a_git_repository_lists_what_git_would_and_skips_ignored_files() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("app");
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"], &[]).unwrap();
        std::fs::write(dir.join(".gitignore"), "secret.txt\n").unwrap();
        std::fs::write(dir.join("secret.txt"), "").unwrap();
        std::fs::write(dir.join("readme.md"), "").unwrap();

        assert_eq!(matching_files(root.path(), "app", ""), vec!["readme.md".to_string(), ".gitignore".to_string()]);
    }

    #[test]
    fn a_folder_outside_the_projects_root_lists_nothing() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::write(elsewhere.path().join("private.txt"), "").unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), root.path().join("link")).unwrap();

        for requested in [elsewhere.path().to_str().unwrap(), "../elsewhere", "link"] {
            assert!(matching_files(root.path(), requested, "").is_empty(), "{requested}");
        }
    }
}
