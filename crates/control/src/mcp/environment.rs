use super::call::{ToolCall, ToolReply};
use crate::environments::record_action;
use crate::error::ApiResult;
use crate::nodes::node_offline;
use crate::state::{EnvironmentInputAnswer, EnvironmentScreenshotAnswer};
use mycelium_proto::api::{EnvironmentController, EnvironmentState, EnvironmentTarget};
use mycelium_proto::link::{ControlToNode, EnvironmentInputAction};
use serde_json::{Value, json};
use std::time::Duration;

const NODE_TIMEOUT: Duration = Duration::from_secs(15);
const NO_ENVIRONMENT: &str = "No environment is running. Ask the person to start one from the Environment panel.";
const DEFAULT_SCROLL_AMOUNT: u64 = 3;
const MAX_SCROLL_AMOUNT: u64 = 50;
const DIRECTIONS: [&str; 4] = ["up", "down", "left", "right"];
const BUTTONS: [&str; 3] = ["left", "right", "middle"];

pub fn definitions() -> Value {
    json!([
        {
            "name": "environment_screenshot",
            "description": "Take a screenshot of the environment: a separate desktop with its own browser that the person started for you. It is never the person's own screen. Coordinates for the other environment tools are pixels in this image, from the top left. Returns an error if no environment is running; you cannot start one.",
            "inputSchema": {"type": "object", "properties": {}},
        },
        {
            "name": "environment_click",
            "description": "Click at a position in the environment. Take a screenshot first to find the position.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "x": {"type": "integer", "description": "Pixels from the left edge of the screenshot."},
                    "y": {"type": "integer", "description": "Pixels from the top edge of the screenshot."},
                    "label": {"type": "string", "description": "What you are clicking, in the person's words, for example \"Password\". It is shown to the person as you work."},
                    "button": {"type": "string", "enum": ["left", "right", "middle"], "description": "Which mouse button. Defaults to left."},
                },
                "required": ["x", "y", "label"],
            },
        },
        {
            "name": "environment_type",
            "description": "Type text into whatever has focus in the environment. Click the field first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "The text to type."},
                    "into": {"type": "string", "description": "Which field you are typing into, for example \"Email\". The text itself is never shown in the log."},
                },
                "required": ["text", "into"],
            },
        },
        {
            "name": "environment_scroll",
            "description": "Scroll in the environment.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "direction": {"type": "string", "enum": ["up", "down", "left", "right"]},
                    "amount": {"type": "integer", "description": "How many steps to scroll, 1 to 50. Defaults to 3."},
                    "label": {"type": "string", "description": "What you are scrolling, for example \"Results\"."},
                },
                "required": ["direction"],
            },
        },
        {
            "name": "environment_key",
            "description": "Press a key or key combination in the environment, for example Enter, Escape or Ctrl+L.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "keys": {"type": "string", "description": "The key or combination, joined with +."},
                },
                "required": ["keys"],
            },
        },
    ])
}

fn text_argument(arguments: &Value, name: &str) -> Option<String> {
    arguments.get(name).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(String::from)
}

