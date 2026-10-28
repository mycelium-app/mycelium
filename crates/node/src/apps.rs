use crate::terminal::find_in_path;
use mycelium_proto::api::{EnvironmentApp, EnvironmentAppKind};
use std::ffi::OsStr;
use std::path::PathBuf;

const FILE_PLACEHOLDERS: [&str; 4] = ["%f", "%F", "%u", "%U"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopApp {
    pub id: String,
    pub name: String,
    pub kind: EnvironmentAppKind,
    command: Vec<String>,
}

impl DesktopApp {
    pub fn offer(&self) -> EnvironmentApp {
        EnvironmentApp { id: self.id.clone(), name: self.name.clone(), kind: self.kind }
    }

    pub fn command_with(&self, file: Option<&str>) -> Vec<String> {
        self.command
            .iter()
            .filter_map(|word| {
                if FILE_PLACEHOLDERS.contains(&word.as_str()) {
                    return file.map(String::from);
                }

                let replaced = FILE_PLACEHOLDERS.iter().fold(word.replace("%%", "\u{0}"), |text, placeholder| text.replace(placeholder, file.unwrap_or_default())).replace('\u{0}', "%");
                let cleaned = strip_field_codes(&replaced);

                (!cleaned.is_empty()).then_some(cleaned)
            })
            .collect()
    }
}

fn strip_field_codes(word: &str) -> String {
    let mut cleaned = String::new();
    let mut characters = word.chars().peekable();

    while let Some(character) = characters.next() {
        let is_code = character == '%' && characters.peek().is_some_and(|next| next.is_ascii_alphabetic());

        if is_code {
            characters.next();
        } else {
            cleaned.push(character);
        }
    }

    cleaned
}

pub fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    let system = std::env::var_os("XDG_DATA_DIRS").filter(|value| !value.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());

    home.into_iter().chain(std::env::split_paths(&system)).collect()
}

pub fn split_command(exec: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut characters = exec.chars();

    while let Some(character) = characters.next() {
        match character {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => {
                if let Some(escaped) = characters.next() {
                    current.push(escaped);
                }
            }
            character if character.is_whitespace() && !quoted => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            character => {
                current.push(character);
                started = true;
            }
        }
    }

    if started {
        words.push(current);
    }

    words
}

fn kind_of(categories: &[&str]) -> EnvironmentAppKind {
    let has = |name: &str| categories.contains(&name);

    if has("WebBrowser") {
        EnvironmentAppKind::Browser
    } else if has("FileManager") {
        EnvironmentAppKind::FileManager
    } else if has("TerminalEmulator") {
        EnvironmentAppKind::Terminal
    } else if has("Email") {
        EnvironmentAppKind::Mail
    } else if has("TextEditor") || has("IDE") {
        EnvironmentAppKind::Editor
    } else if has("Office") || has("WordProcessor") || has("Spreadsheet") || has("Presentation") {
        EnvironmentAppKind::Office
    } else {
        EnvironmentAppKind::Other
    }
}

fn parse_entry(id: &str, text: &str, path: &OsStr) -> Option<DesktopApp> {
    let mut in_entry = false;
    let mut fields = std::collections::HashMap::new();

    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some((key, value)) = line.split_once('=') {
            fields.entry(key.trim()).or_insert_with(|| value.trim());
        }
    }

    let hidden = ["NoDisplay", "Hidden", "Terminal"].iter().any(|key| fields.get(key) == Some(&"true"));

    if hidden || fields.get("Type").is_some_and(|kind| *kind != "Application") {
        return None;
    }

    let name = fields.get("Name").filter(|name| !name.is_empty())?.to_string();
    let words = split_command(fields.get("Exec")?);
    let program_index = if words.first().is_some_and(|word| word == "env") { words.iter().position(|word| word != "env" && !word.contains('=') && !word.starts_with('-'))? } else { 0 };
    let program = words.get(program_index)?;
    let try_exec = fields.get("TryExec").filter(|program| !program.is_empty());

    find_in_path(program, path)?;

    if let Some(try_exec) = try_exec {
        find_in_path(try_exec, path)?;
    }

    let categories: Vec<&str> = fields.get("Categories").map(|value| value.split(';').collect()).unwrap_or_default();

    Some(DesktopApp { id: id.to_string(), name, kind: kind_of(&categories), command: words })
}

