use super::call::{Caller, ToolCall, ToolReply};
use crate::chat_tool_servers::{bearer, list_owned, refreshable_after_rejection, renew, slug};
use crate::chat_tools;
use crate::error::ApiResult;
use crate::mcp_client::{McpConnection, McpError};
use crate::response_editor;
use futures_util::future::join_all;
use serde_json::{Value, json};

pub fn definitions() -> Value {
    json!([
        {"name":"find_integration_tools","description":"Integrations added after this conversation started may be missing from your tool list; find and call them here. Give a few keywords and get the best matching tools of every enabled integration, with their input schemas.","inputSchema":{"type":"object","properties":{"query":{"type":"string","description":"A few keywords for what the tool should do."}},"required":["query"]}},
        {"name":"call_integration_tool","description":"Call a tool found with find_integration_tools by its name, for an integration that is missing from your tool list. Asking and review work as for any other call.","inputSchema":{"type":"object","properties":{"name":{"type":"string","description":"The tool name as find_integration_tools returned it."},"arguments":{"type":"object"}},"required":["name"]}},
    ])
}

pub async fn listed(caller: &Caller<'_>) -> ApiResult<Vec<Value>> {
    let state = caller.state;
    let owner = caller.owner().await?;
    let group = caller.group_id().await?;
    let servers: Vec<_> = list_owned(state, owner).await?.into_iter().filter(|server| server.enabled && server.in_group(group)).collect();
    let http = &state.http;
    let fetched = join_all(servers.iter().filter(|server| server.integration().is_none()).map(|server| async move {
        let token = match bearer(state, server).await {
            Ok(token) => token,
            Err(error) => return (server, Err(Some(error.message().to_string()))),
        };
        let mut connection = McpConnection::new(http, &server.url, token.as_deref(), server.key_header());
        let mut tools = connection.list_tools().await;

        if matches!(tools, Err(McpError::Rejected)) && refreshable_after_rejection(server) {
            match renew(state, server).await {
                Ok(token) => {
                    connection.set_token(token.as_deref());
                    tools = connection.list_tools().await;
                }
                Err(error) => return (server, Err(Some(error.message().to_string()))),
            }
        }

        (server, tools.map_err(|_| None))
    }))
    .await;
    let mut tools = Vec::new();

    for server in servers.iter().filter(|server| server.integration().is_some()) {
        for tool in server.tool_list() {
            tools.push(integration_tool_value(&server.name, &tool.name, &tool.description, tool.input_schema));
        }
    }

    for (server, remote) in fetched {
        match remote {
            Ok(remote) => {
                for tool in remote {
                    tools.push(integration_tool_value(&server.name, &tool.name, &tool.description, tool.input_schema));
                }
            }
            Err(Some(message)) => tools.push(json!({"name": format!("{}__sign_in_again", slug(&server.name)), "description": format!("{}: {message}", server.name), "inputSchema": {"type": "object"}})),
            Err(None) => {}
        }
    }

    Ok(tools)
}

fn integration_tool_value(server_name: &str, name: &str, description: &str, input_schema: Value) -> Value {
    json!({"name": format!("{}__{}", slug(server_name), name), "description": format!("{}: {} {}", server_name, description, crate::file_arguments::ADVERT), "inputSchema": response_editor::with_filter_property(input_schema)})
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, arguments, .. } = call;
    let query = arguments.get("query").and_then(Value::as_str).unwrap_or_default();
    let candidates = listed(&caller)
        .await?
        .into_iter()
        .map(|tool| {
            let name = tool["name"].as_str().unwrap_or_default().to_string();
            let (server, original) = name.split_once("__").map(|(server, original)| (server.to_string(), original.to_string())).unwrap_or_default();

            chat_tools::Candidate { sort_key: name, server, tool: original, description: tool["description"].as_str().unwrap_or_default().to_string(), value: tool }
        })
        .collect();
    let found = chat_tools::best_matches(query, candidates);

    if found.is_empty() {
        return Ok(ToolReply::text("No integration tools match those keywords. Try other words, for example a verb and a noun.", false));
    }

    let entries: Vec<String> = found.iter().map(|tool| format!("{}\n{}\nInput schema: {}", tool["name"].as_str().unwrap_or_default(), tool["description"].as_str().unwrap_or_default(), tool["inputSchema"])).collect();

    Ok(ToolReply::text(&format!("Call these with call_integration_tool, giving the name and the arguments.\n\n{}", entries.join("\n\n")), false))
}
