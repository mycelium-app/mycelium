use crate::error::ApiError;
use crate::state::AppState;
use base64::Engine;
use mycelium_proto::link::GitCommand;
use serde_json::{Map, Value};
use std::collections::HashMap;
use uuid::Uuid;

const LIMIT_BYTES: u64 = 25 * 1024 * 1024;

fn file_of(value: &Value) -> Option<&str> {
    let object = value.as_object().filter(|object| object.len() == 1)?;

    object.get("file")?.as_str()
}

fn files_in(value: &Value, found: &mut Vec<String>) {
    if let Some(path) = file_of(value) {
        found.push(path.to_string());

        return;
    }

    match value {
        Value::Object(object) => object.values().for_each(|inner| files_in(inner, found)),
        Value::Array(items) => items.iter().for_each(|inner| files_in(inner, found)),
        _ => {}
    }
}

fn replaced(value: &Value, contents: &HashMap<String, String>) -> Value {
    if let Some(content) = file_of(value).and_then(|path| contents.get(path)) {
        return Value::String(content.clone());
    }

    match value {
        Value::Object(object) => Value::Object(object.iter().map(|(name, inner)| (name.clone(), replaced(inner, contents))).collect::<Map<_, _>>()),
        Value::Array(items) => Value::Array(items.iter().map(|inner| replaced(inner, contents)).collect()),
        other => other.clone(),
    }
}

fn takes_bytes(tool: &str) -> bool {
    tool.contains("attachment")
}

pub async fn substitute(state: &AppState, owner: Uuid, conversation: Option<Uuid>, tool: &str, arguments: Value) -> Result<Value, String> {
    let mut paths = Vec::new();
    files_in(&arguments, &mut paths);

    if paths.is_empty() {
        return Ok(arguments);
    }

    let Some(conversation_id) = conversation else {
        return Err("A file argument needs a conversation with a node, and this call did not come from one. Nothing was done.".to_string());
    };
    let found = crate::store::find_owned(&state.pool, conversation_id, owner).await.map_err(|error| error.message().to_string())?;
    let mut contents = HashMap::new();

    for path in paths {
        if contents.contains_key(&path) {
            continue;
        }

        let answer = crate::git::relay(state, found.node_id, found.cwd.clone(), GitCommand::ReadFile { path: path.clone() }).await.map_err(|error: ApiError| error.message().to_string())?;
        let size = answer["size"].as_u64().unwrap_or_default();

        if size > LIMIT_BYTES {
            return Err(format!("{path} is larger than 25 MB, which is the most a file argument can carry. Use the integration's API access for files this size."));
        }

        let encoded = answer["data"].as_str().unwrap_or_default().to_string();

        let content = if takes_bytes(tool) {
            encoded
        } else {
            let bytes = base64::engine::general_purpose::STANDARD.decode(&encoded).map_err(|_| format!("{path} could not be read."))?;

            String::from_utf8(bytes).map_err(|_| format!("{path} is not text, and {tool} takes text. Use a tool that takes attachments, or the integration's API."))?
        };

        contents.insert(path, content);
    }

    Ok(replaced(&arguments, &contents))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_objects_holding_just_a_file_are_file_arguments() {
        let arguments = json!({"title": "Notes", "text": {"file": "notes/a.md"}, "nested": [{"body": {"file": "b.txt"}}], "options": {"file": "x", "other": 1}});
        let mut found = Vec::new();

        files_in(&arguments, &mut found);

        assert_eq!(found, vec!["b.txt".to_string(), "notes/a.md".to_string()]);
    }

    #[test]
    fn each_file_argument_is_replaced_by_its_content_and_the_rest_is_untouched() {
        let arguments = json!({"title": "Notes", "text": {"file": "notes/a.md"}, "options": {"file": "x", "other": 1}});
        let contents = HashMap::from([("notes/a.md".to_string(), "# Exact\n".to_string())]);

        assert_eq!(replaced(&arguments, &contents), json!({"title": "Notes", "text": "# Exact\n", "options": {"file": "x", "other": 1}}));
    }

    #[test]
    fn attachment_tools_take_bytes() {
        assert!(takes_bytes("create_attachment"));
        assert!(!takes_bytes("create_document"));
    }
}
