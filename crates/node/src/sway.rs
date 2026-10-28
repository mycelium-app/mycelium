use mycelium_proto::api::EnvironmentWindow;
use serde_json::Value;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

pub fn parse_windows(tree: &str) -> Vec<EnvironmentWindow> {
    let Ok(root) = serde_json::from_str::<Value>(tree) else {
        return Vec::new();
    };
    let mut windows = Vec::new();

    collect_windows(&root, &mut windows);

    windows
}

fn collect_windows(node: &Value, windows: &mut Vec<EnvironmentWindow>) {
    let children: Vec<&Value> = ["nodes", "floating_nodes"].iter().filter_map(|key| node.get(key).and_then(Value::as_array)).flatten().collect();

    if children.is_empty() && node.get("pid").is_some_and(|pid| !pid.is_null()) {
        let app_id = node.get("app_id").and_then(Value::as_str).or_else(|| node.pointer("/window_properties/class").and_then(Value::as_str)).unwrap_or_default();

        windows.push(EnvironmentWindow {
            id: node.get("id").and_then(Value::as_i64).unwrap_or_default(),
            app_id: app_id.to_string(),
            title: node.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
            focused: node.get("focused").and_then(Value::as_bool).unwrap_or(false),
        });
    }

    for child in children {
        collect_windows(child, windows);
    }
}

fn find_socket(runtime_dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(runtime_dir).ok()?;

    entries.flatten().find(|entry| entry.file_name().to_string_lossy().starts_with("sway-ipc.")).map(|entry| entry.path().to_string_lossy().into_owned())
}

async fn message(swaymsg: &str, runtime_dir: &Path, arguments: &[&str]) -> Result<String, String> {
    let Some(socket) = find_socket(runtime_dir) else {
        return Err("The environment's window manager is not ready yet. Try again in a moment.".to_string());
    };
    let mut command = Command::new(swaymsg);
    command.arg("-s").arg(socket).args(arguments).stdin(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
    let output = tokio::time::timeout(COMMAND_TIMEOUT, command.output()).await.map_err(|_| "The window manager took too long to answer. Try again.".to_string())?.map_err(|error| format!("swaymsg could not run: {error}."))?;

    if !output.status.success() {
        return Err(format!("The window manager refused the request: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub async fn windows(swaymsg: &str, runtime_dir: &Path) -> Result<Vec<EnvironmentWindow>, String> {
    let tree = message(swaymsg, runtime_dir, &["-t", "get_tree", "-r"]).await?;

    Ok(parse_windows(&tree))
}

pub async fn focus(swaymsg: &str, runtime_dir: &Path, window: i64) -> Result<(), String> {
    let criteria = format!("[con_id={window}]");

    message(swaymsg, runtime_dir, &[&criteria, "focus"]).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_are_the_leaves_of_the_tree_with_their_app_and_focus() {
        let tree = r#"{"id":1,"pid":null,"nodes":[{"id":2,"nodes":[{"id":3,"nodes":[
            {"id":4,"pid":100,"app_id":"chromium","name":"Inbox","focused":false,"nodes":[],"floating_nodes":[]},
            {"id":5,"pid":101,"app_id":null,"window_properties":{"class":"Gimp"},"name":"Untitled","focused":true,"nodes":[],"floating_nodes":[]}
        ],"floating_nodes":[{"id":6,"pid":102,"app_id":"foot","name":"zsh","focused":false,"nodes":[],"floating_nodes":[]}]}],"floating_nodes":[]}],"floating_nodes":[]}"#;

        let windows = parse_windows(tree);

        assert_eq!(
            windows,
            vec![
                EnvironmentWindow { id: 4, app_id: "chromium".into(), title: "Inbox".into(), focused: false },
                EnvironmentWindow { id: 5, app_id: "Gimp".into(), title: "Untitled".into(), focused: true },
                EnvironmentWindow { id: 6, app_id: "foot".into(), title: "zsh".into(), focused: false },
            ]
        );
    }

    #[test]
    fn an_empty_desktop_has_no_windows() {
        assert!(parse_windows(r#"{"id":1,"pid":null,"nodes":[{"id":2,"nodes":[],"floating_nodes":[]}],"floating_nodes":[]}"#).is_empty());
        assert!(parse_windows("not json").is_empty());
    }
}
