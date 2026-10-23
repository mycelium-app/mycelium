use crate::agents::{AccountHome, account_homes};
use mycelium_proto::conversation::AgentId;
use std::path::{Path, PathBuf};

pub const MARKER: &str = "<!-- Managed by Mycelium. Changes made here are overwritten. -->";

fn file_name(agent: AgentId) -> &'static str {
    match agent {
        AgentId::Claude => "CLAUDE.md",
        AgentId::Codex => "AGENTS.md",
    }
}

fn agent_label(agent: AgentId) -> &'static str {
    match agent {
        AgentId::Claude => "Claude Code",
        AgentId::Codex => "Codex",
    }
}

fn managed_content(global: &str) -> String {
    let body = global.trim_end();

    if body.is_empty() { format!("{MARKER}\n") } else { format!("{MARKER}\n\n{body}\n") }
}

fn write(target: &Path, content: &str) -> std::io::Result<()> {
    if let Some(directory) = target.parent() {
        std::fs::create_dir_all(directory)?;
    }

    let staged = target.with_extension("md.mycelium");
    std::fs::write(&staged, content)?;

    std::fs::rename(&staged, target)
}

fn set_aside(target: &Path) -> std::io::Result<()> {
    let mut backup = target.with_extension("md.bak");
    let mut copy = 1;

    while backup.symlink_metadata().is_ok() {
        copy += 1;
        backup = target.with_extension(format!("md.bak.{copy}"));
    }

    std::fs::rename(target, backup)
}