pub fn discover(path: &OsStr, data_dirs: &[PathBuf]) -> Vec<DesktopApp> {
    let mut apps: Vec<DesktopApp> = Vec::new();

    for directory in data_dirs {
        let Ok(entries) = std::fs::read_dir(directory.join("applications")) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).filter(|file| file.extension().is_some_and(|extension| extension == "desktop")).collect();

        files.sort();

        for file in files {
            let Some(id) = file.file_stem().map(|stem| stem.to_string_lossy().into_owned()) else {
                continue;
            };

            if apps.iter().any(|app| app.id == id) {
                continue;
            }

            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };

            apps.extend(parse_entry(&id, &text, path));
        }
    }

    apps.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));

    apps
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn program(directory: &std::path::Path, name: &str) {
        let file = directory.join(name);
        std::fs::write(&file, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn entry(data: &std::path::Path, id: &str, body: &str) {
        let applications = data.join("applications");
        std::fs::create_dir_all(&applications).unwrap();
        std::fs::write(applications.join(format!("{id}.desktop")), format!("[Desktop Entry]\nType=Application\n{body}\n")).unwrap();
    }

    #[test]
    fn programs_on_the_path_are_offered_with_a_kind_from_their_categories() {
        let bin = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        for name in ["chromium", "nautilus", "code", "soffice", "thunderbird", "foot", "gimp"] {
            program(bin.path(), name);
        }
        entry(data.path(), "chromium", "Name=Chromium\nExec=chromium %U\nCategories=Network;WebBrowser;");
        entry(data.path(), "org.gnome.Nautilus", "Name=Files\nExec=nautilus --new-window %U\nCategories=Utility;FileManager;");
        entry(data.path(), "code", "Name=Code\nExec=code %F\nCategories=Utility;TextEditor;Development;IDE;");
        entry(data.path(), "libreoffice-writer", "Name=Writer\nExec=soffice --writer %U\nCategories=Office;WordProcessor;");
        entry(data.path(), "thunderbird", "Name=Thunderbird\nExec=thunderbird %u\nCategories=Network;Email;");
        entry(data.path(), "foot", "Name=Foot\nExec=foot\nCategories=System;TerminalEmulator;");
        entry(data.path(), "gimp", "Name=GIMP\nExec=gimp\nCategories=Graphics;");

        let apps = discover(bin.path().as_os_str(), &[data.path().to_path_buf()]);
        let kinds: Vec<(&str, EnvironmentAppKind)> = apps.iter().map(|app| (app.id.as_str(), app.kind)).collect();

        assert_eq!(
            kinds,
            vec![
                ("chromium", EnvironmentAppKind::Browser),
                ("code", EnvironmentAppKind::Editor),
                ("org.gnome.Nautilus", EnvironmentAppKind::FileManager),
                ("foot", EnvironmentAppKind::Terminal),
                ("gimp", EnvironmentAppKind::Other),
                ("thunderbird", EnvironmentAppKind::Mail),
                ("libreoffice-writer", EnvironmentAppKind::Office),
            ]
        );
    }

    #[test]
    fn hidden_terminal_missing_and_duplicate_entries_are_not_offered() {
        let bin = tempfile::tempdir().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        for name in ["htop", "tool", "other"] {
            program(bin.path(), name);
        }
        entry(first.path(), "htop", "Name=htop\nExec=htop\nTerminal=true");
        entry(first.path(), "hidden", "Name=Hidden\nExec=tool\nNoDisplay=true");
        entry(first.path(), "absent", "Name=Absent\nExec=not-installed");
        entry(first.path(), "tool", "Name=Tool\nExec=tool");
        entry(second.path(), "tool", "Name=Tool shadowed\nExec=other");

        let apps = discover(bin.path().as_os_str(), &[first.path().to_path_buf(), second.path().to_path_buf()]);

        assert_eq!(apps.iter().map(|app| app.name.as_str()).collect::<Vec<_>>(), vec!["Tool"]);
    }

    #[test]
    fn a_file_replaces_the_placeholder_and_is_dropped_when_there_is_none() {
        let app = DesktopApp { id: "editor".into(), name: "Editor".into(), kind: EnvironmentAppKind::Editor, command: split_command("env GDK_BACKEND=wayland \"/opt/my editor/run\" --flag %F %i") };

        assert_eq!(app.command_with(Some("/tmp/a.txt")), vec!["env", "GDK_BACKEND=wayland", "/opt/my editor/run", "--flag", "/tmp/a.txt"]);
        assert_eq!(app.command_with(None), vec!["env", "GDK_BACKEND=wayland", "/opt/my editor/run", "--flag"]);
    }
}
