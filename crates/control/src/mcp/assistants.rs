use super::call::{ToolCall, ToolReply};
use crate::assistant_actions;
use crate::assistant_changes;
use crate::assistants;
use crate::error::ApiResult;
use serde_json::{Value, json};

pub fn definitions() -> Value {
    json!([
        {
            "name": "assistant_list_files",
            "description": "List the files in this assistant's storage, optionally only those whose path starts with a prefix. Only an assistant's own conversation can use it.",
            "inputSchema": {"type": "object", "properties": {"prefix": {"type": "string", "description": "Only list paths starting with this."}}},
        },
        {
            "name": "assistant_read_file",
            "description": "Read a file from this assistant's storage. AGENTS.md holds the assistant's own instructions.",
            "inputSchema": {"type": "object", "properties": {"path": {"type": "string", "description": "The path of the file, for example notes/standup.md."}}, "required": ["path"]},
        },
        {
            "name": "assistant_write_file",
            "description": "Create or replace a file in this assistant's storage. Storage is kept between sessions, so write down what a later session needs to know. Files are at most 1 MiB.",
            "inputSchema": {"type": "object", "properties": {"path": {"type": "string", "description": "The path of the file, for example notes/standup.md."}, "content": {"type": "string", "description": "The whole new content of the file."}}, "required": ["path", "content"]},
        },
        {
            "name": "assistant_delete_file",
            "description": "Delete a file from this assistant's storage.",
            "inputSchema": {"type": "object", "properties": {"path": {"type": "string", "description": "The path of the file."}}, "required": ["path"]},
        },
        {
            "name": "assistant_set_secret",
            "description": "Store a secret for this assistant, such as an API token the person gave you. Secrets are kept on the control plane, never shown again, and reach working folder sessions as environment variables when they start. Only an assistant's own conversation can use it.",
            "inputSchema": {"type": "object", "properties": {"name": {"type": "string", "description": "The environment variable name, for example GITHUB_TOKEN."}, "value": {"type": "string", "description": "The secret value."}, "description": {"type": "string", "description": "What the secret is for."}}, "required": ["name", "value"]},
        },
        {
            "name": "assistant_request_secret",
            "description": "Ask the person for a secret you need. It appears blank for them to fill in; it is never sent through the conversation. Only an assistant's own conversation can use it.",
            "inputSchema": {"type": "object", "properties": {"name": {"type": "string", "description": "The environment variable name, for example GITHUB_TOKEN."}, "description": {"type": "string", "description": "What the secret is for and where to find it."}}, "required": ["name"]},
        },
        {
            "name": "assistant_propose_action",
            "description": "Propose a reusable action for this assistant, such as a prompt the person keeps asking for. The person approves or edits it before it is added to the assistant's Actions. Use {input} placeholders in the prompt for anything the person should be asked before it runs. Only an assistant's own conversation can use it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "A short name for the action."},
                    "prompt": {"type": "string", "description": "The prompt the action sends, with {input} placeholders."},
                    "inputs": {
                        "type": "array",
                        "description": "What to ask before running.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": {"type": "string", "description": "The placeholder name, without braces."},
                                "kind": {"type": "string", "enum": ["text", "date", "choice"]},
                                "options": {"type": "array", "items": {"type": "string"}, "description": "The choices, for kind choice."},
                                "required": {"type": "boolean"},
                                "default": {"type": "string"},
                            },
                            "required": ["name", "kind"],
                        },
                    },
                    "reason": {"type": "string", "description": "Why this action is worth keeping, for example the request you keep getting."},
                },
                "required": ["name", "prompt"],
            },
        },
        {
            "name": "assistant_list",
            "description": "List the person's assistants with id, name, description, group, state and whether it is built in.",
            "inputSchema": {"type": "object", "properties": {}},
        },
        {
            "name": "assistant_create",
            "description": "Propose a new assistant. The person approves it before it exists. Other settings start at their defaults, and the person sets models and nodes afterwards.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "The assistant's name."},
                    "group": {"type": "string", "description": "The name of one of the person's groups."},
                    "description": {"type": "string", "description": "One line on what the assistant is for."},
                    "instructions": {"type": "string", "description": "The assistant's instructions, in Markdown."},
                },
                "required": ["name", "instructions"],
            },
        },
        {
            "name": "assistant_update",
            "description": "Change an assistant. Description, group, models and nodes apply straight away, and the person can undo them. A new instructions text waits for the person's approval. Secrets and archiving are not available.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "The assistant id, as assistant_list returned it."},
                    "description": {"type": "string"},
                    "group": {"type": "string", "description": "The name of one of the person's groups."},
                    "models": {"type": "array", "items": {"type": "object", "properties": {"agent": {"type": "string", "enum": ["claude", "codex"]}, "provider_id": {"type": "string"}, "model": {"type": "string"}, "account": {"type": "string"}}}, "description": "The models in order of preference."},
                    "nodes": {"type": "array", "items": {"type": "string"}, "description": "Node ids the assistant may run on."},
                    "instructions": {"type": "string", "description": "The whole new instructions text."},
                    "reason": {"type": "string", "description": "Why the instructions change, shown with the proposal."},
                },
                "required": ["id"],
            },
        },
        {
            "name": "assistant_use_workspace",
            "description": "Move this assistant's thread to a node when the work needs files on disk or commands, which a chat cannot do. Only an assistant's own chat can use it. The move happens when this reply ends, with the assistant's storage copied into the working folder, and the conversation carries on there.",
            "inputSchema": {"type": "object", "properties": {"reason": {"type": "string", "description": "Why a working folder is needed."}}},
        },
    ])
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, tool, arguments, .. } = call;
    let state = caller.state;

    Ok(match tool {
        "assistant_list_files" | "assistant_read_file" | "assistant_write_file" | "assistant_delete_file" => {
            let conversation = caller.conversation_id().await?;
            let (text, is_error) = assistants::file_tool(state, conversation, tool, &arguments).await?;

            ToolReply::text(&text, is_error)
        }
        "assistant_set_secret" | "assistant_request_secret" => {
            let conversation = caller.conversation_id().await?;
            let (text, is_error) = assistants::secret_tool(state, conversation, tool, &arguments).await?;

            ToolReply::text(&text, is_error)
        }
        "assistant_propose_action" => {
            let Some((conversation_id, _)) = caller.conversation().await? else {
                return Ok(ToolReply::text("Nothing was proposed. An action belongs to an assistant's conversation, and this call did not come from one.", true));
            };

            match assistant_actions::propose_from_agent(state, conversation_id, &arguments).await? {
                Ok(proposal) => ToolReply::text(&format!("Action \"{}\" proposed. It waits for the person to approve it; do not propose it again.", proposal.name), false),
                Err(message) => ToolReply::text(&message, true),
            }
        }
        "assistant_list" | "assistant_create" | "assistant_update" => {
            let owner = caller.owner().await?;
            let conversation = caller.conversation_id().await?;

            match tool {
                "assistant_list" => ToolReply::text(&assistant_changes::list_tool(state, owner).await?, false),
                "assistant_create" => match assistant_changes::propose_create(state, owner, conversation, &arguments).await? {
                    Ok(change) => ToolReply::text(&format!("Assistant \"{}\" proposed. It waits for the person to approve it; it does not exist yet.", change.name), false),
                    Err(message) => ToolReply::text(&message, true),
                },
                _ => match assistant_changes::update_from_agent(state, owner, conversation, &arguments).await? {
                    Ok(message) => ToolReply::text(&message, false),
                    Err(message) => ToolReply::text(&message, true),
                },
            }
        }
        _ => {
            let conversation = caller.conversation_id().await?;
            let (text, is_error) = assistants::use_workspace_tool(state, conversation).await?;

            ToolReply::text(&text, is_error)
        }
    })
}
