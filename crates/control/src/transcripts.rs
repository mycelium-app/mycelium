use std::io::SeekFrom;
use std::path::{Component, Path, PathBuf};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use uuid::Uuid;

#[derive(Debug)]
pub enum WriteError {
    UnsafePath,
    Io(std::io::Error),
}

impl From<std::io::Error> for WriteError {
    fn from(cause: std::io::Error) -> WriteError {
        WriteError::Io(cause)
    }
}

pub fn safe_relative(path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.contains('\0') || path.contains('\\') {
        return None;
    }

    let relative = Path::new(path);

    if relative.components().all(|component| matches!(component, Component::Normal(_))) {
        return Some(relative.to_path_buf());
    }

    None
}

pub async fn write_chunk(data_dir: &Path, node_id: Uuid, path: &str, offset: i64, bytes: &[u8]) -> Result<i64, WriteError> {
    let relative = safe_relative(path).ok_or(WriteError::UnsafePath)?;
    let full = data_dir.join("transcripts").join(node_id.to_string()).join(relative);

    if let Some(parent) = full.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let current = match tokio::fs::metadata(&full).await {
        Ok(metadata) => metadata.len() as i64,
        Err(_) => 0,
    };

    if offset < 0 || offset > current {
        return Ok(current);
    }

    let mut file = tokio::fs::OpenOptions::new().write(true).create(true).truncate(false).open(&full).await?;
    file.set_len(offset as u64).await?;
    file.seek(SeekFrom::Start(offset as u64)).await?;
    file.write_all(bytes).await?;
    file.flush().await?;

    Ok(offset + bytes.len() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_relative_paths_are_accepted() {
        assert!(safe_relative("projects/-home-me-app/abc.jsonl").is_some());
    }

    #[test]
    fn traversal_absolute_and_odd_paths_are_rejected() {
        for path in ["", "../escape", "a/../../escape", "/etc/passwd", "./a", "a\\b", "a\0b", ".."] {
            assert!(safe_relative(path).is_none(), "{path:?} should be rejected");
        }
    }
}
