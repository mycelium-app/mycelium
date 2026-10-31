use super::call::{ToolCall, ToolReply};
use crate::nodes::node_offline;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mycelium_proto::link::ControlToNode;
use serde_json::{Value, json};
use uuid::Uuid;

const DEFAULT_LINES: usize = 100;
const MAX_LINES: usize = 1000;
const NO_TERMINAL: &str = "No terminal is open in this conversation. Ask the person to open one with Ctrl+` and try again.";
const UNKNOWN_TERMINAL: &str = "That terminal is not open in this conversation. Use terminal_list to see the ones that are.";

pub fn definitions() -> Value {
    json!([
        {
            "name": "terminal_list",
            "description": "List the terminals the person has open in this conversation's terminal panel. Returns an id for each, to give to terminal_read and terminal_type.",
            "inputSchema": {"type": "object", "properties": {}},
        },
        {
            "name": "terminal_read",
            "description": "Read the most recent output of one of the person's open terminals, as plain text. Use it to see what a command printed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "terminal_id": {"type": "string", "description": "A terminal id from terminal_list."},
                    "lines": {"type": "integer", "description": "How many of the last lines to return. Defaults to 100, at most 1000."},
                },
                "required": ["terminal_id"],
            },
        },
        {
            "name": "terminal_type",
            "description": "Type into one of the person's open terminals, as if they had typed it. The person approves this first. Set enter to true to press Enter after the text, which runs a command.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "terminal_id": {"type": "string", "description": "A terminal id from terminal_list."},
                    "text": {"type": "string", "description": "The text to type."},
                    "enter": {"type": "boolean", "description": "Press Enter after the text. Defaults to false."},
                },
                "required": ["terminal_id", "text"],
            },
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> crate::error::ApiResult<ToolReply> {
    let state = call.caller.state;
    let Some(conversation_id) = call.caller.conversation_id().await? else {
        return Ok(ToolReply::invalid("Terminals belong to a conversation. Call this from inside one."));
    };
    let open = state.hub.terminals_of_conversation(conversation_id);

    if call.tool == "terminal_list" {
        if open.is_empty() {
            return Ok(ToolReply::text(NO_TERMINAL, true));
        }

        let lines: Vec<String> = open.iter().map(|terminal_id| format!("- {terminal_id}")).collect();

        return Ok(ToolReply::text(&lines.join("\n"), false));
    }

    let Some(terminal_id) = call.arguments["terminal_id"].as_str().and_then(|text| text.parse::<Uuid>().ok()) else {
        return Ok(ToolReply::invalid(&format!("{} needs terminal_id, an id from terminal_list.", call.tool)));
    };

    if !open.contains(&terminal_id) {
        return Ok(ToolReply::text(UNKNOWN_TERMINAL, true));
    }

    if call.tool == "terminal_read" {
        let wanted = call.arguments["lines"].as_u64().map_or(DEFAULT_LINES, |lines| (lines as usize).clamp(1, MAX_LINES));
        let Some(buffer) = crate::terminals::read_buffer(state, terminal_id).await else {
            return Ok(ToolReply::text(UNKNOWN_TERMINAL, true));
        };
        let scrollback = STANDARD.decode(buffer.data).unwrap_or_default();

        return Ok(ToolReply::text(&last_lines(&plain_text(&scrollback), wanted), false));
    }

    let Some(text) = call.arguments["text"].as_str() else {
        return Ok(ToolReply::invalid("terminal_type needs text to type."));
    };
    let owner = state.hub.terminal_owner(terminal_id).ok_or_else(|| crate::error::ApiError::not_found("That terminal"))?;
    let enter = call.arguments["enter"].as_bool().unwrap_or(false);
    let typed = if enter { format!("{text}\r") } else { text.to_string() };

    if !state.hub.send(owner.node_id, ControlToNode::TerminalInput { terminal_id, data: STANDARD.encode(typed) }) {
        return Err(node_offline());
    }

    Ok(ToolReply::text("Typed into the terminal.", false))
}

fn last_lines(text: &str, wanted: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(wanted);

    lines[start..].join("\n")
}

pub fn plain_text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut plain = String::new();
    let mut characters = text.chars().peekable();

    while let Some(character) = characters.next() {
        match character {
            '\u{1b}' => match characters.next() {
                Some('[') => {
                    for next in characters.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(next) = characters.next() {
                        if next == '\u{7}' {
                            break;
                        }

                        if next == '\u{1b}' {
                            characters.next();

                            break;
                        }
                    }
                }
                _ => {}
            },
            '\r' if characters.peek() == Some(&'\n') => {}
            '\r' => {
                let line_start = plain.rfind('\n').map_or(0, |at| at + 1);
                plain.truncate(line_start);
            }
            '\u{8}' => {
                plain.pop();
            }
            other if other == '\n' || other == '\t' || !other.is_control() => plain.push(other),
            _ => {}
        }
    }

    plain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_codes_and_title_sequences_are_removed() {
        let output = b"\x1b]0;user@host\x07\x1b[32mgreen\x1b[0m done\r\nnext";

        assert_eq!(plain_text(output), "green done\nnext");
    }

    #[test]
    fn a_progress_line_rewritten_with_a_carriage_return_keeps_only_its_last_state() {
        assert_eq!(plain_text(b"10%\r50%\r100%\r\n"), "100%\n");
    }

    #[test]
    fn only_the_last_lines_are_returned() {
        assert_eq!(last_lines("a\nb\nc", 2), "b\nc");
    }
}
