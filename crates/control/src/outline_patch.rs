use serde_json::Value;

const UPDATE_DOCUMENT: &str = "update_document";
const PATCH: &str = "patch";

fn table_rows(markdown: &str) -> Vec<&str> {
    markdown.lines().map(str::trim).filter(|line| line.starts_with('|')).collect()
}

pub fn refusal(tool: &str, arguments: &Value) -> Option<String> {
    if tool != UPDATE_DOCUMENT || arguments["editMode"].as_str() != Some(PATCH) {
        return None;
    }

    let found = table_rows(arguments["findText"].as_str()?);
    let kept = table_rows(arguments["text"].as_str().unwrap_or_default());
    let deleted = found.len().saturating_sub(kept.len());

    if deleted == 0 {
        return None;
    }

    let rows = if deleted == 1 { "1 table row".to_string() } else { format!("{deleted} table rows") };
    let missing: Vec<&str> = found.into_iter().filter(|row| !kept.contains(row)).collect();

    Some(format!(
        "Nothing was changed. This patch would delete {rows}. These rows are in findText but not in text:\n{}\nOutline removes the whole table body when a patch deletes a row, so the other rows in the table would be lost too. To delete a row, patch the whole table: set findText to the entire table and text to the table with every row you want to keep.",
        missing.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::refusal;
    use serde_json::json;

    #[test]
    fn a_patch_that_blanks_one_table_row_is_refused_and_names_the_row() {
        let refused = refusal("update_document", &json!({"id": "doc", "editMode": "patch", "findText": "| Jest | done |", "text": "\n"})).unwrap();

        assert!(refused.starts_with("Nothing was changed. This patch would delete 1 table row. These rows are in findText but not in text:\n| Jest | done |\n"), "{refused}");
        assert!(refused.contains("set findText to the entire table"), "{refused}");
    }

    #[test]
    fn a_patch_of_the_whole_table_that_drops_rows_is_refused_with_every_lost_row() {
        let table = "| Tool | State |\n| --- | --- |\n| Jest | done |\n| Vitest | open |\n| Mocha | open |";
        let refused = refusal("update_document", &json!({"editMode": "patch", "findText": table, "text": "| Tool | State |\n| --- | --- |\n| Jest | done |"})).unwrap();

        assert!(refused.contains("delete 2 table rows. These rows are in findText but not in text:\n| Vitest | open |\n| Mocha | open |\n"), "{refused}");
    }

    #[test]
    fn a_patch_that_keeps_or_changes_rows_goes_through() {
        let table = "| Tool | State |\n| --- | --- |\n| Jest | open |";

        assert_eq!(refusal("update_document", &json!({"editMode": "patch", "findText": table, "text": format!("{table}\n| Vitest | open |")})), None);
        assert_eq!(refusal("update_document", &json!({"editMode": "patch", "findText": "| Jest | open |", "text": "| Jest | done |"})), None);
    }

    #[test]
    fn patches_without_table_rows_and_other_edit_modes_go_through() {
        assert_eq!(refusal("update_document", &json!({"editMode": "patch", "findText": "Draft", "text": ""})), None);
        assert_eq!(refusal("update_document", &json!({"editMode": "replace", "text": "Fresh start"})), None);
        assert_eq!(refusal("search", &json!({"query": "| Jest | done |"})), None);
    }
}
