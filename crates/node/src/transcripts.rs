use anyhow::{Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mycelium_proto::link::NodeToControl;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use uuid::Uuid;

const CHUNK_BYTES: u64 = 256 * 1024;
const CHUNKS_PER_FILE_PER_POLL: usize = 4;
const CHUNKS_PER_POLL: usize = 16;
const PROGRESS_EVERY: std::time::Duration = std::time::Duration::from_secs(15);
const AGENT_QUIET_LIMIT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

pub struct TranscriptFollower {
    home: PathBuf,
    pub(crate) roots: Vec<PathBuf>,
    state_file: PathBuf,
    out: mpsc::UnboundedSender<NodeToControl>,
    acked: HashMap<String, i64>,
    sent: HashMap<String, i64>,
    workflow_positions: HashMap<String, u64>,
    workflows_scanned: bool,
    running_agents: HashMap<String, HashMap<String, RunningAgent>>,
    planned: HashSet<String>,
    ended: HashSet<String>,
    session_lengths: HashMap<String, u64>,
    agent_quiet_limit: std::time::Duration,
    dirty: bool,
    chunks_left: usize,
}

struct RunningAgent {
    key: String,
    sent_at: Option<std::time::Instant>,
    changed_at: std::time::Instant,
    stats: serde_json::Map<String, Value>,
}

struct WorkflowJournal {
    session_id: String,
    run_id: String,
}

fn is_agent_id(id: &str) -> bool {
    id.chars().all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_')
}

fn agent_stats(journal: &Path, agent_id: &str) -> serde_json::Map<String, Value> {
    let file_name = format!("agent-{agent_id}.jsonl");
    let transcript = journal.parent().into_iter().flat_map(|run| [run.to_path_buf(), run.parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_default()]).map(|folder| folder.join(&file_name)).find_map(|candidate| std::fs::read_to_string(candidate).ok());
    let mut tokens_by_message: HashMap<String, u64> = HashMap::new();
    let mut first: Option<chrono::DateTime<chrono::FixedOffset>> = None;
    let mut last: Option<chrono::DateTime<chrono::FixedOffset>> = None;
    let mut stats = serde_json::Map::new();

    for row in transcript.as_deref().unwrap_or_default().lines().filter_map(|row| serde_json::from_str::<Value>(row).ok()) {
        let stamp = row["timestamp"].as_str().and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok());
        first = [first, stamp].into_iter().flatten().min();
        last = [last, stamp].into_iter().flatten().max();

        if let (Some(id), true) = (row["message"]["id"].as_str(), row["type"] == "assistant") {
            let usage = &row["message"]["usage"];
            let counted: u64 = ["input_tokens", "output_tokens", "cache_creation_input_tokens"].iter().filter_map(|key| usage[*key].as_u64()).sum();
            tokens_by_message.insert(id.to_string(), counted);
        }
    }

    if !tokens_by_message.is_empty() {
        stats.insert("tokens".to_string(), tokens_by_message.values().sum::<u64>().into());
        stats.insert("turns".to_string(), tokens_by_message.len().into());
    }

    if let (Some(first), Some(last)) = (first, last) {
        stats.insert("elapsed_ms".to_string(), (last - first).num_milliseconds().into());
    }

    stats
}

fn planned_phases(journal: &Path, run_id: &str) -> Option<Vec<String>> {
    let session = journal.parent()?.parent()?.parent()?.parent()?;
    let suffix = format!("-{run_id}.js");
    let script = std::fs::read_dir(session.join("workflows").join("scripts")).ok()?.flatten().find(|entry| entry.file_name().to_string_lossy().ends_with(&suffix))?;
    let text = std::fs::read_to_string(script.path()).ok()?;
    let phases = text.split_once("phases:")?.1;
    let phases = &phases[..phases.find(']')?];
    let mut titles = Vec::new();

    for piece in phases.split("title:").skip(1) {
        let piece = piece.trim_start();
        let quote = piece.chars().next().filter(|character| matches!(character, '\'' | '"' | '`'))?;
        let title = &piece[1..];
        titles.push(title[..title.find(quote)?].to_string());
    }

    (!titles.is_empty()).then_some(titles)
}

struct WorkflowEnd {
    state: &'static str,
    ended_at: Option<String>,
    reason: Option<String>,
}

fn text_between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let rest = text.split_once(open)?.1;

    Some(&rest[..rest.find(close)?])
}

