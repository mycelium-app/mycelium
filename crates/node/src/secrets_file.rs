use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const SECRETS_FILE_ENV: &str = "MYCELIUM_SECRETS_FILE";

pub fn path_in(session_dir: &Path) -> PathBuf {
    session_dir.join("secrets.env")
}

pub fn write(session_dir: &Path, secrets: &[(String, String)]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(session_dir)?;

    let path = path_in(session_dir);
    let staging = session_dir.join("secrets.env.new");
    let _ = std::fs::remove_file(&staging);

    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&staging)?;
    file.write_all(render(secrets).as_bytes())?;
    file.sync_all()?;
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(&staging, &path)?;

    Ok(path)
}

fn render(secrets: &[(String, String)]) -> String {
    secrets.iter().map(|(name, value)| format!("{name}={}\n", quote(value))).collect()
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn sourced(path: &Path, name: &str) -> String {
        let script = format!("set -a; . '{}'; printf %s \"${name}\"", path.display());
        let output = Command::new("sh").arg("-c").arg(script).output().unwrap();

        String::from_utf8(output.stdout).unwrap()
    }

    #[test]
    fn the_file_is_readable_by_its_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &[("TOKEN".into(), "abc".into())]).unwrap();

        assert_eq!(std::fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn awkward_values_survive_a_shell_source() {
        let dir = tempfile::tempdir().unwrap();
        let awkward = "it's \"a\" $HOME `id` \\n\nnext line; rm -rf /";
        let path = write(dir.path(), &[("TOKEN".into(), awkward.into()), ("OTHER".into(), "plain".into())]).unwrap();

        assert_eq!(sourced(&path, "TOKEN"), awkward);
        assert_eq!(sourced(&path, "OTHER"), "plain");
    }

    #[test]
    fn rewriting_replaces_the_values_and_leaves_no_staging_file() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &[("TOKEN".into(), "old".into())]).unwrap();
        let path = write(dir.path(), &[("OTHER".into(), "new".into())]).unwrap();

        assert_eq!(sourced(&path, "TOKEN"), "");
        assert_eq!(sourced(&path, "OTHER"), "new");
        assert!(!dir.path().join("secrets.env.new").exists());
    }
}