pub fn deliver(home: &Path, accounts_dir: &Path, agents: &[AgentId], global: &str, take_over: bool) -> Vec<String> {
    let content = managed_content(global);
    let mut unmanaged = Vec::new();

    for agent in agents {
        for AccountHome { dir, .. } in account_homes(home, accounts_dir, *agent) {
            let target: PathBuf = dir.join(file_name(*agent));
            let existing = std::fs::read_to_string(&target);

            match existing {
                Ok(text) if !text.starts_with(MARKER) && !take_over => unmanaged.push(format!("{}: {}", agent_label(*agent), target.display())),
                Ok(text) if !text.starts_with(MARKER) => {
                    let moved = set_aside(&target).and_then(|()| write(&target, &content));

                    if let Err(error) = moved {
                        tracing::warn!("could not take over {}: {error}", target.display());
                        unmanaged.push(format!("{}: {}", agent_label(*agent), target.display()));
                    }
                }
                Ok(text) if text == content => {}
                Ok(_) | Err(_) => {
                    if let Err(error) = write(&target, &content) {
                        tracing::warn!("could not write {}: {error}", target.display());
                    }
                }
            }
        }
    }

    unmanaged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let accounts = root.path().join("state").join("accounts");
        std::fs::create_dir_all(&home).unwrap();

        (root, home, accounts)
    }

    #[test]
    fn the_global_file_is_written_where_each_agent_reads_its_user_instructions() {
        let (_root, home, accounts) = dirs();

        let unmanaged = deliver(&home, &accounts, &[AgentId::Claude, AgentId::Codex], "# Rules\n", false);

        assert!(unmanaged.is_empty());
        assert_eq!(std::fs::read_to_string(home.join(".claude/CLAUDE.md")).unwrap(), format!("{MARKER}\n\n# Rules\n"));
        assert_eq!(std::fs::read_to_string(home.join(".codex/AGENTS.md")).unwrap(), format!("{MARKER}\n\n# Rules\n"));
    }

    #[test]
    fn each_signed_in_account_gets_the_file_in_its_own_config_directory() {
        let (_root, home, accounts) = dirs();
        std::fs::create_dir_all(accounts.join("claude").join("work")).unwrap();
        std::fs::create_dir_all(accounts.join("codex").join("work")).unwrap();

        deliver(&home, &accounts, &[AgentId::Claude, AgentId::Codex], "# Rules", false);

        assert!(accounts.join("claude/work/CLAUDE.md").exists());
        assert!(accounts.join("codex/work/AGENTS.md").exists());
    }

    #[test]
    fn a_file_mycelium_wrote_is_replaced_with_the_new_version() {
        let (_root, home, accounts) = dirs();
        deliver(&home, &accounts, &[AgentId::Claude], "# One", false);

        let unmanaged = deliver(&home, &accounts, &[AgentId::Claude], "# Two", false);

        assert!(unmanaged.is_empty());
        assert_eq!(std::fs::read_to_string(home.join(".claude/CLAUDE.md")).unwrap(), format!("{MARKER}\n\n# Two\n"));
    }

    #[test]
    fn a_file_the_person_wrote_is_left_alone_and_reported_as_not_managed() {
        let (_root, home, accounts) = dirs();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::write(home.join(".claude/CLAUDE.md"), "My own rules\n").unwrap();

        let unmanaged = deliver(&home, &accounts, &[AgentId::Claude, AgentId::Codex], "# Rules", false);

        assert_eq!(std::fs::read_to_string(home.join(".claude/CLAUDE.md")).unwrap(), "My own rules\n");
        assert_eq!(unmanaged, vec![format!("Claude Code: {}", home.join(".claude/CLAUDE.md").display())]);
        assert!(home.join(".codex/AGENTS.md").exists());
    }

    #[test]
    fn taking_over_sets_the_persons_file_aside_as_bak_and_writes_the_managed_one() {
        let (_root, home, accounts) = dirs();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::write(home.join(".claude/CLAUDE.md"), "My own rules\n").unwrap();
        std::fs::write(home.join(".claude/CLAUDE.md.bak"), "An older backup\n").unwrap();

        let unmanaged = deliver(&home, &accounts, &[AgentId::Claude], "# Rules", true);

        assert!(unmanaged.is_empty());
        assert_eq!(std::fs::read_to_string(home.join(".claude/CLAUDE.md")).unwrap(), format!("{MARKER}\n\n# Rules\n"));
        assert_eq!(std::fs::read_to_string(home.join(".claude/CLAUDE.md.bak")).unwrap(), "An older backup\n");
        assert_eq!(std::fs::read_to_string(home.join(".claude/CLAUDE.md.bak.2")).unwrap(), "My own rules\n");
    }

    #[test]
    fn taking_over_a_symlinked_file_moves_the_link_and_leaves_what_it_points_at() {
        let (root, home, accounts) = dirs();
        let shared = root.path().join("agents-md/AGENTS.md");
        std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
        std::fs::write(&shared, "Shared rules\n").unwrap();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::os::unix::fs::symlink(&shared, home.join(".claude/CLAUDE.md")).unwrap();

        deliver(&home, &accounts, &[AgentId::Claude], "# Rules", true);

        assert_eq!(std::fs::read_to_string(&shared).unwrap(), "Shared rules\n");
        assert!(std::fs::symlink_metadata(home.join(".claude/CLAUDE.md.bak")).unwrap().file_type().is_symlink());
        assert!(!std::fs::symlink_metadata(home.join(".claude/CLAUDE.md")).unwrap().file_type().is_symlink());
    }

    #[test]
    fn an_empty_file_without_the_marker_is_not_mycelium_s_to_overwrite() {
        let (_root, home, accounts) = dirs();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(home.join(".codex/AGENTS.md"), "").unwrap();

        let unmanaged = deliver(&home, &accounts, &[AgentId::Codex], "# Rules", false);

        assert_eq!(unmanaged.len(), 1);
        assert_eq!(std::fs::read_to_string(home.join(".codex/AGENTS.md")).unwrap(), "");
    }

    #[test]
    fn an_unchanged_file_is_not_rewritten() {
        let (_root, home, accounts) = dirs();
        deliver(&home, &accounts, &[AgentId::Claude], "# Rules", false);
        let target = home.join(".claude/CLAUDE.md");
        let before = std::fs::metadata(&target).unwrap().modified().unwrap();

        deliver(&home, &accounts, &[AgentId::Claude], "# Rules", false);

        assert_eq!(std::fs::metadata(&target).unwrap().modified().unwrap(), before);
    }
}
