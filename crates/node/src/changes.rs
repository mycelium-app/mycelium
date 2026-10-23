use crate::git::{git, status};
use mycelium_proto::link::RowRange;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::UNIX_EPOCH;

const MOST_DIFF_BYTES: usize = 8 * 1024 * 1024;

static CACHE: LazyLock<Mutex<HashMap<PathBuf, HashMap<String, Cached>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone)]
struct Cached {
    fingerprint: String,
    flags: [bool; 3],
    rows: Arc<FileRows>,
}

#[derive(Default)]
struct FileRows {
    added: u32,
    removed: u32,
    rows: Vec<Row>,
    hunks: Vec<u32>,
    truncated: bool,
}

struct Row {
    kind: char,
    old: Option<u32>,
    new: Option<u32>,
    text: String,
}

pub fn layout(cwd: &Path) -> Result<Value, String> {
    let files = refresh(cwd)?;

    Ok(json!({"files": files.iter().map(|(path, cached)| {
        let rows = &cached.rows;

        json!({
            "path": path,
            "staged": cached.flags[0],
            "unstaged": cached.flags[1],
            "untracked": cached.flags[2],
            "added": rows.added,
            "removed": rows.removed,
            "rows": rows.rows.len(),
            "hunks": rows.hunks,
            "truncated": rows.truncated,
            "fingerprint": cached.fingerprint,
        })
    }).collect::<Vec<_>>()}))
}

pub fn rows(cwd: &Path, ranges: &[RowRange]) -> Result<Value, String> {
    let files: HashMap<String, (String, Arc<FileRows>)> = refresh(cwd)?.into_iter().map(|(path, cached)| (path, (cached.fingerprint, cached.rows))).collect();
    let mut answered = Vec::new();

    for range in ranges {
        let Some((fingerprint, file)) = files.get(&range.path) else {
            continue;
        };
        let from = (range.from as usize).min(file.rows.len());
        let to = (range.to as usize).clamp(from, file.rows.len());
        let rows: Vec<Value> = file.rows[from..to].iter().map(|row| json!({"k": row.kind.to_string(), "o": row.old, "n": row.new, "t": row.text})).collect();

        answered.push(json!({"path": range.path, "from": from, "fingerprint": fingerprint, "rows": rows}));
    }

    Ok(json!({"ranges": answered}))
}

fn refresh(cwd: &Path) -> Result<Vec<(String, Cached)>, String> {
    let current = status(cwd)?;
    let head = String::from_utf8_lossy(&git(cwd, &["rev-parse", "HEAD"], &[128]).unwrap_or_default()).trim().to_string();
    let index = stamp(&index_path(cwd));
    let mut cache = CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let held = cache.entry(cwd.to_path_buf()).or_default();
    let mut seen = Vec::new();
    let mut views = Vec::new();

    for file in current["files"].as_array().into_iter().flatten() {
        let Some(path) = file["path"].as_str() else {
            continue;
        };
        let flags = ["staged", "unstaged", "untracked"].map(|name| file[name].as_bool().unwrap_or(false));
        let fingerprint = format!(
            "{flags:?}:{}:{}:{}:{}:{}",
            file["additions"],
            file["deletions"],
            stamp(&cwd.join(path)),
            if flags[2] { String::new() } else { head.clone() },
            if flags[0] { index.clone() } else { String::new() },
        );

        if held.get(path).is_none_or(|cached| cached.fingerprint != fingerprint) {
            let rows = Arc::new(file_rows(cwd, path, flags)?);

            held.insert(path.to_string(), Cached { fingerprint, flags, rows });
        }

        seen.push(path.to_string());

        views.push((path.to_string(), held[path].clone()));
    }

    held.retain(|path, _| seen.contains(path));

    Ok(views)
}

fn index_path(cwd: &Path) -> PathBuf {
    let found = git(cwd, &["rev-parse", "--git-path", "index"], &[]).unwrap_or_default();

    cwd.join(String::from_utf8_lossy(&found).trim())
}

fn stamp(path: &Path) -> String {
    let Ok(metadata) = std::fs::metadata(path) else {
        return "gone".to_string();
    };
    let modified = metadata.modified().ok().and_then(|time| time.duration_since(UNIX_EPOCH).ok()).map(|elapsed| elapsed.as_nanos()).unwrap_or(0);

    format!("{modified}-{}", metadata.len())
}

fn file_rows(cwd: &Path, path: &str, flags: [bool; 3]) -> Result<FileRows, String> {
    let mut built = FileRows::default();

    if flags[2] {
        let bytes = std::fs::read(cwd.join(path)).unwrap_or_default();

        built.truncated = bytes.len() > MOST_DIFF_BYTES;

        let kept = &bytes[..bytes.len().min(MOST_DIFF_BYTES)];

        if let (false, Ok(text)) = (kept.contains(&0), std::str::from_utf8(kept)) {
            let lines: Vec<&str> = text.split_terminator('\n').collect();

            if !lines.is_empty() {
                built.hunks.push(0);
                built.rows.push(Row { kind: 'h', old: None, new: None, text: format!("@@ -0,0 +1,{} @@", lines.len()) });
            }

            for (index, line) in lines.iter().enumerate() {
                built.rows.push(Row { kind: 'a', old: None, new: Some(index as u32 + 1), text: line.to_string() });
            }

            built.added = lines.len() as u32;
        }

        return Ok(built);
    }

    for (cached, wanted) in [(true, flags[0]), (false, flags[1])] {
        if !wanted {
            continue;
        }

        let mut args = vec!["diff", "--no-color", "--no-ext-diff", "--no-renames"];

        if cached {
            args.push("--cached");
        }

        args.extend(["--", path]);

        let output = git(cwd, &args, &[])?;

        built.truncated |= output.len() > MOST_DIFF_BYTES;
        parse(&String::from_utf8_lossy(&output[..output.len().min(MOST_DIFF_BYTES)]), &mut built);
    }

    Ok(built)
}

