use super::call::{ToolCall, ToolReply};
use crate::decision_providers::{ask, box_of, provider_for};
use crate::environments::{EnvironmentRecord, Starter, launch_app, record_action, start_environment};
use crate::error::ApiResult;
use crate::nodes::node_offline;
use crate::state::{AppState, EnvironmentInputAnswer, EnvironmentScreenshotAnswer};
use crate::store::find_owned;
use mycelium_proto::api::{EnvironmentController, EnvironmentState, EnvironmentTarget, StartEnvironmentRequest};
use mycelium_proto::link::{ControlToNode, EnvironmentInputAction};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use uuid::Uuid;

const MIN_BOX_CONFIDENCE: f64 = 0.3;
const NO_BROWSER_PROVIDER_FOR_TARGET: &str = "No decisions provider has Browser tasks switched on, so a target cannot be found. Take a screenshot and click with x and y instead.";
const NODE_TIMEOUT: Duration = Duration::from_secs(15);
const NO_ENVIRONMENT: &str = "No environment is running. Ask the person to start one from the Environment panel.";
const DEFAULT_SCROLL_AMOUNT: u64 = 3;
const MAX_SCROLL_AMOUNT: u64 = 50;
const DIRECTIONS: [&str; 4] = ["up", "down", "left", "right"];
const BUTTONS: [&str; 3] = ["left", "right", "middle"];

pub fn definitions() -> Value {
    json!([
        {
            "name": "environment_start",
            "description": "Start the environment for this conversation: a separate desktop with its own browser. It runs on a node that can run environments; when several are online, the one the person used last. Does nothing if one is already running. Take a screenshot afterwards to see it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "apps": {"type": "array", "items": {"type": "string"}, "description": "App ids to open on start, from the node's apps. Defaults to the browser."},
                },
            },
        },
        {
            "name": "environment_screenshot",
            "description": "Take a screenshot of the environment: a separate desktop with its own browser that the person started for you. It is never the person's own screen. Coordinates for the other environment tools are pixels in this image, from the top left. Returns an error if no environment is running; start one with environment_start.",
            "inputSchema": {"type": "object", "properties": {}},
        },
        {
            "name": "environment_click",
            "description": "Click in the environment, at a position or on a described control. Give x and y from a screenshot, or give target, a description such as \"the Send button\", and Mycelium finds it on a fresh screenshot and clicks its centre. Target works in any app and is quick, but needs a decisions provider with Browser tasks switched on. If the target is not found, click by position.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "x": {"type": "integer", "description": "Pixels from the left edge of the screenshot."},
                    "y": {"type": "integer", "description": "Pixels from the top edge of the screenshot."},
                    "target": {"type": "string", "description": "What to click, described, instead of x and y, for example \"the Send button\"."},
                    "label": {"type": "string", "description": "What you are clicking, in the person's words, for example \"Password\". It is shown to the person as you work. Optional with target."},
                    "button": {"type": "string", "enum": ["left", "right", "middle"], "description": "Which mouse button. Defaults to left."},
                },
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
        {
            "name": "environment_launch",
            "description": "Open an application in the environment, for example a file manager or an editor. The environment's own message lists the apps the node offers if the one you ask for is not among them. The new window is focused and becomes yours to drive.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "app": {"type": "string", "description": "The app id the node offers, for example \"org.gnome.Nautilus\"."},
                    "file": {"type": "string", "description": "A file or address to open with the app."},
                },
                "required": ["app"],
            },
        },
    ])
}

fn text_argument(arguments: &Value, name: &str) -> Option<String> {
    arguments.get(name).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(String::from)
}

