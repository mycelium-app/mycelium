use crate::error::ApiError;
use crate::state::AppState;
use base64::Engine;
use mycelium_proto::link::GitCommand;
use serde_json::{Map, Value};
use std::collections::HashMap;
use uuid::Uuid;

pub const ADVERT: &str = "Any text argument can be {\"file\": \"<path in the conversation folder>\"} instead of the text: Mycelium reads the file and sends its exact contents, up to 25 MB. Add \"encoding\": \"base64\" to send a binary file, such as a PDF, as base64. This needs a conversation on a node.";

const LIMIT_BYTES: u64 = 25 * 1024 * 1024;

pub const SAVE_PROPERTY: &str = "save_to";

pub const NEEDS_SAVE_TO: &str = "This answer is a file, not text. Call it again with save_to set to a path in the conversation folder, and Mycelium will write the file there unchanged.";

pub fn with_save_property(mut schema: Value) -> Value {
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        properties.insert(SAVE_PROPERTY.to_string(), serde_json::json!({"type": "string", "description": "Optional path in the conversation folder. Mycelium writes the whole answer there, up to 20 MB, and returns only where it went and its size. Use it for answers too long to read here."}));
    }

    schema
}

pub fn take_save_to(arguments: &mut Value) -> Option<String> {
    let removed = arguments.as_object_mut()?.remove(SAVE_PROPERTY)?;

    removed.as_str().map(str::trim).filter(|path| !path.is_empty()).map(String::from)
}

pub async fn save(state: &AppState, owner: Uuid, conversation: Uuid, path: &str, contents: &[u8]) -> Result<String, String> {
    let found = crate::store::find_owned(&state.pool, conversation, owner).await.map_err(|error| error.message().to_string())?;
    let data_base64 = base64::engine::general_purpose::STANDARD.encode(contents);
    let answer = crate::git::relay(state, found.node_id, found.cwd.clone(), GitCommand::WriteFile { path: path.to_string(), data_base64 }).await.map_err(|error: ApiError| error.message().to_string())?;

    Ok(format!("Saved the answer, {} bytes, to {path} in the conversation folder.", answer["size"].as_u64().unwrap_or(contents.len() as u64)))
}

fn file_of(value: &Value) -> Option<(&str, bool)> {
    let object = value.as_object()?;
    let base64 = object.get("encoding").and_then(Value::as_str) == Some("base64");

    if object.len() != 1 + usize::from(base64) {
        return None;
    }

    Some((object.get("file")?.as_str()?, base64))
}

fn files_in(value: &Value, found: &mut Vec<(String, bool)>) {
    if let Some((path, base64)) = file_of(value) {
        found.push((path.to_string(), base64));

        return;
    }

    match value {
        Value::Object(object) => object.values().for_each(|inner| files_in(inner, found)),
        Value::Array(items) => items.iter().for_each(|inner| files_in(inner, found)),
        _ => {}
    }
}

pub fn any_in(arguments: &Value) -> bool {
    let mut found = Vec::new();
    files_in(arguments, &mut found);

    !found.is_empty()
}

fn replaced(value: &Value, contents: &HashMap<(String, bool), String>) -> Value {
    if let Some(content) = file_of(value).and_then(|(path, base64)| contents.get(&(path.to_string(), base64))) {
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

    for (path, base64) in paths {
        if contents.contains_key(&(path.clone(), base64)) {
            continue;
        }

        let answer = crate::git::relay(state, found.node_id, found.cwd.clone(), GitCommand::ReadFile { path: path.clone() }).await.map_err(|error: ApiError| error.message().to_string())?;
        let size = answer["size"].as_u64().unwrap_or_default();

        if size > LIMIT_BYTES {
            return Err(format!("{path} is larger than 25 MB, which is the most a file argument can carry. Use the integration's API access for files this size."));
        }

        let encoded = answer["data"].as_str().unwrap_or_default().to_string();

        let content = if base64 || takes_bytes(tool) {
            encoded
        } else {
            let bytes = base64::engine::general_purpose::STANDARD.decode(&encoded).map_err(|_| format!("{path} could not be read."))?;

            String::from_utf8(bytes).map_err(|_| format!("{path} is not text, and {tool} takes text. Use a tool that takes attachments, or the integration's API."))?
        };

        contents.insert((path, base64), content);
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

        assert_eq!(found, vec![("b.txt".to_string(), false), ("notes/a.md".to_string(), false)]);
    }

    #[test]
    fn any_in_finds_a_file_argument_anywhere() {
        assert!(any_in(&json!({"a": [{"b": {"file": "x"}}]})));
        assert!(!any_in(&json!({"a": "x"})));
    }

    #[test]
    fn each_file_argument_is_replaced_by_its_content_and_the_rest_is_untouched() {
        let arguments = json!({"title": "Notes", "text": {"file": "notes/a.md"}, "options": {"file": "x", "other": 1}});
        let contents = HashMap::from([(("notes/a.md".to_string(), false), "# Exact\n".to_string())]);

        assert_eq!(replaced(&arguments, &contents), json!({"title": "Notes", "text": "# Exact\n", "options": {"file": "x", "other": 1}}));
    }

    #[test]
    fn a_file_argument_asking_for_base64_is_found_and_other_extra_fields_are_not_file_arguments() {
        let arguments = json!({"attachment": {"data": {"file": "scan.pdf", "encoding": "base64"}, "file_name": "scan.pdf"}, "other": {"file": "x", "encoding": "hex"}});
        let mut found = Vec::new();

        files_in(&arguments, &mut found);

        assert_eq!(found, vec![("scan.pdf".to_string(), true)]);
    }

    #[test]
    fn attachment_tools_take_bytes() {
        assert!(takes_bytes("create_attachment"));
        assert!(!takes_bytes("create_document"));
    }
}