fn parse(text: &str, into: &mut FileRows) {
    let (mut old, mut new) = (0u32, 0u32);
    let mut in_hunk = false;

    for raw in text.lines() {
        if let Some(numbers) = hunk_numbers(raw) {
            (old, new) = numbers;
            in_hunk = true;
            into.hunks.push(into.rows.len() as u32);
            into.rows.push(Row { kind: 'h', old: None, new: None, text: raw.to_string() });
        } else if !in_hunk || raw.starts_with('\\') {
            continue;
        } else if let Some(line) = raw.strip_prefix('+') {
            into.rows.push(Row { kind: 'a', old: None, new: Some(new), text: line.to_string() });
            new += 1;
            into.added += 1;
        } else if let Some(line) = raw.strip_prefix('-') {
            into.rows.push(Row { kind: 'd', old: Some(old), new: None, text: line.to_string() });
            old += 1;
            into.removed += 1;
        } else if let Some(line) = raw.strip_prefix(' ') {
            into.rows.push(Row { kind: 'c', old: Some(old), new: Some(new), text: line.to_string() });
            old += 1;
            new += 1;
        }
    }
}

fn hunk_numbers(line: &str) -> Option<(u32, u32)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(' ')?;
    let new = rest.strip_prefix('+')?.split_once(' ')?.0;

    Some((old.split(',').next()?.parse().ok()?, new.split(',').next()?.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository(files: u32, lines: u32) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();

        for args in [&["init", "-b", "main"][..], &["config", "user.name", "Tester"], &["config", "user.email", "t@example.com"], &["config", "commit.gpgsign", "false"]] {
            git(dir.path(), args, &[]).unwrap();
        }

        for file in 0..files {
            std::fs::write(dir.path().join(format!("f{file:03}.txt")), (0..lines).map(|line| format!("line {line}\n")).collect::<String>()).unwrap();
        }

        git(dir.path(), &["add", "-A"], &[]).unwrap();
        git(dir.path(), &["commit", "-m", "start"], &[]).unwrap();

        dir
    }

    fn change(dir: &tempfile::TempDir, file: u32, lines: u32) {
        std::fs::write(dir.path().join(format!("f{file:03}.txt")), (0..lines).map(|line| format!("line {line} changed\n")).collect::<String>()).unwrap();
    }

    fn entry<'a>(layout: &'a Value, path: &str) -> &'a Value {
        layout["files"].as_array().unwrap().iter().find(|file| file["path"] == path).unwrap()
    }

    #[test]
    fn the_layout_counts_every_row_a_file_renders_and_where_its_hunks_start() {
        let dir = repository(2, 40);
        change(&dir, 0, 40);
        std::fs::write(dir.path().join("new.txt"), "a\nb\nc\n").unwrap();

        let layout = layout(dir.path()).unwrap();
        let first = entry(&layout, "f000.txt");
        let untracked = entry(&layout, "new.txt");

        assert_eq!((first["added"].as_u64(), first["removed"].as_u64()), (Some(40), Some(40)));
        assert_eq!(first["rows"], 81);
        assert_eq!(first["hunks"], json!([0]));
        assert_eq!((untracked["rows"].clone(), untracked["added"].clone(), untracked["untracked"].clone()), (json!(4), json!(3), json!(true)));
        assert!(layout["files"].as_array().unwrap().iter().all(|file| file["path"] != "f001.txt"));
    }

    #[test]
    fn rows_are_served_by_range_with_numbers_and_the_hunk_header() {
        let dir = repository(1, 10);
        change(&dir, 0, 10);

        let answer = rows(dir.path(), &[RowRange { path: "f000.txt".into(), from: 0, to: 3 }, RowRange { path: "missing".into(), from: 0, to: 3 }]).unwrap();
        let ranges = answer["ranges"].as_array().unwrap();

        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0]["rows"][0]["k"], "h");
        assert_eq!(ranges[0]["rows"][1], json!({"k": "d", "o": 1, "n": null, "t": "line 0"}));
        assert_eq!(ranges[0]["rows"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn a_changed_file_gets_a_new_fingerprint_and_the_others_keep_theirs() {
        let dir = repository(2, 5);
        change(&dir, 0, 5);
        change(&dir, 1, 5);
        let before = layout(dir.path()).unwrap();

        change(&dir, 1, 7);
        let after = layout(dir.path()).unwrap();

        assert_eq!(entry(&before, "f000.txt")["fingerprint"], entry(&after, "f000.txt")["fingerprint"]);
        assert_ne!(entry(&before, "f001.txt")["fingerprint"], entry(&after, "f001.txt")["fingerprint"]);
        assert_eq!(entry(&after, "f001.txt")["added"], 7);
    }

    #[test]
    fn two_hundred_files_of_a_hundred_lines_lay_out_in_one_call() {
        let dir = repository(200, 100);

        for file in 0..200 {
            change(&dir, file, 100);
        }

        let started = std::time::Instant::now();
        let cold = layout(dir.path()).unwrap();
        let cold_time = started.elapsed();
        let started = std::time::Instant::now();
        let warm = layout(dir.path()).unwrap();
        let warm_time = started.elapsed();
        let total: u64 = cold["files"].as_array().unwrap().iter().map(|file| file["rows"].as_u64().unwrap()).sum();

        eprintln!("layout of 200 files and {total} rows: cold {cold_time:?}, warm {warm_time:?}");
        assert_eq!(cold, warm);
        assert_eq!(total, 200 * 201);
    }
}
