use super::call::{ToolCall, ToolReply};
use crate::error::ApiResult;
use mycelium_proto::stream::StreamMessage;
use serde_json::{Value, json};

const ROUTE_LIMIT: usize = 200;

pub fn definitions() -> Value {
    json!([
        {
            "name": "open_screen",
            "description": "Open a screen in the Mycelium app for the person, for example /settings/privacy or /assistants. It only navigates and needs no approval.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route": {"type": "string", "description": "The in-app route to open. It starts with a slash."},
                },
                "required": ["route"],
            },
        },
    ])
}

fn valid_route(route: &str) -> bool {
    route.starts_with('/') && !route.starts_with("//") && route.len() <= ROUTE_LIMIT && !route.contains("://") && !route.chars().any(char::is_control)
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, arguments, .. } = call;
    let route = arguments.get("route").and_then(Value::as_str).map(str::trim).filter(|route| !route.is_empty());
    let Some(route) = route else {
        return Ok(ToolReply::invalid("open_screen needs the route to open, for example /settings/privacy."));
    };

    if !valid_route(route) {
        return Ok(ToolReply::text("The route must be a path inside the app that starts with a slash, for example /settings/privacy.", true));
    }

    let owner = caller.owner().await?;

    caller.state.publish(Some(owner), StreamMessage::OpenScreen { route: route.to_string() });

    Ok(ToolReply::text(&format!("Opened {route}."), false))
}
