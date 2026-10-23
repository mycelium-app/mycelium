use mycelium_proto::api::WalkthroughFile;
use std::collections::HashSet;

pub struct Hunk {
    pub header: String,
    pub text: String,
}

pub struct FileChange {
    pub path: String,
    pub hunks: Vec<Hunk>,
}

fn is_changed_line(line: &str) -> bool {
    (line.starts_with('+') && !line.starts_with("+++")) || (line.starts_with('-') && !line.starts_with("---"))
}

fn range_of(header: &str) -> &str {
    header.trim_start_matches('@').trim_start().split(" @@").next().unwrap_or_default().trim()
}

fn content_of(line: &str) -> &str {
    line[1..].trim()
}

fn close_file_without_hunks(changes: &mut [FileChange], loose: &mut Vec<&str>) {
    if let Some(last) = changes.last_mut().filter(|last| last.hunks.is_empty() && !loose.is_empty()) {
        last.hunks.push(Hunk { header: String::new(), text: loose.join("\n") });
    }

    loose.clear();
}

pub fn parse(diff: &str) -> Vec<FileChange> {
    let mut changes: Vec<FileChange> = Vec::new();
    let mut loose: Vec<&str> = Vec::new();

    for line in diff.lines() {
        if let Some(names) = line.strip_prefix("diff --git ") {
            close_file_without_hunks(&mut changes, &mut loose);
            let path = names.rsplit_once(" b/").map(|(_, path)| path).unwrap_or(names).to_string();
            changes.push(FileChange { path, hunks: Vec::new() });
        } else if line.starts_with("@@") {
            if let Some(last) = changes.last_mut() {
                last.hunks.push(Hunk { header: line.to_string(), text: line.to_string() });
            }
        } else if let Some(hunk) = changes.last_mut().and_then(|last| last.hunks.last_mut()) {
            hunk.text.push('\n');
            hunk.text.push_str(line);
        } else if let Some(last) = changes.last() {
            if last.hunks.is_empty() && (line.starts_with("Binary files") || line.starts_with("GIT binary patch")) {
                loose.push(line);
            }
        }
    }

    close_file_without_hunks(&mut changes, &mut loose);
    changes
}

pub fn matching(change: &FileChange, agent_text: &str) -> Vec<usize> {
    let everything = || (0..change.hunks.len()).collect::<Vec<_>>();

    if agent_text.trim().is_empty() {
        return everything();
    }

    let ranges: HashSet<&str> = agent_text.lines().filter(|line| line.starts_with("@@")).map(range_of).collect();
    let lines: HashSet<&str> = agent_text.lines().filter(|line| is_changed_line(line)).map(|line| line.trim_end()).collect();
    let matched: Vec<usize> = change
        .hunks
        .iter()
        .enumerate()
        .filter(|(_, hunk)| {
            hunk.header.is_empty()
                || ranges.contains(range_of(&hunk.header))
                || hunk.text.lines().any(|line| is_changed_line(line) && !content_of(line).is_empty() && lines.contains(line.trim_end()))
        })
        .map(|(index, _)| index)
        .collect();

    if matched.is_empty() { everything() } else { matched }
}

pub fn file_for(change: Option<&FileChange>, path: &str, indexes: &[usize]) -> WalkthroughFile {
    let diff = change.map(|change| indexes.iter().map(|index| change.hunks[*index].text.as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default();

    WalkthroughFile { path: path.to_string(), unchanged: diff.is_empty(), diff }
}