fn workflow_end(journal: &Path, run_id: &str) -> Option<WorkflowEnd> {
    let session_file = journal.parent()?.parent()?.parent()?.parent()?.with_extension("jsonl");
    let text = std::fs::read_to_string(session_file).ok()?;
    let marker = format!("Run ID: {run_id}");
    let launched = text.lines().find(|line| line.contains(&marker) && line.contains("Task ID: "))?;
    let task_id: String = launched.split_once("Task ID: ")?.1.chars().take_while(|character| character.is_ascii_alphanumeric()).collect();
    let task_marker = format!("<task-id>{task_id}</task-id>");
    let notification = text.lines().rev().find(|line| line.contains(&task_marker) && line.contains("<status>"))?;
    let state = match text_between(notification, "<status>", "</status>")? {
        "completed" => "completed",
        "failed" => "failed",
        "killed" | "stopped" => "stopped",
        _ => return None,
    };
    let ended_at = serde_json::from_str::<Value>(notification).ok().and_then(|value| value["timestamp"].as_str().map(str::to_string));
    let reason = text_between(notification, "<summary>", "</summary>").map(str::to_string).filter(|summary| !summary.is_empty());

    Some(WorkflowEnd { state, ended_at, reason })
}

fn find_jsonl_files(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();

        if file_type.is_dir() {
            find_jsonl_files(&path, files);
        } else if file_type.is_file() && path.extension().is_some_and(|extension| extension == "jsonl") {
            files.push(path);
        }
    }
}

pub fn locate(home: &Path, roots: &[PathBuf], session_id: &str) -> Option<(String, u64)> {
    let mut files = Vec::new();

    for root in roots {
        find_jsonl_files(root, &mut files);
    }

    files.sort();

    files.into_iter().find_map(|path| {
        let stem = path.file_stem()?.to_str()?;

        if stem != session_id && !stem.ends_with(&format!("-{session_id}")) {
            return None;
        }

        let relative = path.strip_prefix(home).ok()?.display().to_string();

        Some((relative, std::fs::metadata(&path).ok()?.len()))
    })
}

pub fn adopt(home: &Path, roots: &[PathBuf], target_root: &Path, session_id: &str) -> Result<bool> {
    let Some((relative, _)) = locate(home, roots, session_id) else {
        return Ok(false);
    };
    let source = home.join(&relative);

    if source.starts_with(target_root) {
        return Ok(false);
    }

    let Some(root) = roots.iter().find(|root| source.starts_with(root)) else {
        return Ok(false);
    };
    let inside_root = source.strip_prefix(root)?;
    let destination = target_root.join(inside_root);
    let source_length = std::fs::metadata(&source)?.len();
    let existing_length = std::fs::metadata(&destination).map(|metadata| metadata.len()).unwrap_or(0);

    if existing_length >= source_length {
        return Ok(false);
    }

    let parent = destination.parent().ok_or_else(|| anyhow!("The transcript has no parent folder."))?;
    std::fs::create_dir_all(parent)?;
    std::fs::copy(&source, &destination)?;

    Ok(true)
}

pub fn restore(home: &Path, roots: &[PathBuf], path: &str, offset: i64, data: &str) -> Result<()> {
    let relative = Path::new(path);
    let plain = relative.components().all(|component| matches!(component, std::path::Component::Normal(_)));
    let full = home.join(relative);
    let root = roots.iter().find(|root| full.starts_with(root));
    let in_a_transcript_folder = root.is_some() && path.ends_with(".jsonl");

    if !plain || !in_a_transcript_folder || offset < 0 {
        bail!("{path} is not a place this node keeps transcripts.");
    }

    let bytes = STANDARD.decode(data).map_err(|_| anyhow!("A transcript chunk was not valid base64."))?;
    let write = || -> std::io::Result<()> {
        use std::io::Write;

        let root = root.ok_or_else(|| std::io::Error::other("The transcript folder is not configured."))?;
        std::fs::create_dir_all(root)?;
        let root = root.canonicalize()?;
        let parent = full.parent().ok_or_else(|| std::io::Error::other("The transcript has no parent folder."))?;
        let mut existing = parent;

        while !existing.exists() {
            existing = existing.parent().ok_or_else(|| std::io::Error::other("The transcript folder is not available."))?;
        }

        if !existing.canonicalize()?.starts_with(&root) || (full.exists() && !full.canonicalize()?.starts_with(&root)) {
            return Err(std::io::Error::other("The transcript path is outside its configured folder."));
        }

        std::fs::create_dir_all(parent)?;
        let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(&full)?;
        file.set_len(offset as u64)?;
        file.seek(SeekFrom::Start(offset as u64))?;
        file.write_all(&bytes)
    };

    write().map_err(|cause| anyhow!("Could not write {path}: {cause}."))
}

fn workflow_journal(root: &Path, path: &Path) -> Option<WorkflowJournal> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Vec<&str> = relative.iter().filter_map(|part| part.to_str()).collect();

    match parts.as_slice() {
        [_project, session_id, "subagents", "workflows", run_id, "journal.jsonl"] => Some(WorkflowJournal { session_id: session_id.to_string(), run_id: run_id.to_string() }),
        _ => None,
    }
}

