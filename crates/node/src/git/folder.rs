use super::MOST_DIFF_BYTES;
use base64::Engine;
use serde_json::{Value, json};
use std::path::Path;

const MOST_READ_FILE_BYTES: u64 = 20 * 1024 * 1024;
const MOST_LISTED_DIRECTORIES: usize = 500;

pub(super) fn directories(root: &Path, requested: &str) -> Result<Value, String> {
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

pub(super) fn read_file(cwd: &Path, path: &str) -> Result<Value, String> {
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

pub(super) fn edit_file(cwd: &Path, path: &str, content: Option<&str>, expected: Option<&str>) -> Result<Value, String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::run_in;
    use crate::git::run;
    use crate::git::test_support::*;
    use mycelium_proto::link::GitCommand;

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
}