async fn take_screenshot(state: &AppState, node_id: Uuid, conversation_id: Uuid) -> Result<String, ToolReply> {
    let answered = state.ask_node::<EnvironmentScreenshotAnswer>(node_id, NODE_TIMEOUT, |request_id| ControlToNode::EnvironmentScreenshot { request_id, conversation_id }).await;
    let Some(answer) = answered else {
        return Err(ToolReply::text(node_offline().message(), true));
    };

    if let Some(error) = answer.error {
        return Err(ToolReply::text(&error, true));
    }

    answer.png.ok_or_else(|| ToolReply::text("The screenshot came back empty. Try again.", true))
}

pub fn agent_environment(state: &AppState, conversation_id: Uuid) -> Result<EnvironmentRecord, ToolReply> {
    let Some(record) = state.hub.environment_of(conversation_id).filter(|record| record.is_live()) else {
        return Err(ToolReply::text(NO_ENVIRONMENT, true));
    };

    if record.environment.state == EnvironmentState::Starting {
        return Err(ToolReply::text("The environment is still starting. Try again in a moment.", true));
    }

    if record.environment.controller == EnvironmentController::Person {
        return Err(ToolReply::text(&format!("{} has taken over the environment; wait until they hand back", record.person), true));
    }

    Ok(record)
}

fn pixel_argument(arguments: &Value, name: &str) -> Option<u32> {
    arguments.get(name).and_then(Value::as_u64).and_then(|value| u32::try_from(value).ok())
}