impl TranscriptFollower {
    pub fn new(home: PathBuf, roots: Vec<PathBuf>, state_file: PathBuf, out: mpsc::UnboundedSender<NodeToControl>) -> TranscriptFollower {
        let acked = std::fs::read_to_string(&state_file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();

        TranscriptFollower { home, roots, state_file, out, acked, sent: HashMap::new(), workflow_positions: HashMap::new(), workflows_scanned: false, running_agents: HashMap::new(), planned: HashSet::new(), ended: HashSet::new(), session_lengths: HashMap::new(), agent_quiet_limit: AGENT_QUIET_LIMIT, dirty: false, chunks_left: CHUNKS_PER_POLL }
    }

    pub fn on_connect(&mut self) {
        self.sent.clear();
    }

    pub fn ack(&mut self, path: &str, bytes: i64) {
        self.acked.insert(path.to_string(), bytes);
        let sent = self.sent.entry(path.to_string()).or_insert(0);
        *sent = (*sent).max(bytes);
        self.dirty = true;
    }

    pub fn poll(&mut self, connected: bool, conversation_for_session: impl Fn(&str) -> Option<Uuid>) {
        let roots = self.roots.clone();
        self.chunks_left = CHUNKS_PER_POLL;
        let mut known_workflows = HashSet::new();

        for root in &roots {
            let mut files = Vec::new();
            find_jsonl_files(root, &mut files);
            files.sort();

            for path in files {
                if let Some(journal) = workflow_journal(root, &path) {
                    known_workflows.insert(path.display().to_string());
                    self.follow_workflow(&path, &journal, connected, &conversation_for_session);
                }

                if connected {
                    self.send_new_bytes(&path);
                }
            }
        }

        self.workflows_scanned = true;
        self.persist();
    }

    fn send_new_bytes(&mut self, path: &Path) {
        let Ok(relative) = path.strip_prefix(&self.home) else {
            return;
        };
        let key = relative.display().to_string();
        let Ok(length) = std::fs::metadata(path).map(|metadata| metadata.len() as i64) else {
            return;
        };

        let acked = self.acked.get(&key).copied().unwrap_or(0);
        let mut position = self.sent.get(&key).copied().unwrap_or(acked);
        if length < position {
            position = 0;
            self.acked.insert(key.clone(), 0);
            self.dirty = true;
        }

        for _ in 0..CHUNKS_PER_FILE_PER_POLL {
            if position >= length || self.chunks_left == 0 {
                break;
            }

            let Some(chunk) = read_chunk(path, position as u64) else {
                break;
            };

            let _ = self.out.send(NodeToControl::Transcript { path: key.clone(), offset: position, data: STANDARD.encode(&chunk), size: Some(length) });
            position += chunk.len() as i64;
            self.chunks_left -= 1;
        }

        self.sent.insert(key, position);
    }

    fn follow_workflow(&mut self, path: &Path, journal: &WorkflowJournal, connected: bool, conversation_for_session: &impl Fn(&str) -> Option<Uuid>) {
        let key = path.display().to_string();
        let Ok(length) = std::fs::metadata(path).map(|metadata| metadata.len()) else {
            return;
        };

        if !self.workflow_positions.contains_key(&key) && !self.workflows_scanned {
            self.workflow_positions.insert(key, length);

            return;
        }

        if !connected {
            return;
        }

        let position = self.workflow_positions.get(&key).copied().unwrap_or(0).min(length);
        let Some(bytes) = read_chunk_from(path, position, length - position) else {
            return;
        };

        let complete = match bytes.iter().rposition(|byte| *byte == b'\n') {
            Some(last_newline) => last_newline + 1,
            None => 0,
        };
        self.workflow_positions.insert(key.clone(), position + complete as u64);

        let Some(conversation_id) = conversation_for_session(&journal.session_id) else {
            return;
        };

        if self.planned.insert(key.clone()) {
            if let Some(phases) = planned_phases(path, &journal.run_id) {
                let _ = self.out.send(NodeToControl::Workflow { conversation_id, run_id: journal.run_id.clone(), line: serde_json::json!({"type": "plan", "phases": phases}) });
            }
        }

        let running = self.running_agents.entry(key.clone()).or_default();

        for line in String::from_utf8_lossy(&bytes[..complete]).lines().filter(|line| !line.trim().is_empty()) {
            let mut line: Value = serde_json::from_str(line).unwrap_or_else(|_| Value::String(line.to_string()));
            let agent_id = line["agentId"].as_str().filter(|id| is_agent_id(id)).map(str::to_string);

            match (line["type"].as_str(), agent_id, line["key"].as_str()) {
                (Some("started"), Some(agent_id), Some(agent_key)) => {
                    running.insert(agent_id, RunningAgent { key: agent_key.to_string(), sent_at: None, changed_at: std::time::Instant::now(), stats: serde_json::Map::new() });
                }
                (Some("result"), Some(agent_id), _) => {
                    running.remove(&agent_id);

                    if let Some(fields) = line.as_object_mut() {
                        for (field, value) in agent_stats(path, &agent_id) {
                            fields.entry(field).or_insert(value);
                        }
                    }
                }
                (Some("failed"), Some(agent_id), _) => {
                    running.remove(&agent_id);
                }
                _ => {}
            }

            let _ = self.out.send(NodeToControl::Workflow { conversation_id, run_id: journal.run_id.clone(), line });
        }

        let mut stopped = Vec::new();

        for (agent_id, agent) in running.iter_mut() {
            if agent.sent_at.is_some_and(|sent_at| sent_at.elapsed() < PROGRESS_EVERY) {
                continue;
            }

            agent.sent_at = Some(std::time::Instant::now());
            let stats = agent_stats(path, agent_id);

            if stats.is_empty() || stats == agent.stats {
                if agent.changed_at.elapsed() >= self.agent_quiet_limit {
                    stopped.push(agent_id.clone());
                    let line = serde_json::json!({"type": "failed", "key": agent.key, "agentId": agent_id, "reason": "Stopped responding"});
                    let _ = self.out.send(NodeToControl::Workflow { conversation_id, run_id: journal.run_id.clone(), line });
                }

                continue;
            }

            agent.changed_at = std::time::Instant::now();

            let mut line = serde_json::json!({"type": "progress", "key": agent.key, "agentId": agent_id});

            if let Some(fields) = line.as_object_mut() {
                fields.extend(stats.clone());
            }

            agent.stats = stats;
            let _ = self.out.send(NodeToControl::Workflow { conversation_id, run_id: journal.run_id.clone(), line });
        }

        for agent_id in stopped {
            running.remove(&agent_id);
        }

        self.report_end(&key, path, &journal.run_id, conversation_id);
    }

    fn report_end(&mut self, key: &str, journal: &Path, run_id: &str, conversation_id: Uuid) {
        if self.ended.contains(key) {
            return;
        }

        let Some(session_length) = journal.parent().and_then(Path::parent).and_then(Path::parent).and_then(Path::parent).and_then(|session| std::fs::metadata(session.with_extension("jsonl")).ok()).map(|metadata| metadata.len()) else {
            return;
        };

        if self.session_lengths.insert(key.to_string(), session_length) == Some(session_length) {
            return;
        }

        let Some(end) = workflow_end(journal, run_id) else {
            return;
        };

        self.ended.insert(key.to_string());
        let last_line_at = std::fs::metadata(journal).and_then(|metadata| metadata.modified()).ok().map(|modified| chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339());
        let line = serde_json::json!({"type": "ended", "state": end.state, "ended_at": end.ended_at, "reason": end.reason, "last_line_at": last_line_at});
        let _ = self.out.send(NodeToControl::Workflow { conversation_id, run_id: run_id.to_string(), line });
    }

    fn persist(&mut self) {
        if !self.dirty {
            return;
        }

        self.dirty = false;
        let Ok(text) = serde_json::to_string(&self.acked) else {
            return;
        };

        if let Some(parent) = self.state_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if let Err(error) = std::fs::write(&self.state_file, text) {
            tracing::warn!("could not record transcript offsets: {error}");
        }
    }
}

fn read_chunk(path: &Path, position: u64) -> Option<Vec<u8>> {
    read_chunk_from(path, position, CHUNK_BYTES)
}

fn read_chunk_from(path: &Path, position: u64, wanted: u64) -> Option<Vec<u8>> {
    let mut file = std::fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(position)).ok()?;

