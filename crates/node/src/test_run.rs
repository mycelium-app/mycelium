use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const OUTPUT_LIMIT: usize = 64 * 1024;
const DEFAULT_TIMEOUT_SECS: u32 = 600;

#[derive(Debug, Default, PartialEq)]
pub struct Summary {
    pub passed: Option<u32>,
    pub failed: Option<u32>,
    pub failing: Vec<String>,
}

fn detected(cwd: &Path) -> Option<&'static str> {
    if cwd.join("Cargo.toml").exists() {
        Some("cargo test")
    } else if cwd.join("gradlew").exists() {
        Some("./gradlew test")
    } else if cwd.join("package.json").exists() {
        Some("npm test")
    } else {
        None
    }
}

fn number_before(line: &str, word: &str) -> Option<u32> {
    let end = line.find(word)?;

    line[..end].trim_end().rsplit(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

pub fn summarise(output: &str) -> Summary {
    let mut summary = Summary::default();
    let mut add = |field: &mut Option<u32>, count: u32| *field = Some(field.unwrap_or(0) + count);

    for line in output.lines() {
        let line = line.trim();

        if line.starts_with("test result:") {
            add(&mut summary.passed, number_before(line, " passed").unwrap_or(0));
            add(&mut summary.failed, number_before(line, " failed").unwrap_or(0));
        } else if let Some(name) = line.strip_prefix("test ").and_then(|rest| rest.strip_suffix(" ... FAILED")) {
            summary.failing.push(name.to_string());
        } else if line.starts_with("Summary [") && line.contains(" tests run:") {
            summary.passed = Some(number_before(line, " passed").unwrap_or(0));
            summary.failed = Some(number_before(line, " failed").unwrap_or(0));
        } else if line.starts_with("FAIL [") {
            let name = line.split_once(']').map(|(_, name)| name.trim()).unwrap_or_default();

            summary.failing.push(name.to_string());
        } else if line.starts_with("Tests:") && line.ends_with(" total") {
            let total = number_before(line, " total").unwrap_or(0);
            let failed = number_before(line, " failed").unwrap_or(0);

            summary.failed = Some(failed);
            summary.passed = Some(total.saturating_sub(failed));
        } else if let Some(name) = line.strip_suffix(" FAILED").filter(|name| name.contains(" > ")) {
            summary.failing.push(name.to_string());
        } else if line.contains(" tests completed, ") && line.contains(" failed") {
            let total = number_before(line, " tests completed").unwrap_or(0);
            let failed = number_before(line, " failed").unwrap_or(0);

            summary.failed = Some(failed);
            summary.passed = Some(total.saturating_sub(failed));
        } else if let Some(name) = line.strip_prefix("● ").filter(|name| name.contains(" › ")) {
            summary.failing.push(name.to_string());
        }
    }

    summary.failing.dedup();
    summary
}

fn tail(bytes: &[u8]) -> String {
    let start = bytes.len().saturating_sub(OUTPUT_LIMIT);

    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

pub fn run(cwd: &Path, command: Option<&str>, timeout_secs: u32) -> Result<Value> {
    let command = match command.map(str::trim).filter(|command| !command.is_empty()) {
        Some(command) => command,
        None => detected(cwd).ok_or_else(|| anyhow!("No test command is set and none could be found here (Cargo.toml, gradlew or package.json). Set the project's test command."))?,
    };
    let seconds = if timeout_secs == 0 { DEFAULT_TIMEOUT_SECS } else { timeout_secs };
    let mut child = Command::new("sh").args(["-c", &format!("{command} 2>&1")]).current_dir(cwd).process_group(0).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|cause| anyhow!("Could not start the tests: {cause}"))?;
    let mut stdout = child.stdout.take().ok_or_else(|| anyhow!("Could not read the test output."))?;
    let reader = std::thread::spawn(move || {
        let mut collected = Vec::new();
        let _ = stdout.read_to_end(&mut collected);

        collected
    });
    let deadline = Instant::now() + Duration::from_secs(u64::from(seconds));
    let mut timed_out = false;

    let status = loop {
        match child.try_wait().map_err(|cause| anyhow!("Could not wait for the tests: {cause}"))? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                timed_out = true;
                let _ = Command::new("kill").args(["-KILL", "--", &format!("-{}", child.id())]).status();

                break child.wait().map_err(|cause| anyhow!("Could not stop the tests: {cause}"))?;
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };

    let collected = reader.join().unwrap_or_default();
    let output = String::from_utf8_lossy(&collected).into_owned();
    let summary = summarise(&output);
    let exit = status.code().unwrap_or(-1);
    let failed = summary.failed.unwrap_or(if exit == 0 { 0 } else { summary.failing.len().max(1) as u32 });

    Ok(json!({"exit": exit, "passed": summary.passed, "failed": failed, "failing": summary.failing, "timed_out": timed_out, "output_tail": tail(output.as_bytes())}))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARGO: &str = "running 6 tests\ntest a::ok ... ok\ntest a::broken ... FAILED\n\nfailures:\n\ntest result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";

    #[test]
    fn run_tests_reports_failing_names_for_cargo_output() {
        let summary = summarise(CARGO);

        assert_eq!(summary, Summary { passed: Some(5), failed: Some(1), failing: vec!["a::broken".to_string()] });
    }

    #[test]
    fn nextest_gradle_and_jest_summaries_are_read() {
        let nextest = summarise("        FAIL [   0.1s] crate::bin tests::broken\n     Summary [   1.0s] 6 tests run: 5 passed, 1 failed, 0 skipped\n");
        let gradle = summarise("FooTest > bar FAILED\n6 tests completed, 1 failed\n");
        let jest = summarise("  ● suite › case\nTests:       1 failed, 5 passed, 6 total\n");

        assert_eq!((nextest.passed, nextest.failed, nextest.failing), (Some(5), Some(1), vec!["crate::bin tests::broken".to_string()]));
        assert_eq!((gradle.passed, gradle.failed, gradle.failing), (Some(5), Some(1), vec!["FooTest > bar".to_string()]));
        assert_eq!((jest.passed, jest.failed, jest.failing), (Some(5), Some(1), vec!["suite › case".to_string()]));
    }

    #[test]
    fn the_command_runs_in_the_folder_and_exit_code_is_the_fallback() {
        let dir = std::env::temp_dir().join(format!("run-tests-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let answer = run(&dir, Some("echo 'test result: FAILED. 5 passed; 1 failed;'; echo 'test x ... FAILED'; exit 101"), 30).unwrap();
        let fallback = run(&dir, Some("echo nothing useful; exit 3"), 30).unwrap();
        let none = run(&dir, None, 30);

        assert_eq!((answer["exit"].clone(), answer["passed"].clone(), answer["failed"].clone(), answer["failing"].clone()), (json!(101), json!(5), json!(1), json!(["x"])));
        assert_eq!((fallback["exit"].clone(), fallback["failed"].clone()), (json!(3), json!(1)));
        assert!(none.is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_test_run_that_overruns_is_stopped() {
        let answer = run(Path::new("/tmp"), Some("sleep 30"), 1).unwrap();

        assert_eq!(answer["timed_out"], true);
    }
}