pub async fn call(call: ToolCall<'_>) -> ApiResult<ToolReply> {
    let ToolCall { caller, tool, arguments, .. } = call;
    let state = caller.state;
    let mut target = None;
    let mut launch = None;
    let mut find = None;
    let (input, logged, mut done) = match tool {
        "environment_launch" => {
            let Some(app) = text_argument(&arguments, "app") else {
                return Ok(ToolReply::invalid("environment_launch needs app, the id of an app the node offers."));
            };

            launch = Some((app, text_argument(&arguments, "file")));

            (None, String::new(), String::new())
        }
        "environment_click" => {
            let point = pixel_argument(&arguments, "x").zip(pixel_argument(&arguments, "y"));
            let described = text_argument(&arguments, "target");
            let Some((x, y)) = point.or(described.as_ref().map(|_| (0, 0))) else {
                return Ok(ToolReply::invalid("environment_click needs x and y, in pixels from the top left of the screenshot, or a target describing what to click."));
            };
            let Some(label) = text_argument(&arguments, "label").or_else(|| described.clone()) else {
                return Ok(ToolReply::invalid("environment_click needs a label saying what you are clicking, for example \"Password\". The person sees it as you work."));
            };
            let button = text_argument(&arguments, "button");

            if button.as_deref().is_some_and(|button| !BUTTONS.contains(&button)) {
                return Ok(ToolReply::invalid("environment_click button must be left, right or middle."));
            }

            if point.is_some() {
                target = Some(EnvironmentTarget { x, y, label: label.clone() });
            } else {
                find = described;
            }

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

    if tool == "environment_start" {
        if let Some(running) = state.hub.environment_of(conversation_id).filter(EnvironmentRecord::is_live) {
            return Ok(ToolReply::text(&format!("The environment is already running on {}.", running.environment.node_name), false));
        }

        let apps = match arguments.get("apps") {
            None | Some(Value::Null) => None,
            Some(Value::Array(chosen)) => Some(chosen.iter().filter_map(Value::as_str).map(String::from).collect()),
            Some(_) => return Ok(ToolReply::invalid("environment_start apps must be a list of app ids.")),
        };
        let owner = caller.owner().await?;
        let row = find_owned(&state.pool, conversation_id, owner).await?;
        let person = sqlx::query_scalar::<_, String>("select name from users where id = $1").bind(owner).fetch_one(&state.pool).await?;
        let started = start_environment(state, &row, &person, StartEnvironmentRequest { width: None, height: None, apps, node_id: None }, Starter::Agent).await;

        return Ok(match started {
            Ok(record) => ToolReply::text(&format!("Started the environment on {}. Take a screenshot to see it once it is running.", record.environment.node_name), false),
            Err(error) => ToolReply::text(error.message(), true),
        });
    }
    let record = match agent_environment(state, conversation_id) {
        Ok(record) => record,
        Err(reply) => return Ok(reply),
    };

    let node_id = record.environment.node_id;
    let unreachable = || ToolReply::text(node_offline().message(), true);
    let mut shot = None;

    if let Some((app, file)) = launch {
        let launched = launch_app(state, &record, &app, file).await;
        let (offer, window) = match launched {
            Ok(launched) => launched,
            Err(error) => return Ok(ToolReply::text(error.message(), true)),
        };
        let mut resumed = false;

        record_action(state, conversation_id, &format!("Opened {}", offer.name), |record| {
            resumed = std::mem::take(&mut record.handed_back);

            if window.is_some() {
                record.environment.agent_window = window;
            }
        });

        let done = format!("Opened {}. Take a screenshot to see it.", offer.name);

        if resumed {
            return Ok(ToolReply::text(&format!("{} used the environment while you were paused, so what you saw earlier may be out of date.\n\n{done}", record.person), false));
        }

        return Ok(ToolReply::text(&done, false));
    }

    match input {
        Some(mut action) => {
            if let Some(description) = find {
                let owner = caller.owner().await?;
                let Some(provider) = provider_for(state, owner, "browser").await? else {
                    return Ok(ToolReply::text(NO_BROWSER_PROVIDER_FOR_TARGET, true));
                };
                let png = match take_screenshot(state, node_id, conversation_id).await {
                    Ok(png) => png,
                    Err(reply) => return Ok(reply),
                };
                let questions = json!({
                    "present": {"type": "noul", "instructions": format!("Is {description} visible in `screen`?")},
                    "target": {"type": "box", "instructions": format!("{description} in `screen`")},
                });
                let started = Instant::now();
                let asked = ask(state, &provider.base_url, provider.api_key.as_deref(), &provider.model, &json!("A screenshot of the environment."), &questions, Some(&png)).await;
                let seconds = started.elapsed().as_secs_f64();
                let answers = match asked {
                    Ok(answers) => answers,
                    Err(message) => return Ok(ToolReply::text(&message, true)),
                };
                let confident = answers["present"]["noul"].as_f64().is_some_and(|present| present >= MIN_BOX_CONFIDENCE) && answers["target"]["confidence"].as_f64().is_none_or(|confidence| confidence >= MIN_BOX_CONFIDENCE);
                let Some((top, bottom, left, right)) = box_of(&answers["target"]).filter(|_| confident) else {
                    return Ok(ToolReply::text(&format!("Could not find {description} on screen"), true));
                };
                let size = record.environment.size;
                let (scale_x, scale_y) = (f64::from(size.width), f64::from(size.height));
                let x = (((left + right) / 2.0 * scale_x).round().max(0.0) as u32).min(size.width.saturating_sub(1));
                let y = (((top + bottom) / 2.0 * scale_y).round().max(0.0) as u32).min(size.height.saturating_sub(1));

                if let EnvironmentInputAction::Click { x: at_x, y: at_y, .. } = &mut action {
                    *at_x = x;
                    *at_y = y;
                }

                done = format!("{} Found at {x}, {y} by {} in {seconds:.1} s.", done.trim_end(), provider.name);
                target = Some(EnvironmentTarget { x, y, label: description });
            }

            let answered = state.ask_node::<EnvironmentInputAnswer>(node_id, NODE_TIMEOUT, |request_id| ControlToNode::EnvironmentInput { request_id, conversation_id, action }).await;
            let Some(answer) = answered else {
                return Ok(unreachable());
            };

            if let Some(error) = answer.error {
                return Ok(ToolReply::text(&error, true));
            }
        }
        None => match take_screenshot(state, node_id, conversation_id).await {
            Ok(png) => shot = Some(png),
            Err(reply) => return Ok(reply),
        },
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