    let mut buffer = Vec::new();
    file.take(wanted.min(CHUNK_BYTES * 16)).read_to_end(&mut buffer).ok()?;

    Some(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn follower(home: &Path, root: &Path, state: &Path) -> (TranscriptFollower, mpsc::UnboundedReceiver<NodeToControl>) {
        let (out, messages) = mpsc::unbounded_channel();

        (TranscriptFollower::new(home.to_path_buf(), vec![root.to_path_buf()], state.to_path_buf(), out), messages)
    }

    fn drain(messages: &mut mpsc::UnboundedReceiver<NodeToControl>) -> Vec<NodeToControl> {
        let mut drained = Vec::new();
        while let Ok(message) = messages.try_recv() {
            drained.push(message);
        }

        drained
    }

    fn append(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    fn transcript_data(message: &NodeToControl) -> (i64, String) {
        let NodeToControl::Transcript { offset, data, .. } = message else {
            panic!("expected a transcript message, got {message:?}");
        };

        (*offset, String::from_utf8(STANDARD.decode(data).unwrap()).unwrap())
    }

    #[test]
    fn a_session_is_found_by_its_id_in_claude_and_codex_transcript_folders() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join(".claude/projects");
        let codex = dir.path().join(".codex/sessions");
        append(&claude.join("-work-app").join("abc-123.jsonl"), "one\n");
        append(&codex.join("2026/10/23").join("rollout-2026-10-23T10-00-00-def-456.jsonl"), "four\n");
        append(&claude.join("-work-app").join("other.jsonl"), "x\n");
        let roots = [claude, codex];

        assert_eq!(locate(dir.path(), &roots, "abc-123"), Some((".claude/projects/-work-app/abc-123.jsonl".to_string(), 4)));
        assert_eq!(locate(dir.path(), &roots, "def-456"), Some((".codex/sessions/2026/10/23/rollout-2026-10-23T10-00-00-def-456.jsonl".to_string(), 5)));
        assert_eq!(locate(dir.path(), &roots, "missing"), None);
    }

    #[test]
    fn adopting_copies_a_claude_transcript_into_the_other_accounts_projects_folder() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join(".claude/projects");
        let second = dir.path().join("accounts/claude/work/projects");
        append(&first.join("-work-app").join("abc-123.jsonl"), "one\n");
        let roots = [first, second.clone()];

        assert_eq!(adopt(dir.path(), &roots, &second, "abc-123").unwrap(), true);
        assert_eq!(std::fs::read_to_string(second.join("-work-app/abc-123.jsonl")).unwrap(), "one\n");
    }

