use anyhow::{Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mycelium_proto::link::FolderFile;
use std::path::{Component, Path};

const FILE_LIMIT: u64 = 1024 * 1024;
const TOTAL_LIMIT: u64 = 25 * 1024 * 1024;
const SKIPPED: &str = ".git";

pub struct FolderRead {
    pub files: Vec<FolderFile>,
    pub truncated: bool,
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty() && Path::new(path).components().all(|component| matches!(component, Component::Normal(name) if name != SKIPPED))
}

pub fn write(projects_root: &Path, root: &str, files: &[FolderFile], replace: bool) -> Result<()> {
    let folder = crate::git::within(projects_root, root)?;

    if replace && folder.is_dir() {
        let entries = std::fs::read_dir(&folder).map_err(|cause| anyhow!("{} cannot be read: {cause}.", folder.display()))?;

        for entry in entries.flatten() {
            if entry.file_name() == SKIPPED {
                continue;
            }

            let path = entry.path();
            let removed = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };

            removed.map_err(|cause| anyhow!("{} cannot be cleared: {cause}.", path.display()))?;
        }
    }

    std::fs::create_dir_all(&folder).map_err(|cause| anyhow!("{} cannot be created: {cause}.", folder.display()))?;

    for file in files {
        if !safe_relative(&file.path) {
            bail!("{} is not a path inside the assistant folder.", file.path);
        }

        let data = STANDARD.decode(&file.data_base64).map_err(|_| anyhow!("{} is not valid data.", file.path))?;
        let target = folder.join(&file.path);

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|cause| anyhow!("{} cannot be created: {cause}.", parent.display()))?;
        }

        std::fs::write(&target, data).map_err(|cause| anyhow!("{} cannot be written: {cause}.", target.display()))?;
    }

    Ok(())
}

pub fn read(projects_root: &Path, root: &str) -> Result<FolderRead> {
    let folder = crate::git::within(projects_root, root)?;
    let mut read = FolderRead { files: Vec::new(), truncated: false };
    let mut total = 0;

    collect(&folder, &folder, &mut read, &mut total)?;
    read.files.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(read)
}

fn collect(base: &Path, directory: &Path, read: &mut FolderRead, total: &mut u64) -> Result<()> {
    let entries = std::fs::read_dir(directory).map_err(|cause| anyhow!("{} cannot be read: {cause}.", directory.display()))?;

    for entry in entries.flatten() {
        let path = entry.path();

        if entry.file_name() == SKIPPED {
            continue;
        }

        let Ok(kind) = entry.file_type() else {
            continue;
        };

        if kind.is_dir() {
            collect(base, &path, read, total)?;

            continue;
        }

        if !kind.is_file() {
            continue;
        }

        let size = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);

        if size > FILE_LIMIT || *total + size > TOTAL_LIMIT {
            read.truncated = true;

            continue;
        }

        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let Ok(relative) = path.strip_prefix(base) else {
            continue;
        };

        *total += size;
        read.files.push(FolderFile { path: relative.to_string_lossy().replace('\\', "/"), data_base64: STANDARD.encode(data) });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, text: &str) -> FolderFile {
        FolderFile { path: path.into(), data_base64: STANDARD.encode(text) }
    }

    #[test]
    fn writing_a_folder_outside_the_projects_root_is_refused() {
        let root = tempfile::tempdir().unwrap();

        let outside = write(root.path(), "../elsewhere", &[file("a.md", "x")], true);
        let escaping = write(root.path(), "assistant", &[file("../a.md", "x")], true);

        assert!(outside.unwrap_err().to_string().contains("outside"));
        assert!(escaping.is_err());
        assert!(!root.path().join("a.md").exists());
    }

    #[test]
    fn replacing_a_folder_removes_files_that_storage_no_longer_has() {
        let root = tempfile::tempdir().unwrap();

        write(root.path(), "assistant", &[file("old.md", "old"), file("notes/a.md", "a")], true).unwrap();
        write(root.path(), "assistant", &[file("new.md", "new")], true).unwrap();

        let read = read(root.path(), "assistant").unwrap();

        assert_eq!(read.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), vec!["new.md"]);
    }

    #[test]
    fn reading_a_folder_skips_git_and_reports_truncation() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("assistant");

        std::fs::create_dir_all(folder.join(".git")).unwrap();
        std::fs::write(folder.join(".git/config"), "secret").unwrap();
        std::fs::write(folder.join("AGENTS.md"), "hello").unwrap();
        std::fs::write(folder.join("huge.bin"), vec![0u8; FILE_LIMIT as usize + 1]).unwrap();

        let read = read(root.path(), "assistant").unwrap();

        assert_eq!(read.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), vec!["AGENTS.md"]);
        assert!(read.truncated);
    }
}
