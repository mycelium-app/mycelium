use std::path::Path;
use std::process::Command;

pub fn keeper_pids(sessions_dir: &Path) -> Vec<String> {
    let output = Command::new("pgrep").args(["-f", &format!("keeper --dir {}", sessions_dir.display())]).output().unwrap();

    String::from_utf8_lossy(&output.stdout).split_whitespace().map(str::to_string).collect()
}

pub fn kill_keepers(sessions_dir: &Path) {
    for pid in keeper_pids(sessions_dir) {
        let _ = Command::new("kill").args(["-KILL", "--", &format!("-{pid}")]).status();
    }
}
