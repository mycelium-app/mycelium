use super::MOST_DIFF_BYTES;
use anyhow::{Result, anyhow, bail};
use base64::Engine;
use serde_json::{Value, json};
use std::path::Path;

const MOST_READ_FILE_BYTES: u64 = 20 * 1024 * 1024;
const MOST_LISTED_DIRECTORIES: usize = 500;

pub(super) fn create_directory(root: &Path, parent: &str, name: &str) -> Result<Value> {
    let name = name.trim();

    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        bail!("A folder name cannot be empty or contain slashes. Enter a name such as quiet-otter.");
    }

    let folder = super::within(root, &Path::new(parent).join(name).to_string_lossy())?;

    if folder.exists() {
        bail!("{name} already exists in {}. Choose another name.", folder.parent().unwrap_or(&folder).display());
    }

    std::fs::create_dir_all(&folder).map_err(|cause| anyhow!("Could not create {}: {cause}.", folder.display()))?;

    Ok(json!({"path": folder.to_string_lossy(), "name": name}))
}

pub(super) fn directories(root: &Path, requested: &str) -> Result<Value> {
    let canonical_root = root.canonicalize().map_err(|cause| anyhow!("The projects folder {} cannot be read: {cause}.", root.display()))?;
    let wanted = if requested.is_empty() { canonical_root.clone() } else { canonical_root.join(requested) };
    let path = wanted.canonicalize().map_err(|_| anyhow!("{} does not exist on this node. Choose a folder inside {}.", wanted.display(), canonical_root.display()))?;

    if !path.starts_with(&canonical_root) {
        bail!("{} is outside {}. Choose a folder inside it.", path.display(), canonical_root.display());
    }

    let entries = std::fs::read_dir(&path).map_err(|cause| anyhow!("{} cannot be read: {cause}.", path.display()))?;
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

pub(super) fn read_file(cwd: &Path, path: &str) -> Result<Value> {
    let root = cwd.canonicalize().map_err(|cause| anyhow!("The conversation's folder could not be read: {cause}."))?;
    let outside = || anyhow!("{path} is outside the conversation's folder. Give a path inside it.");
    let file = root.join(path).canonicalize().map_err(|_| anyhow!("{path} was not found in the conversation's folder."))?;

    if !file.starts_with(&root) {
        return Err(outside());
    }

    let size = std::fs::metadata(&file).map_err(|cause| anyhow!("{path} could not be read: {cause}."))?.len();

    if !file.is_file() {
        bail!("{path} is not a file.");
    }

    if size > MOST_READ_FILE_BYTES {
        bail!("{path} is larger than 20 MB. Use a smaller file.");
    }

    let bytes = std::fs::read(&file).map_err(|cause| anyhow!("{path} could not be read: {cause}."))?;

    Ok(json!({"data": base64::engine::general_purpose::STANDARD.encode(bytes), "size": size}))
}

pub(super) fn write_file(cwd: &Path, path: &str, data_base64: &str) -> Result<Value> {
    let root = cwd.canonicalize().map_err(|cause| anyhow!("The conversation's folder could not be read: {cause}."))?;
    let relative = Path::new(path);
    let plain = !path.trim().is_empty() && relative.components().all(|part| matches!(part, std::path::Component::Normal(name) if name != ".git"));

    if !plain {
        bail!("Give a path inside the conversation's folder, such as exports/explanations.json, without \"..\" or .git.");
    }

    let bytes = base64::engine::general_purpose::STANDARD.decode(data_base64).map_err(|_| anyhow!("The data for {path} is not base64."))?;

    if bytes.len() as u64 > MOST_READ_FILE_BYTES {
        bail!("{path} would be larger than 20 MB. Ask for less, such as a shorter date range.");
    }

    let file = root.join(relative);
    let parent = file.parent().unwrap_or(&root);
    std::fs::create_dir_all(parent).map_err(|cause| anyhow!("{path} could not be written: {cause}."))?;

    if !parent.canonicalize().is_ok_and(|parent| parent.starts_with(&root)) {
        bail!("{path} is outside the conversation's folder. Give a path inside it.");
    }

    std::fs::write(&file, &bytes).map_err(|cause| anyhow!("{path} could not be written: {cause}."))?;

    Ok(json!({"path": path, "size": bytes.len()}))
}

pub(super) fn edit_file(cwd: &Path, path: &str, content: Option<&str>, expected: Option<&str>) -> Result<Value> {
    let root = cwd.canonicalize()?;
    let file = root.join(path).canonicalize().map_err(|_| anyhow!("{path} was not found."))?;

    if !file.starts_with(&root) || file.components().any(|part| part.as_os_str() == ".git") {
        bail!("Choose a file inside the conversation's folder, outside .git.");
    }

    let bytes = std::fs::read(&file)?;

    if bytes.len() > MOST_DIFF_BYTES || bytes.contains(&0) {
        bail!("Only text files up to 200 KB can be edited here.");
    }

    let current = String::from_utf8(bytes).map_err(|_| anyhow!("This file is not UTF-8 text."))?;

    if let Some(content) = content {
        if expected != Some(current.as_str()) {
            bail!("The file changed on disk. Reopen the editor before saving.");
        }

        if content.len() > MOST_DIFF_BYTES {
            bail!("Only text files up to 200 KB can be edited here.");
        }

        std::fs::write(&file, content)?;

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
    fn creating_a_folder_makes_it_under_the_given_parent_and_reports_its_path() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("sites")).unwrap();
        let sites = root.path().join("sites").canonicalize().unwrap();

        let created = run_in(root.path(), &sites.to_string_lossy(), GitCommand::CreateDirectory { name: " quiet-otter ".into() }).unwrap();
        let fresh = run_in(root.path(), "projects", GitCommand::CreateDirectory { name: "calm-heron".into() }).unwrap();

        assert_eq!(created["path"], sites.join("quiet-otter").to_string_lossy().as_ref());
        assert!(sites.join("quiet-otter").is_dir());
        assert!(root.path().join("projects/calm-heron").is_dir());
        assert_eq!(fresh["name"], "calm-heron");
    }

    #[test]
    fn creating_a_folder_refuses_slashes_blank_names_and_folders_that_exist() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("taken")).unwrap();

        for name in ["a/b", "..", "  ", "a\\b"] {
            let error = run_in(root.path(), "", GitCommand::CreateDirectory { name: name.into() }).unwrap_err().to_string();

            assert!(error.contains("cannot be empty or contain slashes"), "{name}: {error}");
        }

        let error = run_in(root.path(), "", GitCommand::CreateDirectory { name: "taken".into() }).unwrap_err().to_string();

        assert!(error.contains("taken already exists"), "{error}");
    }

    #[test]
    fn creating_a_folder_outside_the_projects_root_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();

        let error = run_in(root.path(), &outside.path().to_string_lossy(), GitCommand::CreateDirectory { name: "x".into() }).unwrap_err().to_string();

        assert!(error.contains("Choose a folder inside it."), "{error}");
        assert!(!outside.path().join("x").exists());
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

        let error = run_in(root.path(), &outside.path().to_string_lossy(), GitCommand::Directories).unwrap_err().to_string();

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
        assert!(edit_file(dir.path(), "a.txt", Some("lost\n"), Some("original\n")).unwrap_err().to_string().contains("changed on disk"));
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "edited\n");
        assert!(edit_file(dir.path(), ".git/config", Some("lost"), None).is_err());
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("private"), "private").unwrap();
        std::os::unix::fs::symlink(outside.path().join("private"), dir.path().join("link")).unwrap();
        assert!(edit_file(dir.path(), "link", Some("lost"), Some("private")).is_err());
        assert_eq!(std::fs::read_to_string(outside.path().join("private")).unwrap(), "private");
    }

    #[test]
    fn write_file_creates_a_file_and_its_folders_inside_the_folder_and_refuses_paths_out_of_it() {
        let dir = repository();
        let data = base64::engine::general_purpose::STANDARD.encode("[1,2]");

        let written = run(dir.path(), GitCommand::WriteFile { path: "exports/rows.json".into(), data_base64: data.clone() }).unwrap();
        let escaped = run(dir.path(), GitCommand::WriteFile { path: "../rows.json".into(), data_base64: data.clone() }).unwrap_err().to_string();
        let into_git = run(dir.path(), GitCommand::WriteFile { path: ".git/hooks/pre-commit".into(), data_base64: data }).unwrap_err().to_string();

        assert_eq!(written["size"], 5);
        assert_eq!(std::fs::read_to_string(dir.path().join("exports/rows.json")).unwrap(), "[1,2]");
        assert!(escaped.contains("without \"..\""), "{escaped}");
        assert!(into_git.contains(".git"), "{into_git}");
    }

    #[test]
    fn read_file_returns_the_bytes_of_a_file_inside_the_folder_and_refuses_one_outside_it() {
        let dir = repository();
        write(&dir, "docs/brief.txt", "hello");
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "no").unwrap();

        let read = run(dir.path(), GitCommand::ReadFile { path: "docs/brief.txt".into() }).unwrap();
        let escaped = run(dir.path(), GitCommand::ReadFile { path: outside.path().join("secret.txt").to_string_lossy().into_owned() }).unwrap_err().to_string();
        let missing = run(dir.path(), GitCommand::ReadFile { path: "nothing.txt".into() }).unwrap_err().to_string();

        assert_eq!(read["data"], base64::engine::general_purpose::STANDARD.encode("hello"));
        assert_eq!(read["size"], 5);
        assert!(escaped.contains("outside the conversation's folder"));
        assert!(missing.contains("was not found"));
    }
}