fn pixel_argument(arguments: &Value, name: &str) -> Option<u32> {
    arguments.get(name).and_then(Value::as_u64).and_then(|value| u32::try_from(value).ok())
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, tool, arguments, .. } = call;
    let state = caller.state;
    let mut target = None;
    let (input, logged, done) = match tool {
        "environment_click" => {
            let (Some(x), Some(y)) = (pixel_argument(&arguments, "x"), pixel_argument(&arguments, "y")) else {
                return Ok(ToolReply::invalid("environment_click needs x and y, in pixels from the top left of the screenshot."));
            };
            let Some(label) = text_argument(&arguments, "label") else {
                return Ok(ToolReply::invalid("environment_click needs a label saying what you are clicking, for example \"Password\". The person sees it as you work."));
            };
            let button = text_argument(&arguments, "button");

            if button.as_deref().is_some_and(|button| !BUTTONS.contains(&button)) {
                return Ok(ToolReply::invalid("environment_click button must be left, right or middle."));
            }

            target = Some(EnvironmentTarget { x, y, label: label.clone() });

            (Some(EnvironmentInputAction::Click { x, y, button }), format!("Clicking “{label}”"), format!("Clicked “{label}”."))
        }
        "environment_type" => {
            let Some(into) = text_argument(&arguments, "into") else {
                return Ok(ToolReply::invalid("environment_type needs into, saying which field you are typing into, for example \"Email\". The person sees it as you work."));
            };
            let Some(text) = arguments.get("text").and_then(Value::as_str).filter(|text| !text.is_empty()) else {
                return Ok(ToolReply::invalid("environment_type needs the text to type."));
            };

            (Some(EnvironmentInputAction::Type { text: text.to_string() }), format!("Typed into “{into}”"), format!("Typed into “{into}”."))
        }
        "environment_scroll" => {
            let Some(direction) = text_argument(&arguments, "direction").filter(|direction| DIRECTIONS.contains(&direction.as_str())) else {
                return Ok(ToolReply::invalid("environment_scroll direction must be up, down, left or right."));
            };
            let amount = arguments.get("amount").and_then(Value::as_u64).unwrap_or(DEFAULT_SCROLL_AMOUNT).clamp(1, MAX_SCROLL_AMOUNT) as u32;
            let logged = match text_argument(&arguments, "label") {
                Some(label) => format!("Scrolling {direction} in “{label}”"),
                None => format!("Scrolling {direction}"),
            };

            (Some(EnvironmentInputAction::Scroll { direction: direction.clone(), amount, x: None, y: None }), logged, format!("Scrolled {direction}."))
        }
        "environment_key" => {
            let Some(keys) = text_argument(&arguments, "keys") else {
                return Ok(ToolReply::invalid("environment_key needs the key or combination to press, for example Enter or Ctrl+L."));
            };

            (Some(EnvironmentInputAction::Key { keys: keys.clone() }), format!("Pressing {keys}"), format!("Pressed {keys}."))
        }
        _ => (None, "Took a screenshot".to_string(), String::new()),
    };

    let Some(conversation_id) = caller.conversation_id().await? else {
        return Ok(ToolReply::text("The environment belongs to a conversation, so use this tool from one.", true));
    };
    let record = state.hub.environment_of(conversation_id).filter(|record| record.is_live());
    let Some(record) = record else {
        return Ok(ToolReply::text(NO_ENVIRONMENT, true));
    };

    if record.environment.state == EnvironmentState::Starting {
        return Ok(ToolReply::text("The environment is still starting. Try again in a moment.", true));
    }

    if record.environment.controller == EnvironmentController::Person {
        return Ok(ToolReply::text(&format!("{} has taken over the environment; wait until they hand back", record.person), true));
    }

    let node_id = record.environment.node_id;
    let unreachable = || ToolReply::text(node_offline().message(), true);
    let mut shot = None;

    match input {
        Some(action) => {
            let answered = state.ask_node::<EnvironmentInputAnswer>(node_id, NODE_TIMEOUT, |request_id| ControlToNode::EnvironmentInput { request_id, conversation_id, action }).await;
            let Some(answer) = answered else {
                return Ok(unreachable());
            };

            if let Some(error) = answer.error {
                return Ok(ToolReply::text(&error, true));
            }
        }
        None => {
            let answered = state.ask_node::<EnvironmentScreenshotAnswer>(node_id, NODE_TIMEOUT, |request_id| ControlToNode::EnvironmentScreenshot { request_id, conversation_id }).await;
            let Some(answer) = answered else {
                return Ok(unreachable());
            };

            if let Some(error) = answer.error {
                return Ok(ToolReply::text(&error, true));
            }

            let Some(png) = answer.png else {
                return Ok(ToolReply::text("The screenshot came back empty. Try again.", true));
            };

            shot = Some(png);
        }
    }

    let mut resumed = false;

    record_action(state, conversation_id, &logged, |record| {
        resumed = std::mem::take(&mut record.handed_back);

        if target.is_some() {
            record.environment.target = target;
        }
    });

    if let Some(png) = shot {
        return Ok(ToolReply::Image { png });
    }

    if resumed {
        return Ok(ToolReply::text(&format!("{} used the environment while you were paused, so what you saw earlier may be out of date. Take a screenshot before relying on it.\n\n{done}", record.person), false));
    }

    Ok(ToolReply::text(&done, false))
}