    #[test]
    fn adopting_keeps_the_codex_date_folders() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join(".codex/sessions");
        let second = dir.path().join("accounts/codex/work/sessions");
        append(&first.join("2026/10/23").join("rollout-2026-10-23T10-00-00-def-456.jsonl"), "four\n");
        let roots = [first, second.clone()];

        assert_eq!(adopt(dir.path(), &roots, &second, "def-456").unwrap(), true);
        assert!(second.join("2026/10/23/rollout-2026-10-23T10-00-00-def-456.jsonl").is_file());
    }

    #[test]
    fn adopting_a_transcript_already_in_the_account_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join(".claude/projects");
        let second = dir.path().join("accounts/claude/work/projects");
        append(&first.join("-work-app").join("abc-123.jsonl"), "one\n");
        append(&second.join("-work-app").join("abc-123.jsonl"), "one\ntwo\n");
        let roots = [first, second.clone()];

        assert_eq!(adopt(dir.path(), &roots, &second, "abc-123").unwrap(), false);
        assert_eq!(std::fs::read_to_string(second.join("-work-app/abc-123.jsonl")).unwrap(), "one\ntwo\n");
    }

    #[test]
    fn a_managed_accounts_transcript_can_be_restored_only_within_its_configured_root() {
        let home = tempfile::tempdir().unwrap();
        let folder = ".local/state/mycelium/accounts/codex/account-test/sessions";
        let root = home.path().join(folder);
        let path = format!("{folder}/2026/10/24/session.jsonl");
        restore(home.path(), &[root], &path, 0, &STANDARD.encode("reported usage\n")).unwrap();
        assert_eq!(std::fs::read_to_string(home.path().join(path)).unwrap(), "reported usage\n");
    }

    #[cfg(unix)]
    #[test]
    fn restoring_a_transcript_does_not_follow_a_symlink_outside_the_configured_root() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = home.path().join(".claude/projects");
        std::fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("escape")).unwrap();
        assert!(restore(home.path(), &[root], ".claude/projects/escape/session.jsonl", 0, &STANDARD.encode("example\n")).is_err());
        assert!(!outside.path().join("session.jsonl").exists());
    }

    #[test]
    fn restored_chunks_rebuild_the_transcript_and_a_restart_overwrites_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = ".claude/projects/-w-app/abc.jsonl";

        restore(dir.path(), &[dir.path().join(".claude/projects"), dir.path().join(".codex/sessions")], path, 0, &STANDARD.encode("one\n")).unwrap();
        restore(dir.path(), &[dir.path().join(".claude/projects"), dir.path().join(".codex/sessions")], path, 4, &STANDARD.encode("two\n")).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join(path)).unwrap(), "one\ntwo\n");

        restore(dir.path(), &[dir.path().join(".claude/projects"), dir.path().join(".codex/sessions")], path, 0, &STANDARD.encode("new\n")).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join(path)).unwrap(), "new\n");
    }

    #[test]
    fn a_transcript_outside_the_agent_folders_or_climbing_out_of_them_is_refused() {
        let dir = tempfile::tempdir().unwrap();

        for path in ["../escape.jsonl", ".claude/projects/../../escape.jsonl", "/etc/passwd.jsonl", ".ssh/authorized_keys", ".claude/projects/x/notes.txt"] {
            assert!(restore(dir.path(), &[dir.path().join(".claude/projects"), dir.path().join(".codex/sessions")], path, 0, &STANDARD.encode("x")).is_err(), "{path}");
        }

        assert!(!dir.path().join("escape.jsonl").exists());
    }

    #[test]
    fn new_bytes_are_sent_from_the_last_sent_offset() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("projects").join("p").join("s.jsonl");
        let (mut follower, mut messages) = follower(dir.path(), &dir.path().join("projects"), &dir.path().join("state.json"));

        append(&file, "one\n");
        follower.poll(true, |_| None);
        append(&file, "two\n");
        follower.poll(true, |_| None);
        follower.poll(true, |_| None);

        let sent = drain(&mut messages);
        assert_eq!(sent.len(), 2);
        assert_eq!(transcript_data(&sent[0]), (0, "one\n".to_string()));
        assert_eq!(transcript_data(&sent[1]), (4, "two\n".to_string()));
    }

    #[test]
    fn after_a_reconnect_sending_resumes_from_the_acked_offset() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("projects").join("p").join("s.jsonl");
        let (mut follower, mut messages) = follower(dir.path(), &dir.path().join("projects"), &dir.path().join("state.json"));
        append(&file, "one\ntwo\n");
        follower.poll(true, |_| None);
        follower.ack("projects/p/s.jsonl", 4);
        drain(&mut messages);

        follower.on_connect();
        follower.poll(true, |_| None);

        assert_eq!(transcript_data(&drain(&mut messages)[0]), (4, "two\n".to_string()));
    }

    #[test]
    fn acked_offsets_survive_a_daemon_restart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("projects").join("p").join("s.jsonl");
        let state = dir.path().join("state.json");
        append(&file, "one\ntwo\n");
        let (mut first, _messages) = follower(dir.path(), &dir.path().join("projects"), &state);
        first.ack("projects/p/s.jsonl", 4);
        first.poll(false, |_| None);

        let (mut second, mut messages) = follower(dir.path(), &dir.path().join("projects"), &state);
        second.poll(true, |_| None);

        assert_eq!(transcript_data(&drain(&mut messages)[0]), (4, "two\n".to_string()));
    }

    #[test]
    fn a_backlog_of_many_files_is_sent_over_several_polls_and_all_of_it_arrives() {
        let dir = tempfile::tempdir().unwrap();
        let (mut follower, mut messages) = follower(dir.path(), &dir.path().join("projects"), &dir.path().join("state.json"));

        for index in 0..40 {
            append(&dir.path().join("projects").join("p").join(format!("{index:02}.jsonl")), "line\n");
        }

        follower.poll(true, |_| None);
        let first = drain(&mut messages).len();
        assert_eq!(first, CHUNKS_PER_POLL);

        let mut total = first;

        for _ in 0..4 {
            follower.poll(true, |_| None);
            total += drain(&mut messages).len();
        }

        assert_eq!(total, 40);
    }

    #[test]
    fn transcript_paths_are_sent_relative_to_the_home_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("projects").join("p").join("s.jsonl");
        let (mut follower, mut messages) = follower(dir.path(), &dir.path().join("projects"), &dir.path().join("state.json"));
        append(&file, "one\n");

        follower.poll(true, |_| None);

        let sent = drain(&mut messages);
        assert!(matches!(&sent[0], NodeToControl::Transcript { path, .. } if path == "projects/p/s.jsonl"));
    }

    #[test]
    fn nothing_is_sent_while_disconnected() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("projects").join("p").join("s.jsonl");
        let (mut follower, mut messages) = follower(dir.path(), &dir.path().join("projects"), &dir.path().join("state.json"));
        append(&file, "one\n");

        follower.poll(false, |_| None);

        assert!(drain(&mut messages).is_empty());
    }

    #[test]
    fn a_truncated_file_is_sent_again_from_the_start() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("projects").join("p").join("s.jsonl");
        let (mut follower, mut messages) = follower(dir.path(), &dir.path().join("projects"), &dir.path().join("state.json"));
        append(&file, "a long first line\n");
        follower.poll(true, |_| None);
        drain(&mut messages);

        std::fs::write(&file, "new\n").unwrap();
        follower.poll(true, |_| None);

        assert_eq!(transcript_data(&drain(&mut messages)[0]), (0, "new\n".to_string()));
    }

    #[test]
    fn workflow_lines_go_to_the_conversation_that_owns_the_session_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("projects");
        let journal = root.join("proj").join("session-9").join("subagents").join("workflows").join("run-1").join("journal.jsonl");
        let conversation = Uuid::from_u128(5);
        let (mut follower, mut messages) = follower(dir.path(), &root, &dir.path().join("state.json"));
        append(&journal, "{\"old\":true}\n");
        follower.poll(true, |_| Some(conversation));
        drain(&mut messages);

        append(&journal, "{\"step\":1}\n{\"step\":2}\n{\"partial\"");
        follower.poll(true, |session| (session == "session-9").then_some(conversation));

        let workflow: Vec<NodeToControl> = drain(&mut messages).into_iter().filter(|message| matches!(message, NodeToControl::Workflow { .. })).collect();
        let lines: Vec<Value> = workflow.iter().filter_map(|message| if let NodeToControl::Workflow { line, .. } = message { Some(line.clone()) } else { None }).collect();
        assert_eq!(lines, [serde_json::json!({"step": 1}), serde_json::json!({"step": 2})]);
        assert!(workflow.iter().all(|message| matches!(message, NodeToControl::Workflow { conversation_id, run_id, .. } if *conversation_id == conversation && run_id == "run-1")));
    }

    fn workflow_with_session_lines(session_lines: &[Value]) -> Vec<Value> {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("projects");
        let session = root.join("proj").join("session-9");
        let run = session.join("subagents").join("workflows").join("run-1");
        let conversation = Uuid::from_u128(5);
        let (mut follower, mut messages) = follower(dir.path(), &root, &dir.path().join("state.json"));
        append(&run.join("journal.jsonl"), "{\"type\":\"launched\"}\n");
        append(&session.join("workflows").join("scripts").join("plan-run-1.js"), "meta({ phases: [{ title: 'Implement' }, { title: 'Verify' }] })");
        follower.poll(true, |_| Some(conversation));
        drain(&mut messages);

        for line in session_lines {
            append(&session.with_extension("jsonl"), &format!("{line}\n"));
        }

        append(&run.join("journal.jsonl"), "{\"type\":\"noted\"}\n");
        follower.poll(true, |_| Some(conversation));

        drain(&mut messages).into_iter().filter_map(|message| if let NodeToControl::Workflow { line, .. } = message { Some(line) } else { None }).collect()
    }

    fn launch_line() -> Value {
        serde_json::json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": "Workflow launched in background. Task ID: w9qw46stl\nRun ID: run-1\n"}]}})
    }

    fn notification_line(status: &str, summary: &str) -> Value {
        serde_json::json!({"type": "queue-operation", "operation": "enqueue", "timestamp": "2026-10-27T00:42:10.000Z", "content": format!("<task-notification>\n<task-id>w9qw46stl</task-id>\n<status>{status}</status>\n<summary>{summary}</summary>\n</task-notification>")})
    }

    #[test]
    fn a_stopped_workflow_with_no_agents_reports_stopped() {
        let lines = workflow_with_session_lines(&[launch_line(), notification_line("killed", "Workflow was stopped")]);
        let ended: Vec<&Value> = lines.iter().filter(|line| line["type"] == "ended").collect();

        assert!(lines.iter().all(|line| line["type"] != "started"));
        assert_eq!(ended.len(), 1);
        assert_eq!((ended[0]["state"].as_str(), ended[0]["ended_at"].as_str(), ended[0]["reason"].as_str()), (Some("stopped"), Some("2026-10-27T00:42:10.000Z"), Some("Workflow was stopped")));
        assert!(ended[0]["last_line_at"].is_string());
    }

    #[test]
    fn a_failed_workflow_reports_failed_with_the_reason() {
        let lines = workflow_with_session_lines(&[launch_line(), notification_line("failed", "Weekly limit reached")]);

        assert!(lines.iter().any(|line| line["type"] == "ended" && line["state"] == "failed" && line["reason"] == "Weekly limit reached"));
    }

    #[test]
    fn a_running_workflow_reports_no_end() {
        let lines = workflow_with_session_lines(&[launch_line()]);

        assert!(lines.iter().all(|line| line["type"] != "ended"));
    }

    fn finished_agent_lines(root: &Path, run_files: &[(&str, &str)], journal_line: &str) -> Vec<Value> {
        let dir = root.parent().unwrap();
        let run = root.join("proj").join("session-9").join("subagents").join("workflows").join("run-1");
        let conversation = Uuid::from_u128(5);
        let (mut follower, mut messages) = follower(dir, root, &dir.join("state.json"));
        append(&run.join("journal.jsonl"), "{\"old\":true}\n");
        follower.poll(true, |_| Some(conversation));
        drain(&mut messages);

        for (name, content) in run_files {
            append(&run.join(name), content);
        }

        append(&run.join("journal.jsonl"), journal_line);
        follower.poll(true, |_| Some(conversation));

        drain(&mut messages).into_iter().filter_map(|message| if let NodeToControl::Workflow { line, .. } = message { Some(line) } else { None }).collect()
    }

    fn agent_transcript() -> String {
        [
            serde_json::json!({"type": "user", "timestamp": "2026-10-02T10:00:00.000Z", "message": {"role": "user", "content": "go"}}),
            serde_json::json!({"type": "assistant", "timestamp": "2026-10-02T10:00:05.000Z", "message": {"id": "m1", "usage": {"input_tokens": 2, "output_tokens": 8, "cache_creation_input_tokens": 100, "cache_read_input_tokens": 5000}}}),
            serde_json::json!({"type": "assistant", "timestamp": "2026-10-02T10:00:06.000Z", "message": {"id": "m1", "usage": {"input_tokens": 2, "output_tokens": 40, "cache_creation_input_tokens": 100, "cache_read_input_tokens": 5000}}}),
            serde_json::json!({"type": "user", "timestamp": "2026-10-02T10:00:20.000Z", "message": {"role": "user", "content": "tool result"}}),
            serde_json::json!({"type": "assistant", "timestamp": "2026-10-02T10:01:30.500Z", "message": {"id": "m2", "usage": {"input_tokens": 3, "output_tokens": 10, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 9000}}}),
        ]
        .iter()
        .map(|row| format!("{row}\n"))
        .collect()
    }

    #[test]
    fn a_finished_agent_line_carries_the_tokens_turns_and_elapsed_time_of_its_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("projects");

        let lines = finished_agent_lines(&root, &[("agent-a1.jsonl", &agent_transcript())], "{\"type\":\"result\",\"key\":\"k1\",\"agentId\":\"a1\",\"result\":\"done\"}\n");

        assert_eq!(lines[0]["tokens"], 2 + 40 + 100 + 3 + 10);
        assert_eq!(lines[0]["turns"], 2);
        assert_eq!(lines[0]["elapsed_ms"], 90_500);
        assert_eq!(lines[0]["result"], "done");
    }

    #[test]
    fn a_running_agent_reports_its_progress_and_the_runs_phases_are_sent_from_its_script() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("projects");
        let session = root.join("proj").join("session-9");
        append(&session.join("workflows").join("scripts").join("build-run-1.js"), "export const meta = {\n  name: 'build',\n  phases: [{ title: 'Write', detail: 'code' }, { title: \"Verify\" }],\n}\nphase('Write')\n");

        let lines = finished_agent_lines(&root, &[("agent-a1.jsonl", &agent_transcript())], "{\"type\":\"started\",\"key\":\"k1\",\"agentId\":\"a1\",\"label\":\"write\",\"phase\":\"Write\"}\n");

        assert_eq!(lines[0], serde_json::json!({"type": "plan", "phases": ["Write", "Verify"]}));
        assert_eq!(lines[1]["type"], "started");
        assert_eq!((lines[2]["type"].clone(), lines[2]["key"].clone(), lines[2]["tokens"].clone(), lines[2]["turns"].clone()), (serde_json::json!("progress"), serde_json::json!("k1"), serde_json::json!(2 + 40 + 100 + 3 + 10), serde_json::json!(2)));
    }

    #[test]
    fn a_running_agent_whose_transcript_stops_changing_is_reported_as_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("projects");
        let run = root.join("proj").join("session-9").join("subagents").join("workflows").join("run-1");
        let conversation = Uuid::from_u128(5);
        let (mut follower, mut messages) = follower(dir.path(), &root, &dir.path().join("state.json"));
        follower.agent_quiet_limit = std::time::Duration::ZERO;
        append(&run.join("journal.jsonl"), "{\"old\":true}\n");
        follower.poll(true, |_| Some(conversation));
        append(&run.join("agent-a1.jsonl"), &agent_transcript());
        append(&run.join("journal.jsonl"), "{\"type\":\"started\",\"key\":\"k1\",\"agentId\":\"a1\"}\n");
        follower.poll(true, |_| Some(conversation));
        drain(&mut messages);

        for running in follower.running_agents.values_mut().flat_map(|agents| agents.values_mut()) {
            running.sent_at = None;
        }

        follower.poll(true, |_| Some(conversation));

        let lines: Vec<Value> = drain(&mut messages).into_iter().filter_map(|message| if let NodeToControl::Workflow { line, .. } = message { Some(line) } else { None }).collect();
        assert_eq!(lines, [serde_json::json!({"type": "failed", "key": "k1", "agentId": "a1", "reason": "Stopped responding"})]);
    }

    #[test]
    fn a_transcript_beside_the_subagents_folder_is_read_when_the_run_folder_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("projects");
        let session = root.join("proj").join("session-9").join("subagents");
        append(&session.join("agent-a1.jsonl"), &agent_transcript());

        let lines = finished_agent_lines(&root, &[], "{\"type\":\"result\",\"key\":\"k1\",\"agentId\":\"a1\"}\n");

        assert_eq!(lines[0]["turns"], 2);
    }

    #[test]
    fn a_finished_agent_without_a_transcript_and_other_lines_are_forwarded_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("projects");

        let lines = finished_agent_lines(&root, &[], "{\"type\":\"result\",\"key\":\"k1\",\"agentId\":\"missing\"}\n{\"type\":\"started\",\"key\":\"k2\",\"agentId\":\"a1\"}\n");

        assert_eq!(lines, [serde_json::json!({"type": "result", "key": "k1", "agentId": "missing"}), serde_json::json!({"type": "started", "key": "k2", "agentId": "a1"})]);
    }

    #[test]
    fn an_agent_id_that_climbs_out_of_the_run_folder_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("projects");

        let lines = finished_agent_lines(&root, &[], "{\"type\":\"result\",\"key\":\"k1\",\"agentId\":\"../../x\"}\n");

        assert!(lines[0].get("turns").is_none());
    }

    #[test]
    fn journal_lines_written_before_the_daemon_started_are_not_replayed_as_workflow_messages() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("projects");
        let journal = root.join("proj").join("session-9").join("subagents").join("workflows").join("run-1").join("journal.jsonl");
        append(&journal, "{\"old\":true}\n");
        let (mut follower, mut messages) = follower(dir.path(), &root, &dir.path().join("state.json"));

        follower.poll(true, |_| Some(Uuid::from_u128(5)));

        assert!(drain(&mut messages).iter().all(|message| !matches!(message, NodeToControl::Workflow { .. })));
    }
}
