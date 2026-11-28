use super::call::{Caller, ToolCall, ToolReply};
use crate::error::ApiResult;
use serde_json::{Value, json};
use uuid::Uuid;

pub fn definitions() -> Value {
    json!([
        {
            "name": "post_virtual_message",
            "description": "Send the person a message they will read. Name a virtual chat of this assistant to post it there, or leave chat out to show it in this conversation. Use it for answers, questions and hand-overs: the person may only read these messages and their own.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "The message, in Markdown."},
                    "chat": {"type": "string", "description": "The virtual chat's id or exact name, as list_virtual_chats gives it. Leave out to show the message here."},
                },
                "required": ["text"],
            },
        },
        {
            "name": "list_virtual_chats",
            "description": "List this assistant's virtual chats: one topic each, all driven by this conversation. Gives each chat's id, name, whether it is closed, and when it last changed.",
            "inputSchema": {"type": "object", "properties": {}},
        },
        {
            "name": "read_virtual_chat",
            "description": "Read the messages of one of this assistant's virtual chats, oldest first, with its to-do list.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "chat": {"type": "string", "description": "The virtual chat's id or exact name."},
                    "limit": {"type": "integer", "description": "How many of the latest messages to read, 50 by default."},
                },
                "required": ["chat"],
            },
        },
        {
            "name": "create_virtual_chat",
            "description": "Open a new virtual chat under this assistant for a topic of its own, so it does not get lost among the others. It appears under the assistant in the person's sidebar; they stay where they are.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "A short name for the topic."},
                    "description": {"type": "string", "description": "One sentence on what the chat is for."},
                    "message": {"type": "string", "description": "The chat's first message, in Markdown, posted as post_virtual_message would."},
                },
                "required": ["title"],
            },
        },
        {
            "name": "close_virtual_chat",
            "description": "Close one of this assistant's virtual chats once its topic is settled. It stays under the assistant marked Closed, and opens again when anyone writes in it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "chat": {"type": "string", "description": "The virtual chat's id or exact name."},
                },
                "required": ["chat"],
            },
        },
    ])
}

async fn assistant_of(caller: &Caller<'_>) -> ApiResult<Option<(Uuid, Uuid)>> {
    let Some(conversation_id) = caller.conversation_id().await? else {
        return Ok(None);
    };

    Ok(sqlx::query_as::<_, (Uuid, Uuid)>("select owner, id from assistants where conversation_id = $1").bind(conversation_id).fetch_optional(&caller.state.pool).await?)
}

const NOT_AN_ASSISTANT: &str = "Virtual chats belong to an assistant, and this conversation is not an assistant's. Leave chat out to show the message here.";

pub(super) async fn resolve(caller: &Caller<'_>, named: &str) -> ApiResult<Result<(Uuid, mycelium_proto::api::VirtualChat), String>> {
    let Some((owner, assistant_id)) = assistant_of(caller).await? else {
        return Ok(Err(NOT_AN_ASSISTANT.to_string()));
    };

    Ok(match crate::virtual_chats::find(caller.state, owner, assistant_id, named).await? {
        Some(chat) => Ok((owner, chat)),
        None => Err(format!("This assistant has no virtual chat named {named}. Call list_virtual_chats for their names and ids.")),
    })
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, tool, arguments, .. } = call;
    let field = |key: &str| arguments.get(key).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty());

    match tool {
        "post_virtual_message" => {
            let Some(text) = field("text") else {
                return Ok(ToolReply::invalid("post_virtual_message needs text, a string."));
            };
            let Some(named) = field("chat") else {
                return Ok(ToolReply::text("Shown to the person in this conversation.", false));
            };

            match resolve(&caller, named).await? {
                Ok((owner, chat)) => {
                    crate::virtual_chats::post(caller.state, owner, chat.id, text).await?;

                    Ok(ToolReply::text(&format!("Posted to the virtual chat {}.", chat.title), false))
                }
                Err(reason) => Ok(ToolReply::text(&format!("Nothing was posted. {reason}"), true)),
            }
        }
        "list_virtual_chats" => {
            let Some((owner, assistant_id)) = assistant_of(&caller).await? else {
                return Ok(ToolReply::text(NOT_AN_ASSISTANT, true));
            };
            let chats = crate::virtual_chats::chats_of(caller.state, owner, assistant_id).await?;
            let listed: Vec<Value> = chats.iter().map(|chat| json!({"id": chat.id, "name": chat.title, "description": chat.description, "closed": chat.closed, "updated_at": chat.updated_at})).collect();

            Ok(ToolReply::text(&serde_json::to_string_pretty(&listed).unwrap_or_default(), false))
        }
        "read_virtual_chat" => {
            let Some(named) = field("chat") else {
                return Ok(ToolReply::invalid("read_virtual_chat needs chat, the id or name of a virtual chat."));
            };
            let limit = arguments.get("limit").and_then(Value::as_i64).filter(|limit| *limit > 0).unwrap_or(crate::virtual_chats::HISTORY_LIMIT);

            match resolve(&caller, named).await? {
                Ok((_, chat)) => {
                    let messages = crate::virtual_chats::messages_of(caller.state, chat.id, limit, None).await?;
                    let lines: Vec<Value> = messages.iter().map(|message| json!({"from": if message.from_person { "person" } else { "assistant" }, "at": message.created_at, "text": message.text, "attachments": message.attachments.iter().map(|file| file.name.clone()).collect::<Vec<_>>()})).collect();
                    let body = json!({"chat": {"id": chat.id, "name": chat.title, "closed": chat.closed}, "todos": chat.todos, "messages": lines});

                    Ok(ToolReply::text(&super::call::data_block("virtual_chat", &[("name", Some(chat.title.clone()))], &serde_json::to_string_pretty(&body).unwrap_or_default()), false))
                }
                Err(reason) => Ok(ToolReply::text(&reason, true)),
            }
        }
        "create_virtual_chat" => {
            let Some(title) = field("title") else {
                return Ok(ToolReply::invalid("create_virtual_chat needs title, a string."));
            };
            let Some((owner, assistant_id)) = assistant_of(&caller).await? else {
                return Ok(ToolReply::text(NOT_AN_ASSISTANT, true));
            };

            let chat = crate::virtual_chats::create_for(caller.state, owner, assistant_id, title, field("description")).await?;

            if let Some(message) = field("message") {
                crate::virtual_chats::post(caller.state, owner, chat.id, message).await?;
            }

            Ok(ToolReply::text(&format!("Opened the virtual chat {} ({}). Post to it with post_virtual_message.", chat.title, chat.id), false))
        }
        "close_virtual_chat" => {
            let Some(named) = field("chat") else {
                return Ok(ToolReply::invalid("close_virtual_chat needs chat, the id or name of a virtual chat."));
            };

            match resolve(&caller, named).await? {
                Ok((owner, chat)) => {
                    crate::virtual_chats::close(caller.state, owner, chat.id, true).await?;

                    Ok(ToolReply::text(&format!("Closed the virtual chat {}. It opens again when anyone writes in it.", chat.title), false))
                }
                Err(reason) => Ok(ToolReply::text(&format!("Nothing was closed. {reason}"), true)),
            }
        }
        _ => Ok(ToolReply::invalid("Unknown virtual chat tool.")),
    }
}
