use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mycelium_proto::link::NodeToControl;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use uuid::Uuid;

const CHUNK_BYTES: u64 = 256 * 1024;
const CHUNKS_PER_FILE_PER_POLL: usize = 4;

pub struct TranscriptFollower {
    home: PathBuf,
    roots: Vec<PathBuf>,
    state_file: PathBuf,
    out: mpsc::UnboundedSender<NodeToControl>,
    acked: HashMap<String, i64>,
    sent: HashMap<String, i64>,
    workflow_positions: HashMap<String, u64>,
    workflows_scanned: bool,
    dirty: bool,
}

struct WorkflowJournal {
    session_id: String,
    run_id: String,
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

        TranscriptFollower { home, roots, state_file, out, acked, sent: HashMap::new(), workflow_positions: HashMap::new(), workflows_scanned: false, dirty: false }
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
            if position >= length {
                break;
            }

            let Some(chunk) = read_chunk(path, position as u64) else {
                break;
            };

            let _ = self.out.send(NodeToControl::Transcript { path: key.clone(), offset: position, data: STANDARD.encode(&chunk) });
            position += chunk.len() as i64;
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
        self.workflow_positions.insert(key, position + complete as u64);

        let Some(conversation_id) = conversation_for_session(&journal.session_id) else {
            return;
        };

        for line in String::from_utf8_lossy(&bytes[..complete]).lines().filter(|line| !line.trim().is_empty()) {
            let _ = self.out.send(NodeToControl::Workflow { conversation_id, seq: 0, run_id: journal.run_id.clone(), line: line.to_string() });
        }
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
        let lines: Vec<&str> = workflow.iter().map(|message| if let NodeToControl::Workflow { line, .. } = message { line.as_str() } else { "" }).collect();
        assert_eq!(lines, ["{\"step\":1}", "{\"step\":2}"]);
        assert!(workflow.iter().all(|message| matches!(message, NodeToControl::Workflow { conversation_id, run_id, .. } if *conversation_id == conversation && run_id == "run-1")));
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
